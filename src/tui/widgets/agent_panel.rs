use crate::tui::theme::THEME;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
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
        // Clear the heartbeat "Waiting for model response..." status on first token
        if self
            .current_action
            .starts_with("Waiting for model response")
        {
            self.current_action = "Generating...".to_string();
        }
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
    pub focused_agent: Option<&'a str>,
    pub tick_count: u64,
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

        // ── Focused Mode ─────────────────────────────────────────────────────
        if let Some(target) = self.focused_agent {
            // Match if agent name equals target OR starts with target (e.g. "developer" matches "developer:src/main.rs")
            if let Some(agent) = self
                .agents
                .iter()
                .find(|a| a.name == target || a.name.starts_with(&format!("{}:", target)))
            {
                render_agent_block(frame, agent, inner, self.tick_count);
                return;
            }
        }

        // ── Grid Mode ────────────────────────────────────────────────────────
        // Divide inner area into a grid based on active agents (max 6 visible)
        let count = self.agents.len().min(6);
        let (rows, cols) = match count {
            1 => (1, 1),
            2 => (1, 2),
            3 | 4 => (2, 2),
            5 | 6 => (2, 3),
            _ => (2, 3),
        };

        let row_constraints: Vec<Constraint> = (0..rows)
            .map(|_| Constraint::Ratio(1, rows as u32))
            .collect();
        let col_constraints: Vec<Constraint> = (0..cols)
            .map(|_| Constraint::Ratio(1, cols as u32))
            .collect();

        let row_rects = Layout::default()
            .direction(Direction::Vertical)
            .constraints(row_constraints)
            .split(inner);

        for r in 0..rows {
            let col_rects = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(col_constraints.clone())
                .split(row_rects[r]);

            for c in 0..cols {
                let index = r * cols + c;
                if index < count {
                    render_agent_block(frame, &self.agents[index], col_rects[c], self.tick_count);
                }
            }
        }
    }
}

fn render_agent_block(frame: &mut Frame, agent: &ActiveAgent, area: Rect, tick_count: u64) {
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

    // Split: status line (1) | stream content (fill)
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // ── Status line ──────────────────────────────────────────────────────────
    let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let (status_label, status_color) = match agent.status {
        AgentRunStatus::Running => {
            let frame = spinner_frames[(tick_count % spinner_frames.len() as u64) as usize];
            (format!("{} RUN", frame), THEME.primary)
        }
        AgentRunStatus::Done => ("✓ DONE".to_string(), THEME.success),
        AgentRunStatus::Error => ("✗ ERR".to_string(), THEME.error),
    };

    // Build mini progress bar: [███░░] 60%
    let progress_bar = if agent.progress > 0 && agent.progress < 100 {
        let width = 10;
        let filled = (agent.progress as usize * width) / 100;
        let mut bar = String::from("[");
        for i in 0..width {
            if i < filled {
                bar.push('█');
            } else {
                bar.push('░');
            }
        }
        bar.push_str("] ");
        bar.push_str(&format!("{:>2}% ", agent.progress));
        bar
    } else {
        "".to_string()
    };

    let status_line = Line::from(vec![
        Span::styled(
            format!(" {} ", status_label),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(progress_bar, Style::default().fg(Color::Indexed(8))),
        Span::styled(
            agent.current_action.clone(),
            Style::default().fg(THEME.text),
        ),
    ]);
    frame.render_widget(Paragraph::new(vec![status_line]), split[0]);

    // ── Stream content ───────────────────────────────────────────────────────
    let content_area = split[1];
    let available_lines = content_area.height as usize;
    let panel_width = content_area.width as usize;

    if !agent.stream_buffer.is_empty() {
        if agent.status == AgentRunStatus::Done {
            // Done: render full content as formatted markdown, shown from top.
            let md_lines = render_markdown_lines(&agent.stream_buffer);
            frame.render_widget(
                Paragraph::new(md_lines).wrap(Wrap { trim: false }),
                content_area,
            );
        } else {
            // Streaming: word-wrap and show only the last `available_lines` rows
            // so the newest text stays visible at the bottom.
            let wrapped = word_wrap(&agent.stream_buffer, panel_width.max(1));
            let start = wrapped.len().saturating_sub(available_lines);
            let visible_lines: Vec<Line> = wrapped[start..]
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let is_last = i + start + 1 == wrapped.len();
                    let color = if is_last {
                        THEME.text
                    } else {
                        Color::Rgb(160, 160, 160)
                    };
                    Line::from(Span::styled(line.clone(), Style::default().fg(color)))
                })
                .collect();
            frame.render_widget(Paragraph::new(visible_lines), content_area);
        }
    } else {
        // Waiting for first token — show nothing (status line has the heartbeat message)
        frame.render_widget(Paragraph::new(vec![Line::from("")]), content_area);
    }
}

/// Convert a markdown string into styled ratatui `Line`s.
///
/// Supported syntax:
/// - `# Heading` / `## Heading`  → bold (+ primary colour)
/// - `- item`                     → `• item` bullet
/// - `**bold**`                   → BOLD modifier
/// - `*italic*`                   → ITALIC modifier
/// - `【…】`                       → citation markers dimmed
/// - plain text                   → default colour
fn render_markdown_lines(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for raw in text.split('\n') {
        let line = raw.trim_end();

        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // H2 heading
        if let Some(rest) = line.strip_prefix("## ") {
            let spans = parse_inline_spans(
                rest,
                Style::default()
                    .fg(THEME.primary)
                    .add_modifier(Modifier::BOLD),
            );
            lines.push(Line::from(spans));
            continue;
        }
        // H1 heading
        if let Some(rest) = line.strip_prefix("# ") {
            let spans = parse_inline_spans(
                rest,
                Style::default()
                    .fg(THEME.primary)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            );
            lines.push(Line::from(spans));
            continue;
        }
        // Bullet point (`- ` but not `**`)
        if let Some(rest) = line.strip_prefix("- ") {
            let mut spans = vec![Span::styled("• ", Style::default().fg(THEME.primary))];
            spans.extend(parse_inline_spans(rest, Style::default().fg(THEME.text)));
            lines.push(Line::from(spans));
            continue;
        }

        // Normal line — parse inline markers
        let spans = parse_inline_spans(line, Style::default().fg(THEME.text));
        lines.push(Line::from(spans));
    }

    lines
}

/// Parse inline markdown markers (`**bold**`, `*italic*`, `【citation】`) within a
/// single line of text, returning styled `Span`s.
fn parse_inline_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut buf = String::new();

    macro_rules! flush {
        () => {
            if !buf.is_empty() {
                spans.push(Span::styled(buf.clone(), base_style));
                buf.clear();
            }
        };
    }

    while i < n {
        // Bold: **...**
        if i + 1 < n && chars[i] == '*' && chars[i + 1] == '*' {
            let inner_start = i + 2;
            let mut j = inner_start;
            while j + 1 < n && !(chars[j] == '*' && chars[j + 1] == '*') {
                j += 1;
            }
            if j + 1 < n {
                flush!();
                let bold: String = chars[inner_start..j].iter().collect();
                spans.push(Span::styled(bold, base_style.add_modifier(Modifier::BOLD)));
                i = j + 2;
            } else {
                buf.push(chars[i]);
                i += 1;
            }
        }
        // Italic: *...* (only when not followed by another *)
        else if chars[i] == '*' && (i + 1 >= n || chars[i + 1] != '*') {
            let inner_start = i + 1;
            let mut j = inner_start;
            while j < n && chars[j] != '*' {
                j += 1;
            }
            if j < n {
                flush!();
                let italic: String = chars[inner_start..j].iter().collect();
                spans.push(Span::styled(
                    italic,
                    base_style.add_modifier(Modifier::ITALIC),
                ));
                i = j + 1;
            } else {
                buf.push(chars[i]);
                i += 1;
            }
        }
        // Citation markers 【…】 — render dimmed
        else if chars[i] == '【' {
            flush!();
            let mut j = i + 1;
            while j < n && chars[j] != '】' {
                j += 1;
            }
            let end = j.min(n - 1);
            let citation: String = chars[i..=end].iter().collect();
            spans.push(Span::styled(
                citation,
                Style::default().fg(Color::Rgb(80, 80, 80)),
            ));
            i = if j < n { j + 1 } else { n };
        } else {
            buf.push(chars[i]);
            i += 1;
        }
    }

    flush!();
    spans
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
                AgentPanelWidget {
                    agents: &[],
                    focused_agent: None,
                    tick_count: 0,
                }
                .render(f, area);
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
                AgentPanelWidget {
                    agents: &agents,
                    focused_agent: None,
                    tick_count: 0,
                }
                .render(f, area);
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
                AgentPanelWidget {
                    agents: &agents,
                    focused_agent: None,
                    tick_count: 0,
                }
                .render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_done_agent_with_markdown() {
        let mut terminal = make_terminal();
        let mut agent = ActiveAgent::new("assistant");
        agent.push_chunk(
            "## Summary\n\n**Key findings:**\n- Item one\n- Item two\n\n*Note:* plain text.",
        );
        agent.finish();
        let agents = vec![agent];
        terminal
            .draw(|f| {
                let area = f.area();
                AgentPanelWidget {
                    agents: &agents,
                    focused_agent: None,
                    tick_count: 0,
                }
                .render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn parse_inline_bold_and_italic() {
        let spans = parse_inline_spans("hello **world** and *there*", Style::default());
        // Should have: "hello ", "world" (bold), " and ", "there" (italic)
        assert!(spans.len() >= 4);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
        assert!(text.contains("there"));
    }

    #[test]
    fn render_markdown_headings_and_bullets() {
        let lines = render_markdown_lines("# Title\n## Sub\n- item one\n- item two\nPlain.");
        assert_eq!(lines.len(), 5);
        // Bullet lines start with the bullet span
        let bullet_line = &lines[2];
        assert!(
            bullet_line
                .spans
                .first()
                .map(|s| s.content.contains('•'))
                .unwrap_or(false)
        );
    }
}
