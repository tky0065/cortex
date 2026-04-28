use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRunStatus {
    Running,
    Done,
    Error,
}

/// Live state for a single active agent.
#[derive(Debug, Clone)]
pub struct ActiveAgent {
    pub name: String,
    /// What the agent is currently doing.
    pub current_action: String,
    /// Short human-readable result shown when the agent finishes.
    pub summary: String,
    /// Latest detailed log line.
    pub last_detail: String,
    pub status: AgentRunStatus,
    /// Progress 0–100 (advanced by token chunks)
    pub progress: u8,
}

impl ActiveAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            current_action: "Starting...".to_string(),
            summary: String::new(),
            last_detail: String::new(),
            status: AgentRunStatus::Running,
            progress: 0,
        }
    }

    pub fn set_progress(&mut self, message: &str) {
        self.status = AgentRunStatus::Running;
        self.current_action = message.to_owned();
        if self.progress < 95 {
            self.progress = (self.progress + 8).min(95);
        }
    }

    pub fn restart(&mut self) {
        self.status = AgentRunStatus::Running;
        self.current_action = "Starting...".to_string();
        self.summary.clear();
        self.last_detail.clear();
        self.progress = 0;
    }

    pub fn set_summary(&mut self, summary: &str) {
        self.summary = summary.to_owned();
        if self.progress < 95 {
            self.progress = 95;
        }
    }

    pub fn push_chunk(&mut self, chunk: &str) {
        self.last_detail = chunk.to_owned();
        // Advance progress by a small amount per chunk (cap at 95 — finish() sets 100)
        if self.progress < 95 {
            self.progress = (self.progress + 2).min(95);
        }
    }

    pub fn finish(&mut self) {
        self.progress = 100;
        self.status = AgentRunStatus::Done;
        if self.current_action == "Starting..." {
            self.current_action = "Completed".to_string();
        }
    }

    pub fn fail(&mut self, message: &str) {
        self.status = AgentRunStatus::Error;
        self.current_action = message.to_owned();
    }
}

/// Renders the agents panel — one block per active agent with a progress gauge.
pub struct AgentPanelWidget<'a> {
    pub agents: &'a [ActiveAgent],
}

impl<'a> AgentPanelWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let outer = Block::default()
            .title(" Agents ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        if self.agents.is_empty() {
            frame.render_widget(Paragraph::new("  No active agents").block(outer), area);
            return;
        }

        frame.render_widget(outer, area);
        let inner = area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        });

        // Divide inner area equally among active agents (max 6 visible)
        let count = self.agents.len().min(6);
        let constraints: Vec<Constraint> = (0..count)
            .map(|_| Constraint::Ratio(1, count as u32))
            .collect();
        let cells = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        for (i, agent) in self.agents.iter().take(count).enumerate() {
            render_agent_block(frame, agent, cells[i]);
        }
    }
}

fn render_agent_block(frame: &mut Frame, agent: &ActiveAgent, area: Rect) {
    if area.height < 2 {
        return;
    }

    let block = Block::default()
        .title(Span::styled(
            format!(" {} ", agent.name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    frame.render_widget(block, area);
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 1,
    });

    if inner.height < 2 {
        return;
    }

    // Split: last line (fill) | gauge (1)
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let (status_label, status_color) = match agent.status {
        AgentRunStatus::Running => ("En cours", Color::Yellow),
        AgentRunStatus::Done => ("Termine", Color::Green),
        AgentRunStatus::Error => ("Erreur", Color::Red),
    };
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("  {} ", status_label),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            agent.current_action.clone(),
            Style::default().fg(Color::White),
        ),
    ])];

    if agent.status == AgentRunStatus::Done && !agent.summary.is_empty() {
        for line in agent.summary.lines().take(3) {
            lines.push(Line::from(Span::styled(
                format!("  {}", line),
                Style::default().fg(Color::Gray),
            )));
        }
    } else if !agent.last_detail.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", agent.last_detail),
            Style::default().fg(Color::DarkGray),
        )));
    }

    frame.render_widget(Paragraph::new(lines), split[0]);

    // Progress gauge
    let label = if agent.progress >= 100 {
        "done".to_string()
    } else {
        format!("{}%", agent.progress)
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .percent(agent.progress as u16)
        .label(label);
    frame.render_widget(gauge, split[1]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    #[test]
    fn renders_empty_panel() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                AgentPanelWidget { agents: &[] }.render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_multiple_agents() {
        let mut terminal = make_terminal();
        let agents = vec![
            {
                let mut agent = ActiveAgent::new("CEO");
                agent.set_progress("Analyzing idea...");
                agent
            },
            ActiveAgent::new("PM"),
            {
                let mut agent = ActiveAgent::new("Developer");
                agent.set_summary("Created source files");
                agent.finish();
                agent
            },
        ];
        terminal
            .draw(|f| {
                let area = f.area();
                AgentPanelWidget { agents: &agents }.render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn progress_and_summary_survive_done() {
        let mut agent = ActiveAgent::new("pm");
        agent.set_progress("Redaction de specs.md");
        agent.set_summary("Specs completes\nRisques identifies");
        agent.finish();

        assert_eq!(agent.status, AgentRunStatus::Done);
        assert_eq!(agent.current_action, "Redaction de specs.md");
        assert!(agent.summary.contains("Specs completes"));
        assert_eq!(agent.progress, 100);
    }
}
