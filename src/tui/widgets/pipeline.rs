use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
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
        Self { name: name.to_string(), status: AgentStatus::Idle }
    }
}

/// Renders the pipeline status bar showing each agent's status with a symbol.
///
/// Symbols: ✓ done · ● running · ◌ idle · ✗ error
pub struct PipelineWidget<'a> {
    pub agents: &'a [AgentState],
}

impl<'a> PipelineWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = vec![Span::raw("  ")];

        for (i, agent) in self.agents.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("   │   ", Style::default().fg(Color::DarkGray)));
            }
            let (symbol, style) = match agent.status {
                AgentStatus::Idle => (
                    "◌",
                    Style::default().fg(Color::DarkGray),
                ),
                AgentStatus::Running => (
                    "●",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                AgentStatus::Done => (
                    "✓",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                AgentStatus::Error => (
                    "✗",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
            };
            spans.push(Span::styled(format!("{} {}", symbol, agent.name), style));
        }

        let block = Block::default()
            .title(" Pipeline ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        frame.render_widget(
            Paragraph::new(Line::from(spans)).block(block),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    #[test]
    fn renders_empty_pipeline() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                PipelineWidget { agents: &[] }.render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_mixed_statuses() {
        let mut terminal = make_terminal();
        let agents = vec![
            AgentState { name: "CEO".to_string(), status: AgentStatus::Done },
            AgentState { name: "PM".to_string(), status: AgentStatus::Running },
            AgentState { name: "TechLead".to_string(), status: AgentStatus::Idle },
            AgentState { name: "Developer".to_string(), status: AgentStatus::Error },
        ];
        terminal
            .draw(|f| {
                let area = f.area();
                PipelineWidget { agents: &agents }.render(f, area);
            })
            .unwrap();
    }
}
