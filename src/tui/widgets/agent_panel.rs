use crate::tui::theme::THEME;
use crate::tui::widgets::diff_viewer::{DiffLine, FileDiff};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
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
    /// Vertical scroll offset for the content area
    pub scroll_offset: usize,
    /// Inline file diffs accumulated during this agent's run.
    pub diffs: Vec<FileDiff>,
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
            scroll_offset: 0,
            diffs: Vec::new(),
        }
    }

    pub fn push_diff(&mut self, diff: FileDiff) {
        self.diffs.push(diff);
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
        self.scroll_offset = 0;
        self.diffs.clear();
    }

    /// Like `restart()` but keeps previous content with a visual separator so
    /// the chat history stays visible across prompts (used for the "cortex" assistant).
    pub fn new_turn(&mut self) {
        self.status = AgentRunStatus::Running;
        self.current_action = "Thinking…".to_string();
        self.summary.clear();
        self.progress = 0;
        self.scroll_offset = 0;
        if !self.stream_buffer.is_empty() {
            self.stream_buffer.push_str("\n\n---\n\n");
        }
    }

    pub fn set_summary(&mut self, summary: &str) {
        self.summary = summary.to_owned();
        if self.progress < 95 {
            self.progress = 95;
        }
    }

    pub fn push_chunk(&mut self, chunk: &str) {
        // Clear the heartbeat "Waiting for agent response..." status on first token
        if self
            .current_action
            .starts_with("Waiting for agent response")
        {
            self.current_action = "Generating...".to_string();
        }
        self.stream_buffer.push_str(chunk);
        // Keep only the last 50 000 chars to prevent unbounded buffer growth.
        const MAX_BUFFER: usize = 50_000;
        if self.stream_buffer.len() > MAX_BUFFER {
            let excess = self.stream_buffer.len() - MAX_BUFFER;
            let cut = self.stream_buffer[excess..]
                .find('\n')
                .map(|i| excess + i + 1)
                .unwrap_or(excess);
            self.stream_buffer = self.stream_buffer[cut..].to_string();
        }
        // Advance progress by a small amount per chunk (cap at 95 — finish() sets 100)
        if self.progress < 95 {
            self.progress = (self.progress + 1).min(95);
        }
    }

    pub fn finish(&mut self) {
        self.progress = 100;
        self.status = AgentRunStatus::Done;
        if self.current_action == "Starting..."
            || self
                .current_action
                .starts_with("Waiting for agent response")
        {
            self.current_action = "Completed".to_string();
        }
    }

    pub fn fail(&mut self, message: &str) {
        self.status = AgentRunStatus::Error;
        self.current_action = message.to_owned();
    }

    /// Replace stream_buffer with a clean final reply (used to fix content duplication).
    pub fn replace_buffer(&mut self, content: &str) {
        self.stream_buffer = content.to_owned();
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
            .border_type(BorderType::Rounded)
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
        // Select up to 6 agents to display:
        // 1. Running or Error agents.
        // 2. Most recently added Done agents.
        // 3. Re-sort by original index to keep positions stable.
        let mut enumerated_agents: Vec<(usize, &ActiveAgent)> =
            self.agents.iter().enumerate().collect();

        // Sort: Running/Error first, then by index descending (newest first) for Done agents
        enumerated_agents.sort_by(|(idx_a, a), (idx_b, b)| {
            let a_active = a.status != AgentRunStatus::Done;
            let b_active = b.status != AgentRunStatus::Done;
            if a_active && !b_active {
                std::cmp::Ordering::Less
            } else if !a_active && b_active {
                std::cmp::Ordering::Greater
            } else {
                // If both are same status type (both active or both done), newest first to pick the most recent ones if we have > 6
                idx_b.cmp(idx_a)
            }
        });

        // Take max 6
        enumerated_agents.truncate(6);

        // Re-sort by original index so they appear in creation order on screen
        enumerated_agents.sort_by_key(|(idx, _)| *idx);

        let visible_agents: Vec<&ActiveAgent> =
            enumerated_agents.into_iter().map(|(_, a)| a).collect();

        // Divide inner area into a grid based on active agents (max 6 visible)
        let count = visible_agents.len();

        // Responsive grid: adjust columns based on available width to keep panels readable.
        // min_col_width = 35 chars is a reasonable floor for readable agent output.
        let min_col_width = 35;
        let available_width = inner.width as usize;
        let max_cols = (available_width / min_col_width).max(1);

        let desired_cols = match count {
            1 => 1,
            2 => 2,
            3 | 4 => 2,
            _ => 3,
        };

        let cols = desired_cols.min(max_cols);
        let rows = count.div_ceil(cols);

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
                    render_agent_block(frame, visible_agents[index], col_rects[c], self.tick_count);
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

    // Status line shown at the bottom only while Running or Error.
    // Done agents give the full area to content.
    let show_status = agent.status != AgentRunStatus::Done;
    let (content_area, status_area_opt) = if show_status && inner.height >= 3 {
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        (split[0], Some(split[1]))
    } else {
        (inner, None)
    };

    // ── Status line (bottom) ─────────────────────────────────────────────────
    if let Some(status_area) = status_area_opt {
        let spinner_frames = ["◐", "◓", "◑", "◒"];
        let (status_label, status_color) = match agent.status {
            AgentRunStatus::Running => {
                let frame_ch = spinner_frames[(tick_count % spinner_frames.len() as u64) as usize];
                (format!("{} running", frame_ch), THEME.primary)
            }
            AgentRunStatus::Error => ("✕ error".to_string(), THEME.error),
            AgentRunStatus::Done => unreachable!(),
        };

        let progress_bar = if agent.progress > 0 && agent.progress < 100 {
            let width = 10;
            let filled = (agent.progress as usize * width) / 100;
            let mut bar = String::from("[");
            for i in 0..width {
                bar.push(if i < filled { '█' } else { '░' });
            }
            bar.push_str("] ");
            bar.push_str(&format!("{:>2}% ", agent.progress));
            bar
        } else {
            String::new()
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
        frame.render_widget(Paragraph::new(vec![status_line]), status_area);
    }

    // ── Stream content + inline diffs ────────────────────────────────────────
    let available_lines = content_area.height as usize;
    let panel_width = content_area.width as usize;

    // Build all content lines: diffs first (actions), then stream buffer (agent reply).
    // With bottom-anchored scroll this means: scroll_offset=0 shows the agent's final
    // message at the bottom, and scrolling up reveals the file diffs above it.
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    for diff in &agent.diffs {
        all_lines.extend(render_diff_inline(diff, panel_width));
        all_lines.push(Line::from(""));
    }

    if !agent.stream_buffer.is_empty() {
        let clean_buffer = crate::assistant::strip_tool_calls_for_display(&agent.stream_buffer);
        let content_lines = build_content_lines(&clean_buffer, panel_width.max(20));
        if agent.status == AgentRunStatus::Running {
            // Gradient: dim older lines, full brightness on newest
            let total = content_lines.len();
            for (i, line) in content_lines.into_iter().enumerate() {
                if i + 4 >= total {
                    all_lines.push(line);
                } else {
                    let dimmed: Vec<Span<'static>> = line
                        .spans
                        .into_iter()
                        .map(|s| Span::styled(s.content, s.style.fg(Color::Rgb(130, 140, 150))))
                        .collect();
                    all_lines.push(Line::from(dimmed));
                }
            }
        } else {
            all_lines.extend(content_lines);
        }
    }

    if all_lines.is_empty() {
        frame.render_widget(Paragraph::new(vec![Line::from("")]), content_area);
    } else {
        // Always bottom-anchored: most recent content (including diffs) is at the bottom.
        // scroll_offset counts lines scrolled UP from the bottom; Alt+↑/↓ adjusts it.
        let total = all_lines.len();
        let base_start = total.saturating_sub(available_lines);
        let start = base_start.saturating_sub(agent.scroll_offset);
        let end = (start + available_lines).min(total);
        let visible = all_lines[start..end].to_vec();
        frame.render_widget(Paragraph::new(visible), content_area);
    }
}

/// Render a `FileDiff` as compact inline lines for display inside an agent panel.
///
/// Shows only context lines within 2 lines of a change; collapses the rest with `···`.
fn render_diff_inline(diff: &FileDiff, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let verb = if diff.is_new_file {
        "New file"
    } else {
        "Update"
    };
    lines.push(Line::from(vec![
        Span::styled(
            "● ",
            Style::default()
                .fg(THEME.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{}({})", verb, diff.path),
            Style::default()
                .fg(THEME.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    let summary = if diff.is_new_file {
        format!("  └ Added {} lines", diff.added_count)
    } else if diff.added_count > 0 && diff.removed_count > 0 {
        format!(
            "  └ Added {}, removed {}",
            diff.added_count, diff.removed_count
        )
    } else if diff.added_count > 0 {
        format!("  └ Added {} lines", diff.added_count)
    } else {
        format!("  └ Removed {} lines", diff.removed_count)
    };
    lines.push(Line::from(Span::styled(
        summary,
        Style::default().fg(THEME.muted),
    )));

    if diff.too_large {
        lines.push(Line::from(Span::styled(
            "  (fichier trop grand — diff non calculé)",
            Style::default().fg(THEME.muted),
        )));
        return lines;
    }

    if diff.lines.is_empty() {
        return lines;
    }

    // Group diff.lines into segments; collapse runs of > HUNK_MAX same-direction changes.
    const HUNK_MAX: usize = 8; // show at most this many consecutive +/- lines before collapsing
    const HUNK_HEAD: usize = 3; // lines to show at start of a collapsed hunk
    const HUNK_TAIL: usize = 2; // lines to show at end of a collapsed hunk
    const CTX_RADIUS: usize = 2; // context lines to show around changes

    let changed_indices: Vec<usize> = diff
        .lines
        .iter()
        .enumerate()
        .filter(|(_, dl)| !matches!(dl, DiffLine::Context { .. }))
        .map(|(i, _)| i)
        .collect();

    let near_change = |idx: usize| -> bool {
        changed_indices
            .iter()
            .any(|&ci| idx >= ci.saturating_sub(CTX_RADIUS) && idx <= ci + CTX_RADIUS)
    };

    // Identify contiguous changed runs so we can collapse large ones.
    // A run is a maximal sequence of consecutive Added/Removed indices.
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (start_idx, end_idx) inclusive
    if !changed_indices.is_empty() {
        let mut run_start = changed_indices[0];
        let mut run_end = changed_indices[0];
        for &ci in &changed_indices[1..] {
            if ci == run_end + 1 {
                run_end = ci;
            } else {
                runs.push((run_start, run_end));
                run_start = ci;
                run_end = ci;
            }
        }
        runs.push((run_start, run_end));
    }

    // Build a set of indices that should be collapsed (hidden) due to large runs
    let mut collapsed_ranges: Vec<(usize, usize, usize)> = Vec::new(); // (first_hidden, last_hidden, count)
    for &(rs, re) in &runs {
        let run_len = re - rs + 1;
        if run_len > HUNK_MAX {
            let first_hidden = rs + HUNK_HEAD;
            let last_hidden = re - HUNK_TAIL;
            if first_hidden <= last_hidden {
                collapsed_ranges.push((first_hidden, last_hidden, run_len - HUNK_HEAD - HUNK_TAIL));
            }
        }
    }

    let is_hidden = |idx: usize| -> Option<usize> {
        for &(fh, lh, count) in &collapsed_ranges {
            if idx > fh && idx <= lh {
                return Some(0); // hidden, not the summary line
            }
            if idx == fh {
                return Some(count); // first hidden = show summary here
            }
        }
        None
    };

    let mut prev_ctx_collapsed = false;
    for (i, dl) in diff.lines.iter().enumerate() {
        match dl {
            DiffLine::Context { line_no, text } => {
                if near_change(i) {
                    prev_ctx_collapsed = false;
                    lines.push(Line::from(vec![
                        Span::styled(format!("{:>5} ", line_no), Style::default().fg(THEME.muted)),
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            truncate_str(text, width.saturating_sub(8)),
                            Style::default().fg(THEME.muted),
                        ),
                    ]));
                } else if !prev_ctx_collapsed {
                    prev_ctx_collapsed = true;
                    lines.push(Line::from(Span::styled(
                        "       ···",
                        Style::default().fg(THEME.muted),
                    )));
                }
            }
            DiffLine::Added { line_no, text } => {
                prev_ctx_collapsed = false;
                match is_hidden(i) {
                    Some(0) => continue, // hidden
                    Some(count) => {
                        // Summary line for this collapsed run
                        lines.push(Line::from(Span::styled(
                            format!("       ··· +{} lines ···", count),
                            Style::default().fg(THEME.muted),
                        )));
                        continue;
                    }
                    None => {}
                }
                lines.push(Line::from(vec![
                    Span::styled(format!("{:>5} ", line_no), Style::default().fg(THEME.muted)),
                    Span::styled(
                        "+ ",
                        Style::default()
                            .fg(THEME.success)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        truncate_str(text, width.saturating_sub(8)),
                        Style::default().fg(THEME.success),
                    ),
                ]));
            }
            DiffLine::Removed { line_no, text } => {
                prev_ctx_collapsed = false;
                match is_hidden(i) {
                    Some(0) => continue,
                    Some(count) => {
                        lines.push(Line::from(Span::styled(
                            format!("       ··· -{} lines ···", count),
                            Style::default().fg(THEME.muted),
                        )));
                        continue;
                    }
                    None => {}
                }
                lines.push(Line::from(vec![
                    Span::styled(format!("{:>5} ", line_no), Style::default().fg(THEME.muted)),
                    Span::styled(
                        "- ",
                        Style::default()
                            .fg(THEME.error)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        truncate_str(text, width.saturating_sub(8)),
                        Style::default().fg(THEME.error),
                    ),
                ]));
            }
        }
    }

    lines
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", t)
    }
}

/// Strip residual HTML tags and decode entities from a display string.
fn clean_html(s: &str) -> String {
    // Strip <tag> / </tag> patterns
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    crate::assistant::decode_basic_html_entities(&out)
}

/// Build display lines from stream buffer content.
///
/// Handles fenced code blocks (preserves indentation), word-wraps prose,
/// and applies basic markdown styling. Used for both streaming and Done states.
fn build_content_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;

    for raw in text.split('\n') {
        // Decode HTML entities and strip residual HTML tags
        let cleaned = clean_html(raw.trim_end());
        let line = cleaned.as_str();

        // Fenced code block delimiter
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            // Show the fence line dimmed
            lines.push(Line::from(Span::styled(
                line.to_string(),
                Style::default().fg(THEME.muted),
            )));
            continue;
        }

        if in_code_block {
            // Inside code block: preserve indentation, truncate only if truly too long
            lines.push(Line::from(Span::styled(
                truncate_str(line, width),
                Style::default().fg(Color::Rgb(180, 215, 180)),
            )));
            continue;
        }

        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // Prose: detect block-level markdown, then word-wrap
        if let Some(rest) = line.strip_prefix("## ") {
            for wl in wrap_prose(rest, width.saturating_sub(3)) {
                let spans = parse_inline_spans(
                    &wl,
                    Style::default()
                        .fg(THEME.primary)
                        .add_modifier(Modifier::BOLD),
                );
                lines.push(Line::from(spans));
            }
        } else if let Some(rest) = line.strip_prefix("# ") {
            for wl in wrap_prose(rest, width.saturating_sub(2)) {
                let spans = parse_inline_spans(
                    &wl,
                    Style::default()
                        .fg(THEME.primary)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                );
                lines.push(Line::from(spans));
            }
        } else if let Some(rest) = line.strip_prefix("- ") {
            let bullet_w = width.saturating_sub(2);
            let wrapped = wrap_prose(rest, bullet_w.max(1));
            for (idx, wl) in wrapped.into_iter().enumerate() {
                let mut spans: Vec<Span<'static>> = if idx == 0 {
                    vec![Span::styled("• ", Style::default().fg(THEME.primary))]
                } else {
                    vec![Span::raw("  ")]
                };
                spans.extend(parse_inline_spans(&wl, Style::default().fg(THEME.text)));
                lines.push(Line::from(spans));
            }
        } else {
            // Normal prose — word-wrap preserving indentation
            let leading = line.len() - line.trim_start().len();
            let indent = &line[..leading];
            let content = line.trim_start();
            let wrap_w = width.saturating_sub(leading).max(1);
            let wrapped = wrap_prose(content, wrap_w);
            for wl in wrapped {
                let full = format!("{}{}", indent, wl);
                let spans = parse_inline_spans(&full, Style::default().fg(THEME.text));
                lines.push(Line::from(spans));
            }
        }
    }

    lines
}

/// Word-wrap a plain prose string to `width` chars, splitting at whitespace.
/// Does NOT try to preserve leading indentation (handled by the caller).
fn wrap_prose(text: &str, width: usize) -> Vec<String> {
    if width == 0 || text.chars().count() <= width {
        return vec![text.to_string()];
    }
    let mut result = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
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
    if result.is_empty() {
        result.push(String::new());
    }
    result
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
#[cfg(test)]
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
#[cfg(test)]
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
    fn visible_agents_selection_prioritizes_active() {
        let mut agents = Vec::new();
        // 7 agents Done
        for i in 0..7 {
            let mut a = ActiveAgent::new(format!("Done {}", i));
            a.finish();
            agents.push(a);
        }
        // 1 agent Running (the 8th one)
        let mut running = ActiveAgent::new("Running");
        running.status = AgentRunStatus::Running;
        agents.push(running);

        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                let widget = AgentPanelWidget {
                    agents: &agents,
                    focused_agent: None,
                    tick_count: 0,
                };

                // We can't easily inspect the internal state of the render,
                // but we can check if it compiles and runs without panic.
                // In a real TUI test we might inspect the buffer.
                widget.render(f, area);
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
    fn finish_clears_waiting_heartbeat() {
        let mut agent = ActiveAgent::new("assistant");
        agent.set_progress("Waiting for agent response... (120s)");
        agent.finish();

        assert_eq!(agent.status, AgentRunStatus::Done);
        assert_eq!(agent.current_action, "Completed");
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
