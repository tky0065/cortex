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
        let q = self.search.to_lowercase();
        self.groups
            .iter()
            .flat_map(|g| {
                let q = q.clone();
                g.items
                    .iter()
                    .filter(move |item| {
                        q.is_empty()
                            || item.label.to_lowercase().contains(&q)
                            || item
                                .description
                                .as_deref()
                                .map(|d| d.to_lowercase().contains(&q))
                                .unwrap_or(false)
                    })
                    .map(move |item| (g.title.as_str(), item))
            })
            .collect()
    }

    pub fn move_up(&mut self) {
        let len = self.filtered().len();
        if len > 0 && self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let len = self.filtered().len();
        if len > 0 && self.cursor + 1 < len {
            self.cursor += 1;
        }
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

        // Split inner: title row / search bar / list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title + esc hint
                Constraint::Length(1), // spacer
                Constraint::Length(1), // search
                Constraint::Length(1), // spacer
                Constraint::Min(1),    // list
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

        // Build list rows — grouped with section headers
        let filtered = self.state.filtered();
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
pub fn provider_picker(current: &str) -> PickerState {
    let providers = vec![
        PickerGroup {
            title: "Local".to_string(),
            items: vec![PickerItem {
                id: "ollama".to_string(),
                label: "Ollama".to_string(),
                description: Some("local models via ollama".to_string()),
                checked: current == "ollama",
            }],
        },
        PickerGroup {
            title: "Cloud".to_string(),
            items: vec![
                PickerItem {
                    id: "openrouter".to_string(),
                    label: "OpenRouter".to_string(),
                    description: Some("API key required".to_string()),
                    checked: current == "openrouter",
                },
                PickerItem {
                    id: "groq".to_string(),
                    label: "Groq".to_string(),
                    description: Some("API key required — very fast inference".to_string()),
                    checked: current == "groq",
                },
                PickerItem {
                    id: "together".to_string(),
                    label: "Together AI".to_string(),
                    description: Some("API key required".to_string()),
                    checked: current == "together",
                },
            ],
        },
    ];
    PickerState::new("Connect a provider", providers)
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

/// Build a PickerState for session history resume selection.
/// Sessions are grouped by status: active/interrupted first, then completed, then failed.
pub fn resume_picker(sessions: &[crate::repl::SessionInfo]) -> PickerState {
    if sessions.is_empty() {
        let groups = vec![PickerGroup {
            title: "No sessions yet".to_string(),
            items: vec![PickerItem {
                id: "__empty__".to_string(),
                label: "No past sessions found".to_string(),
                description: Some("Use /start <workflow> \"<idea>\" to begin".to_string()),
                checked: false,
            }],
        }];
        return PickerState::new("Resume a session  esc to close", groups);
    }

    // Partition sessions by status group
    let mut active: Vec<&crate::repl::SessionInfo> = Vec::new();
    let mut completed: Vec<&crate::repl::SessionInfo> = Vec::new();
    let mut failed: Vec<&crate::repl::SessionInfo> = Vec::new();

    for s in sessions.iter().rev() {
        match s.status {
            crate::repl::SessionStatus::Running | crate::repl::SessionStatus::Interrupted => {
                active.push(s)
            }
            crate::repl::SessionStatus::Completed => completed.push(s),
            crate::repl::SessionStatus::Failed => failed.push(s),
        }
    }

    let make_item = |s: &crate::repl::SessionInfo| -> PickerItem {
        let status_icon = match s.status {
            crate::repl::SessionStatus::Running => "● ",
            crate::repl::SessionStatus::Interrupted => "✗ ",
            crate::repl::SessionStatus::Completed => "✓ ",
            crate::repl::SessionStatus::Failed => "✗ ",
        };
        let idea = if s.idea.len() > 55 {
            format!("{}…", &s.idea[..55])
        } else {
            s.idea.clone()
        };
        let time_str = s.timestamp.format("%Y-%m-%d %H:%M").to_string();
        let dir = s.directory.display().to_string();
        PickerItem {
            id: s.id.clone(),
            label: format!("{}{} · {}", status_icon, s.workflow, idea),
            description: Some(format!("{} — {}", time_str, dir)),
            checked: false,
        }
    };

    let mut groups: Vec<PickerGroup> = Vec::new();

    if !active.is_empty() {
        groups.push(PickerGroup {
            title: "Active / Interrupted".to_string(),
            items: active.iter().map(|s| make_item(s)).collect(),
        });
    }
    if !completed.is_empty() {
        groups.push(PickerGroup {
            title: "Completed".to_string(),
            items: completed.iter().map(|s| make_item(s)).collect(),
        });
    }
    if !failed.is_empty() {
        groups.push(PickerGroup {
            title: "Failed".to_string(),
            items: failed.iter().map(|s| make_item(s)).collect(),
        });
    }

    PickerState::new("Resume a session  esc to close", groups)
}

#[cfg(test)]
mod tests {
    use super::*;

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
