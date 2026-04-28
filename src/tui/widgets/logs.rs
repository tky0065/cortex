use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    layout::Rect,
    Frame,
};

/// A single log entry with an optional agent tag.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub agent: Option<String>,
    pub message: String,
    pub is_error: bool,
}

impl LogEntry {
    pub fn system(message: impl Into<String>) -> Self {
        Self {
            timestamp: current_time(),
            agent: None,
            message: message.into(),
            is_error: false,
        }
    }

    pub fn agent(agent: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            timestamp: current_time(),
            agent: Some(agent.into()),
            message: message.into(),
            is_error: false,
        }
    }

    pub fn error(agent: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            timestamp: current_time(),
            agent: Some(agent.into()),
            message: message.into(),
            is_error: true,
        }
    }
}

/// Scrollable, timestamped log panel.
pub struct LogsWidget<'a> {
    pub entries: &'a [LogEntry],
    /// Optional agent name to filter — None shows all entries
    pub filter: Option<&'a str>,
}

impl<'a> LogsWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(" Logs ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let inner_height = area.height.saturating_sub(2) as usize;
        let inner_width = area.width.saturating_sub(2) as usize;

        let filtered: Vec<&LogEntry> = self
            .entries
            .iter()
            .filter(|e| self.filter.is_none_or(|f| e.agent.as_deref() == Some(f)))
            .collect();

        // Select the most-recent entries that fit within inner_height visual rows,
        // accounting for line-wrapping of long messages.
        let mut rows_used: usize = 0;
        let visible: Vec<&LogEntry> = filtered
            .into_iter()
            .rev()
            .take_while(|e| {
                let n = visual_rows(e, inner_width);
                if rows_used + n <= inner_height {
                    rows_used += n;
                    true
                } else {
                    false
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();

        let lines: Vec<Line> = visible.iter().map(|e| format_entry(e)).collect();

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}

/// Estimate how many terminal rows an entry will occupy when wrapped.
fn visual_rows(entry: &LogEntry, panel_width: usize) -> usize {
    if panel_width == 0 {
        return 1;
    }
    // prefix: "HH:MM:SS " (9) + "[agent] " (agent.len + 3) or nothing
    let prefix_len = 9 + entry.agent.as_ref().map(|a| a.len() + 3).unwrap_or(0);
    let available = panel_width.saturating_sub(prefix_len);
    if available == 0 {
        return 1;
    }
    entry.message.len().div_ceil(available).max(1)
}

fn format_entry(entry: &LogEntry) -> Line<'static> {
    let ts = Span::styled(
        format!("{} ", entry.timestamp),
        Style::default().fg(Color::DarkGray),
    );

    match &entry.agent {
        None => {
            let msg = Span::styled(
                entry.message.clone(),
                Style::default().fg(Color::White),
            );
            Line::from(vec![ts, msg])
        }
        Some(agent) => {
            let (tag_color, msg_color) = if entry.is_error {
                (Color::Red, Color::Red)
            } else {
                (Color::Cyan, Color::White)
            };
            let tag = Span::styled(
                format!("[{}] ", agent),
                Style::default().fg(tag_color),
            );
            // Error marker
            let prefix = if entry.is_error {
                Span::styled("✗ ".to_string(), Style::default().fg(Color::Red))
            } else {
                Span::raw("")
            };
            let msg = Span::styled(
                entry.message.clone(),
                Style::default().fg(msg_color),
            );
            Line::from(vec![ts, tag, prefix, msg])
        }
    }
}

fn current_time() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    #[test]
    fn renders_empty_logs() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                LogsWidget { entries: &[], filter: None }.render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_with_entries() {
        let mut terminal = make_terminal();
        let entries = vec![
            LogEntry {
                timestamp: "12:00:00".to_string(),
                agent: None,
                message: "Workflow started".to_string(),
                is_error: false,
            },
            LogEntry {
                timestamp: "12:00:01".to_string(),
                agent: Some("CEO".to_string()),
                message: "Analyzing the idea".to_string(),
                is_error: false,
            },
            LogEntry {
                timestamp: "12:00:02".to_string(),
                agent: Some("PM".to_string()),
                message: "Writing specs.md".to_string(),
                is_error: false,
            },
        ];
        terminal
            .draw(|f| {
                let area = f.area();
                LogsWidget { entries: &entries, filter: None }.render(f, area);
            })
            .unwrap();
    }
}
