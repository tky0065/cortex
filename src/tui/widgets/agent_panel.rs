use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};

/// Live state for a single active agent.
#[derive(Debug, Clone)]
pub struct ActiveAgent {
    pub name: String,
    /// Latest streamed line from the LLM
    pub last_line: String,
    /// Progress 0–100 (advanced by token chunks)
    pub progress: u8,
}

impl ActiveAgent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            last_line: String::new(),
            progress: 0,
        }
    }

    pub fn push_chunk(&mut self, chunk: &str) {
        self.last_line = chunk.to_owned();
        // Advance progress by a small amount per chunk (cap at 95 — finish() sets 100)
        if self.progress < 95 {
            self.progress = (self.progress + 2).min(95);
        }
    }

    pub fn finish(&mut self) {
        self.progress = 100;
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
            frame.render_widget(
                Paragraph::new("  No active agents").block(outer),
                area,
            );
            return;
        }

        frame.render_widget(outer, area);
        let inner = area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });

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
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    frame.render_widget(block, area);
    let inner = area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });

    if inner.height < 2 {
        return;
    }

    // Split: last line (fill) | gauge (1)
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    // Last streamed line
    let preview = if agent.last_line.is_empty() {
        Line::from(Span::styled("  working…", Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(Span::styled(
            format!("  {}", &agent.last_line),
            Style::default().fg(Color::White),
        ))
    };
    frame.render_widget(Paragraph::new(preview), split[0]);

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
    use ratatui::{backend::TestBackend, Terminal};

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
            ActiveAgent { name: "CEO".to_string(), last_line: "Analyzing idea...".to_string(), progress: 25 },
            ActiveAgent { name: "PM".to_string(), last_line: String::new(), progress: 0 },
            ActiveAgent { name: "Developer".to_string(), last_line: "Writing code".to_string(), progress: 100 },
        ];
        terminal
            .draw(|f| {
                let area = f.area();
                AgentPanelWidget { agents: &agents }.render(f, area);
            })
            .unwrap();
    }
}
