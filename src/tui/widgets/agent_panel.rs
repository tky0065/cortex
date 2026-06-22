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

#[derive(Debug, Clone, Default)]
pub struct ConversationTurn {
    pub prompt: String,
    pub response: String,
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
    /// Absolute first visible line while manual scrolling is active.
    pub scroll_offset: usize,
    /// Whether the panel follows newly appended content at the bottom.
    pub follow_tail: bool,
    /// Structured chat transcript used by the Cortex assistant.
    pub turns: Vec<ConversationTurn>,
    /// Inline file diffs accumulated during this agent's run.
    pub diffs: Vec<FileDiff>,
    /// Tool calls emitted during this agent's run — rendered as structured labeled blocks.
    pub tool_calls: Vec<(String, String)>, // (tool, label)
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
            follow_tail: true,
            turns: Vec::new(),
            diffs: Vec::new(),
            tool_calls: Vec::new(),
        }
    }

    pub fn push_diff(&mut self, diff: FileDiff) {
        self.diffs.push(diff);
    }

    pub fn push_tool_call(&mut self, tool: impl Into<String>, label: impl Into<String>) {
        self.tool_calls.push((tool.into(), label.into()));
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
        self.follow_tail = true;
        self.turns.clear();
        self.diffs.clear();
        self.tool_calls.clear();
    }

    pub fn start_turn(&mut self, prompt: impl Into<String>) {
        self.status = AgentRunStatus::Running;
        self.current_action = "Thinking…".to_string();
        self.summary.clear();
        self.progress = 0;
        self.scroll_offset = 0;
        self.follow_tail = true;
        self.turns.push(ConversationTurn {
            prompt: prompt.into(),
            response: String::new(),
        });
        self.trim_transcript();
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
        if let Some(turn) = self.turns.last_mut() {
            turn.response.push_str(chunk);
            self.trim_transcript();
        } else {
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

    /// Replace only the current response with its clean final form.
    pub fn replace_buffer(&mut self, content: &str) {
        if let Some(turn) = self.turns.last_mut() {
            turn.response = content.to_owned();
            self.trim_transcript();
        } else {
            self.stream_buffer = content.to_owned();
        }
    }

    pub fn transcript_text(&self) -> String {
        if self.turns.is_empty() {
            return self.stream_buffer.clone();
        }
        self.turns
            .iter()
            .map(|turn| format!("Vous:\n{}\n\nCortex:\n{}", turn.prompt, turn.response))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }

    fn trim_transcript(&mut self) {
        const MAX_TRANSCRIPT_CHARS: usize = 50_000;
        let mut total = self
            .turns
            .iter()
            .map(|turn| turn.prompt.chars().count() + turn.response.chars().count())
            .sum::<usize>();
        while self.turns.len() > 1 && total > MAX_TRANSCRIPT_CHARS {
            let removed = self.turns.remove(0);
            total = total
                .saturating_sub(removed.prompt.chars().count() + removed.response.chars().count());
        }
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

        let visible_indices = visible_agent_indices(self.agents, self.focused_agent);
        let panel_areas = visible_agent_areas(inner, visible_indices.len(), self.focused_agent);
        for (index, panel_area) in visible_indices.into_iter().zip(panel_areas) {
            render_agent_block(frame, &self.agents[index], panel_area, self.tick_count);
        }
    }
}

/// Scrolls the agent panels currently visible in `area`.
///
/// A negative delta moves toward older content; a positive delta returns toward
/// the bottom. Manual scrolling uses an absolute line index so streaming does
/// not move the viewport underneath the user.
pub fn scroll_visible_agents(
    agents: &mut [ActiveAgent],
    focused_agent: Option<&str>,
    area: Rect,
    delta: isize,
) -> bool {
    if agents.is_empty() || area.width < 3 || area.height < 3 {
        return false;
    }

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let visible_indices = visible_agent_indices(agents, focused_agent);
    let panel_areas = visible_agent_areas(inner, visible_indices.len(), focused_agent);
    if visible_indices.is_empty() {
        return false;
    }

    for (index, panel_area) in visible_indices.into_iter().zip(panel_areas) {
        scroll_agent(&mut agents[index], panel_area, delta);
    }
    true
}

pub fn scroll_hovered_agent(
    agents: &mut [ActiveAgent],
    focused_agent: Option<&str>,
    area: Rect,
    column: u16,
    row: u16,
    delta: isize,
) -> bool {
    if agents.is_empty() || area.width < 3 || area.height < 3 {
        return false;
    }
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let visible_indices = visible_agent_indices(agents, focused_agent);
    let panel_areas = visible_agent_areas(inner, visible_indices.len(), focused_agent);
    for (index, panel_area) in visible_indices.into_iter().zip(panel_areas) {
        if column >= panel_area.x
            && column < panel_area.right()
            && row >= panel_area.y
            && row < panel_area.bottom()
        {
            scroll_agent(&mut agents[index], panel_area, delta);
            return true;
        }
    }
    false
}

fn scroll_agent(agent: &mut ActiveAgent, panel_area: Rect, delta: isize) {
    let content_area = agent_content_area(agent, panel_area);
    let total_lines = build_agent_content_lines(agent, content_area.width as usize).len();
    let tail_start = total_lines.saturating_sub(content_area.height as usize);
    let current_start = if agent.follow_tail {
        tail_start
    } else {
        agent.scroll_offset.min(tail_start)
    };
    if delta < 0 {
        agent.scroll_offset = current_start.saturating_sub(delta.unsigned_abs());
        agent.follow_tail = false;
    } else {
        let next = current_start.saturating_add(delta as usize).min(tail_start);
        agent.scroll_offset = next;
        agent.follow_tail = next == tail_start;
    }
}

fn visible_agent_indices(agents: &[ActiveAgent], focused_agent: Option<&str>) -> Vec<usize> {
    if let Some(target) = focused_agent
        && let Some(index) = agents
            .iter()
            .position(|agent| agent.name == target || agent.name.starts_with(&format!("{target}:")))
    {
        return vec![index];
    }

    let mut indices: Vec<usize> = (0..agents.len()).collect();
    indices.sort_by(|&idx_a, &idx_b| {
        let a_active = agents[idx_a].status != AgentRunStatus::Done;
        let b_active = agents[idx_b].status != AgentRunStatus::Done;
        if a_active && !b_active {
            std::cmp::Ordering::Less
        } else if !a_active && b_active {
            std::cmp::Ordering::Greater
        } else {
            idx_b.cmp(&idx_a)
        }
    });
    indices.truncate(6);
    indices.sort_unstable();
    indices
}

fn visible_agent_areas(inner: Rect, count: usize, focused_agent: Option<&str>) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    if focused_agent.is_some() {
        return vec![inner];
    }

    let max_cols = (inner.width as usize / 35).max(1);
    let desired_cols = match count {
        1 => 1,
        2 => 2,
        3 | 4 => 2,
        _ => 3,
    };
    let cols = desired_cols.min(max_cols);
    let rows = count.div_ceil(cols);
    let row_rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            (0..rows)
                .map(|_| Constraint::Ratio(1, rows as u32))
                .collect::<Vec<_>>(),
        )
        .split(inner);

    let mut areas = Vec::with_capacity(count);
    for row_rect in row_rects.iter().take(rows) {
        let col_rects = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                (0..cols)
                    .map(|_| Constraint::Ratio(1, cols as u32))
                    .collect::<Vec<_>>(),
            )
            .split(*row_rect);
        for col_rect in col_rects.iter().take(cols) {
            if areas.len() == count {
                return areas;
            }
            areas.push(*col_rect);
        }
    }
    areas
}

fn agent_content_area(agent: &ActiveAgent, area: Rect) -> Rect {
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    if agent.status != AgentRunStatus::Done && inner.height >= 3 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner)[0]
    } else {
        inner
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

    let all_lines = build_agent_content_lines(agent, panel_width);

    if all_lines.is_empty() {
        frame.render_widget(Paragraph::new(vec![Line::from("")]), content_area);
    } else {
        let total = all_lines.len();
        let tail_start = total.saturating_sub(available_lines);
        let start = if agent.follow_tail {
            tail_start
        } else {
            agent.scroll_offset.min(tail_start)
        };
        let end = (start + available_lines).min(total);
        let visible = all_lines[start..end].to_vec();
        frame.render_widget(Paragraph::new(visible), content_area);
        if tail_start > 0 && area.width >= 24 {
            let hint_area = Rect::new(area.right().saturating_sub(20), area.y, 19, 1);
            frame.render_widget(
                Paragraph::new("↕ wheel/PageUp").style(Style::default().fg(THEME.muted)),
                hint_area,
            );
        }
    }
}

fn build_agent_content_lines(agent: &ActiveAgent, panel_width: usize) -> Vec<Line<'static>> {
    let mut all_lines = Vec::new();

    for (tool, label) in &agent.tool_calls {
        all_lines.extend(render_tool_call_line(tool, label, panel_width));
    }
    if !agent.tool_calls.is_empty() {
        all_lines.push(Line::from(""));
    }

    for diff in &agent.diffs {
        all_lines.extend(render_diff_inline(diff, panel_width));
        all_lines.push(Line::from(""));
    }

    if !agent.turns.is_empty() {
        for (index, turn) in agent.turns.iter().enumerate() {
            if index > 0 {
                all_lines.push(Line::from(Span::styled(
                    "────────────────────────────────",
                    Style::default().fg(THEME.muted),
                )));
                all_lines.push(Line::from(""));
            }
            all_lines.push(Line::from(Span::styled(
                "Vous",
                Style::default()
                    .fg(THEME.secondary)
                    .add_modifier(Modifier::BOLD),
            )));
            all_lines.extend(build_content_lines(&turn.prompt, panel_width.max(20)));
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(Span::styled(
                "Cortex",
                Style::default()
                    .fg(THEME.primary)
                    .add_modifier(Modifier::BOLD),
            )));
            let clean_response = crate::assistant::strip_tool_calls_for_display(&turn.response);
            all_lines.extend(build_content_lines(&clean_response, panel_width.max(20)));
        }
    } else if !agent.stream_buffer.is_empty() {
        let clean_buffer = crate::assistant::strip_tool_calls_for_display(&agent.stream_buffer);
        let content_lines = build_content_lines(&clean_buffer, panel_width.max(20));
        if agent.status == AgentRunStatus::Running {
            let total = content_lines.len();
            for (i, line) in content_lines.into_iter().enumerate() {
                if i + 4 >= total {
                    all_lines.push(line);
                } else {
                    let dimmed = line
                        .spans
                        .into_iter()
                        .map(|span| {
                            Span::styled(span.content, span.style.fg(Color::Rgb(130, 140, 150)))
                        })
                        .collect::<Vec<_>>();
                    all_lines.push(Line::from(dimmed));
                }
            }
        } else {
            all_lines.extend(content_lines);
        }
    }

    all_lines
}

/// Render a tool call as a single styled line — Claude Code style:
/// `  ✓ write_file(specs.md)`  in accent + checkmark green.
fn render_tool_call_line(tool: &str, label: &str, width: usize) -> Vec<Line<'static>> {
    let icon = "✓ ";
    let call_text = format!("{}({})", tool, label);
    let call_truncated = truncate_str(&call_text, width.saturating_sub(4));
    vec![Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            icon,
            Style::default()
                .fg(THEME.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            call_truncated,
            Style::default()
                .fg(THEME.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ])]
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

    let summary = if diff.added_count > 0 && diff.removed_count > 0 && !diff.is_new_file {
        format!(
            "  └ Added {}, removed {}",
            diff.added_count, diff.removed_count
        )
    } else if diff.removed_count > 0 && !diff.is_new_file {
        format!("  └ Removed {} lines", diff.removed_count)
    } else {
        format!("  └ Added {} lines", diff.added_count)
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

        // Parse block and inline markdown before visual wrapping so markers can
        // never be split across terminal lines.
        if let Some((level, rest)) = parse_heading(line) {
            let mut style = Style::default()
                .fg(THEME.primary)
                .add_modifier(Modifier::BOLD);
            if level == 1 {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            lines.extend(wrap_styled_spans(
                parse_inline_spans(rest, style),
                width.max(1),
                None,
            ));
        } else if is_horizontal_rule(line) {
            lines.push(Line::from(Span::styled(
                "─".repeat(width.min(48)),
                Style::default().fg(THEME.muted),
            )));
        } else if let Some(rest) = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("+ "))
        {
            lines.extend(wrap_styled_spans(
                parse_inline_spans(rest, Style::default().fg(THEME.text)),
                width.saturating_sub(2).max(1),
                Some((
                    Span::styled("• ", Style::default().fg(THEME.primary)),
                    "  ".to_string(),
                )),
            ));
        } else if let Some(rest) = line.strip_prefix("> ") {
            lines.extend(wrap_styled_spans(
                parse_inline_spans(rest, Style::default().fg(THEME.muted)),
                width.saturating_sub(2).max(1),
                Some((
                    Span::styled("▎ ", Style::default().fg(THEME.secondary)),
                    "  ".to_string(),
                )),
            ));
        } else if is_ordered_list_item(line) {
            let (number, rest) = parse_ordered_list_item(line);
            let prefix = format!("{number}. ");
            let continuation = " ".repeat(prefix.chars().count());
            lines.extend(wrap_styled_spans(
                parse_inline_spans(rest, Style::default().fg(THEME.text)),
                width.saturating_sub(prefix.chars().count()).max(1),
                Some((
                    Span::styled(prefix, Style::default().fg(THEME.primary)),
                    continuation,
                )),
            ));
        } else {
            let leading = line.len() - line.trim_start().len();
            let indent = &line[..leading];
            let content = line.trim_start();
            let wrap_w = width.saturating_sub(leading).max(1);
            let prefix =
                (!indent.is_empty()).then(|| (Span::raw(indent.to_string()), indent.to_string()));
            lines.extend(wrap_styled_spans(
                parse_inline_spans(content, Style::default().fg(THEME.text)),
                wrap_w,
                prefix,
            ));
        }
    }

    lines
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&level) && line.as_bytes().get(level) == Some(&b' ') {
        Some((level, &line[level + 1..]))
    } else {
        None
    }
}

fn is_horizontal_rule(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    compact.len() >= 3
        && (compact.chars().all(|ch| ch == '-')
            || compact.chars().all(|ch| ch == '*')
            || compact.chars().all(|ch| ch == '_'))
}

fn is_ordered_list_item(line: &str) -> bool {
    let digit_end = line
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(line.len());
    digit_end > 0
        && digit_end < line.len()
        && (line[digit_end..].starts_with(". ") || line[digit_end..].starts_with(") "))
}

fn parse_ordered_list_item(line: &str) -> (&str, &str) {
    let digit_end = line
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(line.len());
    (&line[..digit_end], &line[digit_end + 2..])
}

fn wrap_styled_spans(
    spans: Vec<Span<'static>>,
    width: usize,
    prefix: Option<(Span<'static>, String)>,
) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut result = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_len = 0usize;

    let flush = |result: &mut Vec<Line<'static>>,
                 current: &mut Vec<Span<'static>>,
                 current_len: &mut usize| {
        if !current.is_empty() {
            result.push(Line::from(std::mem::take(current)));
            *current_len = 0;
        }
    };

    for span in spans {
        let style = span.style;
        let mut word = String::new();
        let push_word = |word: &mut String,
                         result: &mut Vec<Line<'static>>,
                         current: &mut Vec<Span<'static>>,
                         current_len: &mut usize| {
            if word.is_empty() {
                return;
            }
            let word_len = word.chars().count();
            if *current_len > 0 && *current_len + word_len > width {
                flush(result, current, current_len);
            }
            if word_len <= width {
                current.push(Span::styled(std::mem::take(word), style));
                *current_len += word_len;
            } else {
                let chars = std::mem::take(word).chars().collect::<Vec<_>>();
                for chunk in chars.chunks(width) {
                    if *current_len > 0 {
                        flush(result, current, current_len);
                    }
                    current.push(Span::styled(chunk.iter().collect::<String>(), style));
                    *current_len = chunk.len();
                    flush(result, current, current_len);
                }
            }
        };

        for ch in span.content.chars() {
            if ch.is_whitespace() {
                push_word(&mut word, &mut result, &mut current, &mut current_len);
                if current_len > 0 && current_len < width {
                    current.push(Span::styled(" ", style));
                    current_len += 1;
                }
            } else {
                word.push(ch);
            }
        }
        push_word(&mut word, &mut result, &mut current, &mut current_len);
    }
    flush(&mut result, &mut current, &mut current_len);
    if result.is_empty() {
        result.push(Line::from(""));
    }

    if let Some((first_prefix, continuation)) = prefix {
        for (index, line) in result.iter_mut().enumerate() {
            let prefix = if index == 0 {
                first_prefix.clone()
            } else {
                Span::raw(continuation.clone())
            };
            line.spans.insert(0, prefix);
        }
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
    build_content_lines(text, 10_000)
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
        // Inline code: `code`
        else if chars[i] == '`' {
            let inner_start = i + 1;
            let mut j = inner_start;
            while j < n && chars[j] != '`' {
                j += 1;
            }
            if j < n {
                flush!();
                let code: String = chars[inner_start..j].iter().collect();
                spans.push(Span::styled(
                    code,
                    Style::default()
                        .fg(Color::Rgb(255, 200, 100))
                        .add_modifier(Modifier::BOLD),
                ));
                i = j + 1;
            } else {
                buf.push(chars[i]);
                i += 1;
            }
        }
        // Markdown links: [label](url)
        else if chars[i] == '[' {
            let label_start = i + 1;
            let mut label_end = label_start;
            while label_end < n && chars[label_end] != ']' {
                label_end += 1;
            }
            if label_end + 1 < n && chars[label_end + 1] == '(' {
                let url_start = label_end + 2;
                let mut url_end = url_start;
                while url_end < n && chars[url_end] != ')' {
                    url_end += 1;
                }
                if url_end < n {
                    flush!();
                    let label: String = chars[label_start..label_end].iter().collect();
                    let url: String = chars[url_start..url_end].iter().collect();
                    spans.push(Span::styled(
                        label,
                        Style::default()
                            .fg(THEME.secondary)
                            .add_modifier(Modifier::BOLD),
                    ));
                    spans.push(Span::styled(
                        format!(" ({url})"),
                        Style::default().fg(THEME.muted),
                    ));
                    i = url_end + 1;
                } else {
                    buf.push(chars[i]);
                    i += 1;
                }
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
    fn cortex_transcript_keeps_previous_prompt_and_response() {
        let mut agent = ActiveAgent::new("cortex");
        agent.start_turn("premier prompt");
        agent.push_chunk("premiere reponse");
        agent.replace_buffer("premiere reponse finale");

        agent.start_turn("deuxieme prompt");
        agent.push_chunk("deuxieme reponse");
        agent.replace_buffer("deuxieme reponse finale");

        assert_eq!(agent.turns.len(), 2);
        assert_eq!(agent.turns[0].prompt, "premier prompt");
        assert_eq!(agent.turns[0].response, "premiere reponse finale");
        assert_eq!(agent.turns[1].prompt, "deuxieme prompt");
        assert_eq!(agent.turns[1].response, "deuxieme reponse finale");
    }

    #[test]
    fn replace_buffer_only_replaces_current_cortex_response() {
        let mut agent = ActiveAgent::new("cortex");
        agent.start_turn("un");
        agent.replace_buffer("reponse un");
        agent.start_turn("deux");
        agent.push_chunk("bruit de streaming");

        agent.replace_buffer("reponse deux");

        assert_eq!(agent.turns[0].response, "reponse un");
        assert_eq!(agent.turns[1].response, "reponse deux");
    }

    #[test]
    fn transcript_limit_removes_complete_oldest_turns() {
        let mut agent = ActiveAgent::new("cortex");
        for index in 0..3 {
            agent.start_turn(format!("prompt {index}"));
            agent.replace_buffer(&format!("response-{index}-{}", "x".repeat(20_000)));
        }

        assert_eq!(agent.turns.len(), 2);
        assert_eq!(agent.turns[0].prompt, "prompt 1");
        assert_eq!(agent.turns[1].prompt, "prompt 2");
        assert!(agent.turns[0].response.starts_with("response-1-"));
    }

    #[test]
    fn markdown_is_parsed_before_wrapping() {
        let text =
            "### Heading\nThis contains **a bold phrase spanning several words** after text.";
        let lines = build_content_lines(text, 18);
        let rendered = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(!rendered.contains("###"));
        assert!(!rendered.contains("**"));
        assert!(lines.iter().flat_map(|line| line.spans.iter()).any(|span| {
            span.style.add_modifier.contains(Modifier::BOLD) && span.content.contains("bold")
        }));
    }

    #[test]
    fn manual_scroll_position_stays_fixed_while_tokens_arrive() {
        let mut agent = ActiveAgent::new("cortex");
        agent.start_turn("prompt");
        agent.replace_buffer(
            &(0..60)
                .map(|index| format!("line {index}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        agent.finish();
        let area = Rect::new(0, 0, 80, 20);
        let mut agents = vec![agent];

        assert!(scroll_visible_agents(&mut agents, None, area, -8));
        let fixed_start = agents[0].scroll_offset;
        assert!(!agents[0].follow_tail);

        agents[0].push_chunk("\nnew streamed line");

        assert_eq!(agents[0].scroll_offset, fixed_start);
        assert!(!agents[0].follow_tail);
    }

    #[test]
    fn scrolling_is_bounded_by_available_content() {
        let mut agent = ActiveAgent::new("cortex");
        agent.stream_buffer = (0..40)
            .map(|index| format!("line {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        agent.finish();
        let mut agents = vec![agent];
        let area = Rect::new(0, 0, 80, 20);

        assert!(scroll_visible_agents(&mut agents, None, area, -1_000));
        assert_eq!(agents[0].scroll_offset, 0);
        assert!(!agents[0].follow_tail);

        assert!(scroll_visible_agents(&mut agents, None, area, 1_000));
        assert!(agents[0].follow_tail);
        assert!(agents[0].scroll_offset > 0);
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
    fn headless_render_keeps_two_markdown_turns_visible() {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        let mut agent = ActiveAgent::new("cortex");
        agent.start_turn("premier prompt");
        agent.replace_buffer("### Première réponse\n**important**");
        agent.start_turn("deuxième prompt");
        agent.replace_buffer("### Deuxième réponse\n- détail");
        agent.finish();
        let agents = vec![agent];

        terminal
            .draw(|frame| {
                AgentPanelWidget {
                    agents: &agents,
                    focused_agent: None,
                    tick_count: 0,
                }
                .render(frame, frame.area());
            })
            .unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("premier prompt"));
        assert!(rendered.contains("Première réponse"));
        assert!(rendered.contains("deuxième prompt"));
        assert!(rendered.contains("Deuxième réponse"));
        assert!(!rendered.contains("###"));
        assert!(!rendered.contains("**"));
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
