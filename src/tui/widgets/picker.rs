/// Generic searchable picker popup widget.
///
/// Renders a centered popup with:
///   - a title and "esc" hint
///   - a live search bar
///   - grouped rows (section headers + items)
///   - the currently-active item highlighted in orange
///   - a checkmark (✓) beside items that are "checked"
///
/// Caller maintains `PickerState` and drives it with `PickerState::handle_*` helpers.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
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
                g.items.iter().filter(move |item| {
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
            .border_style(Style::default().fg(Color::DarkGray))
            .style(Style::default().bg(Color::Rgb(20, 20, 20)));
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
            Span::styled(
                format!(" {} ", self.state.title),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "esc",
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        frame.render_widget(Paragraph::new(title_line), chunks[0]);

        // Search bar
        let search_text = format!(" {}{}", self.state.search, "█");
        frame.render_widget(
            Paragraph::new(search_text)
                .style(Style::default().fg(Color::White).bg(Color::Rgb(35, 35, 35))),
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
                    format!(" {}", group_title),
                    Style::default()
                        .fg(Color::Rgb(150, 100, 255))
                        .add_modifier(Modifier::BOLD),
                ))));
                last_group = group_title;
            }

            let is_cursor = flat_idx == self.state.cursor;
            let check = if item.checked {
                Span::styled(" ✓ ", Style::default().fg(Color::Green))
            } else {
                Span::raw("   ")
            };

            let label_style = if is_cursor {
                Style::default()
                    .bg(Color::Rgb(180, 80, 20))
                    .fg(Color::White)
            } else {
                Style::default().fg(Color::White)
            };

            let mut spans = vec![check, Span::styled(item.label.clone(), label_style)];
            if let Some(desc) = &item.description {
                spans.push(Span::styled(
                    format!(" {}", desc),
                    if is_cursor {
                        Style::default()
                            .bg(Color::Rgb(180, 80, 20))
                            .fg(Color::Rgb(220, 180, 160))
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ));
            }
            rows.push(ListItem::new(Line::from(spans)));
        }

        // Scroll so cursor stays visible
        let mut list_state = ListState::default();
        // We need to map cursor (flat item index) to list row index (includes headers)
        let row_idx = cursor_to_row_idx(&filtered, self.state.cursor);
        list_state.select(Some(row_idx));

        let list = List::new(rows)
            .style(Style::default().bg(Color::Rgb(20, 20, 20)));

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
    let groups = vec![
        PickerGroup {
            title: "Roles — select to edit".to_string(),
            items,
        },
    ];
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
