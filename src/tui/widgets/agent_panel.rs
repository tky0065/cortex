use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, BorderType, Gauge, Paragraph, Wrap},
};
use crate::tui::theme::THEME;

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
    /// What the agent is currently doing (status line).
    pub current_action: String,
    /// Short human-readable result shown when the agent finishes.
    pub summary: String,
    /// Accumulated token stream — raw text as it arrives from the LLM.
    pub stream_buffer: String,
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
            stream_buffer: String::new(),
            status: AgentRunStatus::Running,
            progress: 0,
        }
    }

    pub fn set_progress(&mut self, message: &str) {
        self.status = AgentRunStatus::Running;
        self.current_action = message.to_owned();
    }

    pub fn restart(&mut self) {
        self.status = AgentRunStatus::Running;
        self.current_action = "Starting...".to_string();
        self.summary.clear();
        self.stream_buffer.clear();
        self.progress = 0;
    }

    pub fn set_summary(&mut self, summary: &str) {
        self.summary = summary.to_owned();
        if self.progress < 95 {
            self.progress = 95;
        }
    }

    pub fn push_chunk(&mut self, chunk: &str) {
        self.stream_buffer.push_str(chunk);
        // Advance progress by a small amount per chunk (cap at 95 — finish() sets 100)
        if self.progress < 95 {
            self.progress = (self.progress + 1).min(95);
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
            .title(Span::styled(" Agents ", THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.border_style());

        if self.agents.is_empty() {
            frame.render_widget(Paragraph::new("  No active agents").block(outer), area);
            return;
        }

        frame.render_widget(outer, area);
        let inner = area.inner(Margin {
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
            THEME.title_style(),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(THEME.border_style());

    frame.render_widget(block, area);
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });

    if inner.height < 2 {
        return;
    }

    // Split: status line (1) | stream content (fill) | gauge (1)
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    // ── Status line ──────────────────────────────────────────────────────────
    let (status_label, status_color) = match agent.status {
        AgentRunStatus::Running => ("⚡ RUN", THEME.primary),
        AgentRunStatus::Done => ("✓ DONE", THEME.success),
        AgentRunStatus::Error => ("✗ ERR", THEME.error),
    };
    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} ", status_label),
            Style::default().fg(status_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(agent.current_action.clone(), Style::default().fg(THEME.text)),
    ]);
    frame.render_widget(Paragraph::new(vec![status_line]), split[0]);

    // ── Stream content ───────────────────────────────────────────────────────
    let content_area = split[1];
    let available_lines = content_area.height as usize;
    let panel_width = content_area.width as usize;

    if agent.status == AgentRunStatus::Done && !agent.summary.is_empty() {
        // After completion: show the summary
        let lines: Vec<Line> = agent
            .summary
            .lines()
            .map(|l| Line::from(Span::styled(l.to_string(), Style::default().fg(THEME.muted))))
            .collect();
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            content_area,
        );
    } else if !agent.stream_buffer.is_empty() {
        // While streaming or done without summary: show the live token stream.
        // Word-wrap and take only the last `available_lines` visual rows.
        let text = &agent.stream_buffer;
        let wrapped = word_wrap(text, panel_width.max(1));

        // Take last available_lines rows so newest text stays at the bottom
        let start = wrapped.len().saturating_sub(available_lines);
        let visible_lines: Vec<Line> = wrapped[start..]
            .iter()
            .enumerate()
            .map(|(i, line)| {
                // Fade older lines slightly
                let is_last = i + start + 1 == wrapped.len();
                let color = if is_last { THEME.text } else { Color::Rgb(160, 160, 160) };
                Line::from(Span::styled(line.clone(), Style::default().fg(color)))
            })
            .collect();
        frame.render_widget(Paragraph::new(visible_lines), content_area);
    } else {
        // Waiting for first token — show nothing (status line has the heartbeat message)
        frame.render_widget(Paragraph::new(vec![Line::from("")]), content_area);
    }

    // ── Progress gauge ───────────────────────────────────────────────────────
    let label = if agent.progress >= 100 {
        "COMPLETE".to_string()
    } else {
        format!("{}%", agent.progress)
    };
    let gauge_color = match agent.status {
        AgentRunStatus::Running => THEME.primary,
        AgentRunStatus::Done => THEME.success,
        AgentRunStatus::Error => THEME.error,
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(gauge_color).bg(Color::Rgb(30, 41, 59)))
        .percent(agent.progress as u16)
        .label(Span::styled(label, Style::default().add_modifier(Modifier::BOLD)));
    frame.render_widget(gauge, split[2]);
}

/// Simple word-wrapping: splits `text` into lines of at most `width` chars,
/// breaking at whitespace boundaries where possible.
fn word_wrap(text: &str, width: usize) -> Vec<String> {
    let mut result = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                line.push_str(word);
            } else if line.len() + 1 + word.len() <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                result.push(line.clone());
                line = word.to_string();
            }
        }
        if !line.is_empty() {
            result.push(line);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
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

    #[test]
    fn stream_buffer_accumulates_chunks() {
        let mut agent = ActiveAgent::new("ceo");
        agent.push_chunk("Hello ");
        agent.push_chunk("world");
        assert_eq!(agent.stream_buffer, "Hello world");
        assert!(agent.progress > 0);
    }

    #[test]
    fn word_wrap_basic() {
        let lines = word_wrap("hello world foo bar", 10);
        assert!(lines.iter().all(|l| l.len() <= 10));
        let joined = lines.join(" ");
        assert!(joined.contains("hello"));
        assert!(joined.contains("world"));
    }

    #[test]
    fn renders_streaming_agent() {
        let mut terminal = make_terminal();
        let mut agent = ActiveAgent::new("ceo");
        agent.push_chunk("Analyzing the business idea and defining the MVP scope...");
        let agents = vec![agent];
        terminal
            .draw(|f| {
                let area = f.area();
                AgentPanelWidget { agents: &agents }.render(f, area);
            })
            .unwrap();
    }
}
