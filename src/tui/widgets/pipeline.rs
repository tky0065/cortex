use crate::tui::theme::THEME;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Idle,
    Running,
    Done,
    Error,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub name: String,
    pub status: AgentStatus,
}

impl AgentState {
    pub fn idle(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: AgentStatus::Idle,
        }
    }
}

/// Renders the pipeline status bar showing each agent's status with a symbol.
///
/// Symbols: ✓ done · ● running · ◌ idle · ✗ error
pub struct PipelineWidget<'a> {
    pub agents: &'a [AgentState],
    /// When set, shows a "ALL COMPLETE" line with the given duration in seconds.
    pub complete_duration_secs: Option<u64>,
}

impl<'a> PipelineWidget<'a> {
    fn duration_str(secs: u64) -> String {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        if h > 0 {
            format!("{}h {}m {}s", h, m, s)
        } else if m > 0 {
            format!("{}m {}s", m, s)
        } else {
            format!("{}s", s)
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = vec![Span::raw("  ")];

        for (i, agent) in self.agents.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  →  ", Style::default().fg(THEME.muted)));
            }
            let (symbol, style) = match agent.status {
                AgentStatus::Idle => ("◇", Style::default().fg(THEME.muted)),
                AgentStatus::Running => (
                    "◈",
                    Style::default()
                        .fg(THEME.warning)
                        .add_modifier(Modifier::BOLD),
                ),
                AgentStatus::Done => (
                    "◆",
                    Style::default()
                        .fg(THEME.success)
                        .add_modifier(Modifier::BOLD),
                ),
                AgentStatus::Error => (
                    "✗",
                    Style::default()
                        .fg(THEME.error)
                        .add_modifier(Modifier::BOLD),
                ),
            };

            let color = match agent.status {
                AgentStatus::Idle => THEME.muted,
                AgentStatus::Running => THEME.primary,
                AgentStatus::Done => THEME.success,
                AgentStatus::Error => THEME.error,
            };

            spans.push(Span::styled(format!("{} ", symbol), style));
            spans.push(Span::styled(
                agent.name.to_uppercase(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }

        let block = Block::default()
            .title(Span::styled(" Pipeline ", THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.border_style());

        let mut lines = vec![Line::from(spans)];
        if let Some(secs) = self.complete_duration_secs {
            lines.push(Line::from(Span::styled(
                format!(
                    "  ✓ ALL COMPLETE — {}",
                    Self::duration_str(secs)
                ),
                Style::default()
                    .fg(THEME.success)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    #[test]
    fn renders_empty_pipeline() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                PipelineWidget {
                    agents: &[],
                    complete_duration_secs: None,
                }
                .render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_mixed_statuses() {
        let mut terminal = make_terminal();
        let agents = vec![
            AgentState {
                name: "CEO".to_string(),
                status: AgentStatus::Done,
            },
            AgentState {
                name: "PM".to_string(),
                status: AgentStatus::Running,
            },
            AgentState {
                name: "TechLead".to_string(),
                status: AgentStatus::Idle,
            },
            AgentState {
                name: "Developer".to_string(),
                status: AgentStatus::Error,
            },
        ];
        terminal
            .draw(|f| {
                let area = f.area();
                PipelineWidget {
                    agents: &agents,
                    complete_duration_secs: None,
                }
                .render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_complete_line() {
        let mut terminal = make_terminal();
        let agents = vec![
            AgentState {
                name: "CEO".to_string(),
                status: AgentStatus::Done,
            },
        ];
        terminal
            .draw(|f| {
                let area = f.area();
                PipelineWidget {
                    agents: &agents,
                    complete_duration_secs: Some(125),
                }
                .render(f, area);
            })
            .unwrap();
    }
}
