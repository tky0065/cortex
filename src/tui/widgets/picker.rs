use crate::tui::theme::THEME;
/// Generic searchable picker popup widget.
///
/// Renders a centered popup with:
///   - a title and "esc" hint
///   - a live search bar
///   - grouped rows (section headers + items)
///   - the currently-active item highlighted in orange
///   - a stable checkbox column beside items that are "checked"
///
/// Caller maintains `PickerState` and drives it with `PickerState::handle_*` helpers.
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// A single row in the picker list.
#[derive(Debug, Clone)]
pub struct PickerItem {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
    pub checked: bool,
}

/// A named group of items.
#[derive(Debug, Clone)]
pub struct PickerGroup {
    pub title: String,
    pub items: Vec<PickerItem>,
}

// ---------------------------------------------------------------------------
// PickerState — call this from the App to maintain picker state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PickerState {
    pub title: String,
    pub groups: Vec<PickerGroup>,
    pub search: String,
    /// Flat index into the filtered item list (excludes section headers).
    pub cursor: usize,
}

impl PickerState {
    pub fn new(title: impl Into<String>, groups: Vec<PickerGroup>) -> Self {
        Self {
            title: title.into(),
            groups,
            search: String::new(),
            cursor: 0,
        }
    }

    /// Filtered items matching the current search string.
    pub fn filtered(&self) -> Vec<(/*group_title*/ &str, &PickerItem)> {
        let query = normalize_search_text(&self.search);
        let terms: Vec<&str> = query.split_whitespace().collect();
        let mut matches: Vec<(usize, usize, &str, &PickerItem)> = Vec::new();
        let mut original_index = 0usize;
        for group in &self.groups {
            for item in &group.items {
                let label = normalize_search_text(&item.label);
                let description =
                    normalize_search_text(item.description.as_deref().unwrap_or_default());
                let searchable = format!("{label} {description}");
                if terms.iter().all(|term| searchable.contains(term)) {
                    let rank = if query.is_empty() || label == query {
                        0
                    } else if label.starts_with(&query) {
                        1
                    } else if terms.iter().all(|term| label.contains(term)) {
                        2
                    } else {
                        3
                    };
                    matches.push((rank, original_index, group.title.as_str(), item));
                }
                original_index += 1;
            }
        }
        matches.sort_by_key(|(rank, original_index, _, _)| (*rank, *original_index));
        matches
            .into_iter()
            .map(|(_, _, group, item)| (group, item))
            .collect()
    }

    pub fn move_up(&mut self) {
        self.move_by(-1);
    }

    pub fn move_down(&mut self) {
        self.move_by(1);
    }

    pub fn move_by(&mut self, delta: isize) {
        let len = self.filtered().len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = self
            .cursor
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    pub fn page_up(&mut self) {
        self.move_by(-10);
    }

    pub fn page_down(&mut self) {
        self.move_by(10);
    }

    pub fn move_first(&mut self) {
        self.cursor = 0;
    }

    pub fn move_last(&mut self) {
        self.cursor = self.filtered().len().saturating_sub(1);
    }

    pub fn push_search(&mut self, c: char) {
        self.search.push(c);
        self.cursor = 0;
    }

    pub fn pop_search(&mut self) {
        self.search.pop();
        self.cursor = 0;
    }

    /// Return the `id` of the currently-highlighted item, if any.
    pub fn selected_id(&self) -> Option<String> {
        self.filtered()
            .get(self.cursor)
            .map(|(_, item)| item.id.clone())
    }

    pub fn checked_ids(&self) -> Vec<String> {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .filter(|item| item.checked)
            .map(|item| item.id.clone())
            .collect()
    }

    pub fn checked_count(&self) -> usize {
        self.groups
            .iter()
            .flat_map(|group| group.items.iter())
            .filter(|item| item.checked)
            .count()
    }

    pub fn toggle_selected(&mut self) -> Option<(String, bool)> {
        let id = self.selected_id()?;
        for item in self
            .groups
            .iter_mut()
            .flat_map(|group| group.items.iter_mut())
        {
            if item.id == id {
                item.checked = !item.checked;
                return Some((id, item.checked));
            }
        }
        None
    }

    pub fn remove_item(&mut self, id: &str) {
        for group in &mut self.groups {
            group.items.retain(|item| item.id != id);
        }
        let len = self.filtered().len();
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }
}

fn normalize_search_text(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub struct PickerWidget<'a> {
    pub state: &'a PickerState,
}

impl<'a> PickerWidget<'a> {
    pub fn render(&self, frame: &mut Frame) {
        let area = centered_rect(60, 70, frame.area());
        frame.render_widget(Clear, area);

        let outer_block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border_style())
            .style(Style::default().bg(THEME.bg));
        let inner = outer_block.inner(area);
        frame.render_widget(outer_block, area);

        // Split inner: title row / search bar / metadata / list / shortcuts
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title + esc hint
                Constraint::Length(1), // spacer
                Constraint::Length(1), // search
                Constraint::Length(1), // result count + cursor position
                Constraint::Min(1),    // list
                Constraint::Length(1), // shortcuts
            ])
            .split(inner);

        // Title row
        let title_line = Line::from(vec![
            Span::styled(format!(" {} ", self.state.title), THEME.title_style()),
            Span::raw("  "),
            Span::styled(
                "esc to close",
                Style::default()
                    .fg(THEME.muted)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]);
        frame.render_widget(Paragraph::new(title_line), chunks[0]);

        // Search bar
        let search_text = format!("  🔍 {} ", self.state.search);
        frame.render_widget(
            Paragraph::new(search_text)
                .style(Style::default().fg(THEME.text).bg(Color::Rgb(30, 41, 59))),
            chunks[2],
        );

        let filtered = self.state.filtered();
        let total = self
            .state
            .groups
            .iter()
            .map(|group| group.items.len())
            .sum::<usize>();
        let position = if filtered.is_empty() {
            "position 0/0".to_string()
        } else {
            format!(
                "position {}/{}",
                self.state.cursor.min(filtered.len() - 1) + 1,
                filtered.len()
            )
        };
        frame.render_widget(
            Paragraph::new(format!(
                "  {} results / {} total  •  {}",
                filtered.len(),
                total,
                position
            ))
            .style(Style::default().fg(THEME.muted)),
            chunks[3],
        );

        // Build list rows — grouped with section headers
        let mut rows: Vec<ListItem> = Vec::new();
        let mut last_group = "";

        for (flat_idx, (group_title, item)) in filtered.iter().enumerate() {
            // Section header when group changes
            if *group_title != last_group {
                rows.push(ListItem::new(Line::from(Span::styled(
                    format!(" 📂 {}", group_title),
                    Style::default()
                        .fg(THEME.secondary)
                        .add_modifier(Modifier::BOLD),
                ))));
                last_group = group_title;
            }

            let is_cursor = flat_idx == self.state.cursor;
            let (cursor_style, check_style, label_style, desc_style, row_bg) = if is_cursor {
                (
                    Style::default()
                        .bg(THEME.primary)
                        .fg(THEME.bg)
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .bg(THEME.primary)
                        .fg(if item.checked {
                            THEME.success
                        } else {
                            THEME.bg
                        })
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .bg(THEME.primary)
                        .fg(THEME.bg)
                        .add_modifier(Modifier::BOLD),
                    Style::default()
                        .bg(THEME.primary)
                        .fg(Color::Rgb(30, 41, 59)),
                    THEME.primary,
                )
            } else {
                (
                    Style::default().fg(THEME.muted),
                    Style::default()
                        .fg(if item.checked {
                            THEME.success
                        } else {
                            THEME.muted
                        })
                        .add_modifier(if item.checked {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                    Style::default().fg(THEME.text),
                    Style::default().fg(THEME.muted),
                    Color::Reset,
                )
            };

            let cursor = if is_cursor { ">" } else { " " };
            let check = if item.checked { "[x]" } else { "[ ]" };
            let mut spans = vec![
                Span::styled(format!(" {} ", cursor), cursor_style),
                Span::styled(format!("{} ", check), check_style),
                Span::styled(format!(" {} ", item.label), label_style),
            ];
            if let Some(desc) = &item.description {
                spans.push(Span::styled(format!("  {}", desc), desc_style));
            }
            rows.push(ListItem::new(Line::from(spans)).style(Style::default().bg(row_bg)));
        }

        // Scroll so cursor stays visible
        let mut list_state = ListState::default();
        // We need to map cursor (flat item index) to list row index (includes headers)
        let row_idx = cursor_to_row_idx(&filtered, self.state.cursor);
        list_state.select(Some(row_idx));

        let list = List::new(rows).style(Style::default().bg(THEME.bg));

        frame.render_stateful_widget(list, chunks[4], &mut list_state);
        frame.render_widget(
            Paragraph::new("  ↑↓ row  PgUp/PgDn page  Home/End  wheel  Enter select  Esc back")
                .style(Style::default().fg(THEME.muted)),
            chunks[5],
        );
    }
}

/// Convert flat item cursor to list row index accounting for section headers.
fn cursor_to_row_idx(filtered: &[(&str, &PickerItem)], cursor: usize) -> usize {
    let mut row = 0usize;
    let mut last_group = "";
    for (flat, (group, _item)) in filtered.iter().enumerate() {
        if *group != last_group {
            row += 1; // section header
            last_group = group;
        }
        if flat == cursor {
            return row;
        }
        row += 1;
    }
    row
}

// ---------------------------------------------------------------------------
// Centered rect helper
// ---------------------------------------------------------------------------

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ---------------------------------------------------------------------------
// Preset factory helpers
// ---------------------------------------------------------------------------

/// Build a PickerState for provider selection.
/// `current` is the currently configured default provider string.
pub fn provider_picker(
    current: &str,
    custom_providers: &std::collections::BTreeMap<String, crate::config::CustomProviderConfig>,
) -> PickerState {
    let mut local = Vec::new();
    let mut hosted = Vec::new();
    let mut aggregators = Vec::new();

    for provider in crate::providers::registry::BUILTIN_PROVIDERS {
        let item = PickerItem {
            id: provider.id.to_string(),
            label: provider.name.to_string(),
            description: Some(provider.description.to_string()),
            checked: current == provider.id,
        };
        match provider.kind {
            crate::providers::registry::ProviderKind::Local => local.push(item),
            crate::providers::registry::ProviderKind::Hosted => hosted.push(item),
            crate::providers::registry::ProviderKind::Aggregator => aggregators.push(item),
            crate::providers::registry::ProviderKind::Custom => {}
        }
    }

    let mut providers = Vec::new();
    if !local.is_empty() {
        providers.push(PickerGroup {
            title: "Local".to_string(),
            items: local,
        });
    }
    if !hosted.is_empty() {
        providers.push(PickerGroup {
            title: "Hosted".to_string(),
            items: hosted,
        });
    }
    if !aggregators.is_empty() {
        providers.push(PickerGroup {
            title: "Aggregators".to_string(),
            items: aggregators,
        });
    }
    if !custom_providers.is_empty() {
        providers.push(PickerGroup {
            title: "Custom".to_string(),
            items: custom_providers
                .iter()
                .map(|(id, provider)| PickerItem {
                    id: id.clone(),
                    label: id.clone(),
                    description: Some(provider.base_url.clone()),
                    checked: current == id,
                })
                .collect(),
        });
    }
    PickerState::new("Connect a provider", providers)
}

pub fn auth_method_picker(provider: &str, methods: &[crate::auth::AuthMethodSpec]) -> PickerState {
    let items = methods
        .iter()
        .map(|method| PickerItem {
            id: method.id.to_string(),
            label: method.label.to_string(),
            description: Some(method.description.to_string()),
            checked: false,
        })
        .collect();
    PickerState::new(
        format!("Connect {provider}"),
        vec![PickerGroup {
            title: "Auth methods".to_string(),
            items,
        }],
    )
}

/// Build a PickerState for model/role selection.
/// `current_models` maps role id → model string (used for the description).
pub fn model_picker(current_models: &[(&str, &str)]) -> PickerState {
    let items = current_models
        .iter()
        .map(|(role, model)| PickerItem {
            id: role.to_string(),
            label: role.to_string(),
            description: Some(model.to_string()),
            checked: false,
        })
        .collect();
    let groups = vec![PickerGroup {
        title: "Roles — select to edit".to_string(),
        items,
    }];
    PickerState::new("Set model per role", groups)
}

/// Build a PickerState listing past sessions for the current project.
///
/// Workflow runs (from `session_history`) and persisted chats are merged into a
/// single, flat, newest-first list. Each row shows a relative time. Item ids are
/// prefixed `wf:` or `chat:` so the handler knows how to resume them.
pub fn resume_picker(
    workflows: &[crate::repl::SessionInfo],
    chats: &[crate::chat_session::ChatSession],
) -> PickerState {
    if workflows.is_empty() && chats.is_empty() {
        let groups = vec![PickerGroup {
            title: "No sessions yet".to_string(),
            items: vec![PickerItem {
                id: "__empty__".to_string(),
                label: "No past sessions for this directory".to_string(),
                description: Some(
                    "Send a message to start a chat, or /start dev \"<idea>\"".to_string(),
                ),
                checked: false,
            }],
        }];
        return PickerState::new("Resume a session  esc to close", groups);
    }

    // (recency, PickerItem) so chats and workflows can be merged by time.
    let mut rows: Vec<(chrono::DateTime<chrono::Utc>, PickerItem)> = Vec::new();

    for c in chats {
        rows.push((
            c.updated_at,
            PickerItem {
                id: format!("chat:{}", c.id),
                label: format!("💬 {}", c.summary()),
                description: Some(format!(
                    "{} · {} turn{}",
                    crate::chat_session::humanize_since(c.updated_at),
                    c.turns.len(),
                    if c.turns.len() == 1 { "" } else { "s" }
                )),
                checked: false,
            },
        ));
    }

    for s in workflows {
        let idea = if s.idea.chars().count() > 55 {
            let truncated: String = s.idea.chars().take(55).collect();
            format!("{truncated}…")
        } else {
            s.idea.clone()
        };
        let status = match s.status {
            crate::repl::SessionStatus::Running => "running",
            crate::repl::SessionStatus::Interrupted => "interrupted",
            crate::repl::SessionStatus::Completed => "completed",
            crate::repl::SessionStatus::Failed => "failed",
        };
        rows.push((
            s.timestamp,
            PickerItem {
                id: format!("wf:{}", s.id),
                label: format!("[{}] {}", s.workflow, idea),
                description: Some(format!(
                    "{} · {}",
                    crate::chat_session::humanize_since(s.timestamp),
                    status
                )),
                checked: false,
            },
        ));
    }

    // Newest first.
    rows.sort_by_key(|(when, _)| std::cmp::Reverse(*when));

    let groups = vec![PickerGroup {
        title: "Sessions".to_string(),
        items: rows.into_iter().map(|(_, item)| item).collect(),
    }];
    PickerState::new("Resume a session  esc to close", groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn item(id: &str, label: &str, description: Option<&str>) -> PickerItem {
        PickerItem {
            id: id.to_string(),
            label: label.to_string(),
            description: description.map(str::to_string),
            checked: false,
        }
    }

    fn state_with_items(items: Vec<PickerItem>) -> PickerState {
        PickerState::new(
            "Models",
            vec![PickerGroup {
                title: "OpenRouter".to_string(),
                items,
            }],
        )
    }

    #[test]
    fn multi_word_search_matches_case_insensitively_across_fields() {
        let mut state = state_with_items(vec![
            item(
                "nemotron",
                "nvidia/nemotron-3-ultra",
                Some("Free reasoning model"),
            ),
            item("other", "openai/gpt-4o", Some("Paid general model")),
        ]);

        state.search = "NVIDIA FREE".to_string();

        assert_eq!(state.filtered()[0].1.id, "nemotron");
        assert_eq!(state.filtered().len(), 1);
    }

    #[test]
    fn search_ranks_exact_then_prefix_then_label_then_description() {
        let mut state = state_with_items(vec![
            item("description", "vendor/other", Some("deepseek r1 model")),
            item("contains", "vendor/deepseek-r1-chat", None),
            item("prefix", "deepseek-r1-distill", None),
            item("exact", "deepseek-r1", None),
        ]);

        state.search = "deepseek-r1".to_string();

        let ids: Vec<&str> = state
            .filtered()
            .into_iter()
            .map(|(_, item)| item.id.as_str())
            .collect();
        assert_eq!(ids, vec!["exact", "prefix", "contains", "description"]);
    }

    #[test]
    fn navigation_supports_steps_pages_and_boundaries() {
        let mut state = state_with_items(
            (0..25)
                .map(|index| item(&index.to_string(), &format!("model-{index}"), None))
                .collect(),
        );

        state.move_by(3);
        assert_eq!(state.cursor, 3);
        state.page_down();
        assert_eq!(state.cursor, 13);
        state.move_last();
        assert_eq!(state.cursor, 24);
        state.move_by(3);
        assert_eq!(state.cursor, 24);
        state.page_up();
        assert_eq!(state.cursor, 14);
        state.move_first();
        assert_eq!(state.cursor, 0);
        state.move_by(-3);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn search_edits_reset_cursor_to_first_result() {
        let mut state = state_with_items(vec![
            item("one", "model-one", None),
            item("two", "model-two", None),
        ]);
        state.move_last();

        state.push_search('t');

        assert_eq!(state.cursor, 0);
        assert_eq!(state.selected_id().as_deref(), Some("two"));
    }

    #[test]
    fn rendering_keeps_last_result_visible_in_long_lists() {
        let mut state = state_with_items(
            (0..40)
                .map(|index| item(&index.to_string(), &format!("model-{index}"), None))
                .collect(),
        );
        state.move_last();
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| PickerWidget { state: &state }.render(frame))
            .unwrap();

        let rendered = terminal.backend().to_string();
        assert!(rendered.contains("model-39"));
        assert!(!rendered.contains("model-0 "));
    }

    /// Collect every selectable item id across all groups in a picker.
    fn picker_ids(state: &PickerState) -> Vec<String> {
        state
            .filtered()
            .into_iter()
            .map(|(_, item)| item.id.clone())
            .collect()
    }

    fn chat(prompt: &str) -> crate::chat_session::ChatSession {
        let mut s = crate::chat_session::ChatSession::new(std::path::PathBuf::from("/tmp/proj"));
        s.turns.push(crate::chat_session::ChatTurn {
            prompt: prompt.to_string(),
            response: "ok".to_string(),
        });
        s
    }

    fn workflow(id: &str, name: &str, idea: &str) -> crate::repl::SessionInfo {
        crate::repl::SessionInfo {
            id: id.to_string(),
            workflow: name.to_string(),
            idea: idea.to_string(),
            directory: std::path::PathBuf::from("/tmp/proj"),
            timestamp: chrono::Utc::now(),
            status: crate::repl::SessionStatus::Completed,
            git_hash: None,
        }
    }

    #[test]
    fn resume_picker_merges_chats_and_workflows_newest_first() {
        // Workflow is older; chat is the most recent.
        let mut wf = workflow("wf1", "scanner", "scan le projet");
        wf.timestamp = chrono::Utc::now() - chrono::Duration::minutes(10);
        let chat = chat("most recent question");

        let state = resume_picker(std::slice::from_ref(&wf), std::slice::from_ref(&chat));
        let ids = picker_ids(&state);

        // Newest (chat) first, then the workflow; ids are prefixed by kind.
        assert_eq!(ids, vec![format!("chat:{}", chat.id), "wf:wf1".to_string()]);
        assert!(state.groups.iter().any(|g| g.title == "Sessions"));

        let wf_row = state.groups[0]
            .items
            .iter()
            .find(|i| i.id == "wf:wf1")
            .unwrap();
        assert!(wf_row.label.contains("[scanner]"));
        // Descriptions carry a relative time.
        assert!(wf_row.description.as_ref().unwrap().contains("ago"));
    }

    #[test]
    fn resume_picker_shows_empty_state_when_no_sessions() {
        let state = resume_picker(&[], &[]);
        let ids = picker_ids(&state);
        assert_eq!(ids, vec!["__empty__".to_string()]);
    }

    #[test]
    fn toggle_selected_updates_checked_count() {
        let mut state = PickerState::new(
            "Skills",
            vec![PickerGroup {
                title: "Remote".to_string(),
                items: vec![PickerItem {
                    id: "remote:find-skills".to_string(),
                    label: "find-skills".to_string(),
                    description: None,
                    checked: false,
                }],
            }],
        );

        assert_eq!(state.checked_count(), 0);
        assert_eq!(
            state.toggle_selected(),
            Some(("remote:find-skills".to_string(), true))
        );
        assert_eq!(state.checked_count(), 1);
        assert_eq!(state.checked_ids(), vec!["remote:find-skills"]);
    }
}
