use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use similar::{ChangeTag, TextDiff};

use crate::tui::theme::THEME;

const MAX_DIFF_LINES: usize = 10_000;

#[derive(Debug, Clone)]
pub enum DiffLine {
    Added { line_no: usize, text: String },
    Removed { line_no: usize, text: String },
    Context { line_no: usize, text: String },
}

#[derive(Debug, Clone)]
pub struct FileDiff {
    pub agent: String,
    pub path: String,
    pub lines: Vec<DiffLine>,
    pub added_count: usize,
    pub removed_count: usize,
    pub is_new_file: bool,
    pub too_large: bool,
}

impl FileDiff {
    pub fn compute(agent: &str, path: &str, old: Option<&str>, new: &str) -> Self {
        let is_new_file = old.is_none();
        let old_str = old.unwrap_or("");

        let total_lines = old_str.lines().count() + new.lines().count();
        if total_lines > MAX_DIFF_LINES {
            return Self {
                agent: agent.to_string(),
                path: path.to_string(),
                lines: vec![],
                added_count: new.lines().count(),
                removed_count: old_str.lines().count(),
                is_new_file,
                too_large: true,
            };
        }

        let diff = TextDiff::from_lines(old_str, new);
        let mut lines = Vec::new();
        let mut added_count = 0usize;
        let mut removed_count = 0usize;
        let mut old_line = 1usize;
        let mut new_line = 1usize;

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Insert => {
                    let text = change.value().trim_end_matches('\n').to_string();
                    lines.push(DiffLine::Added {
                        line_no: new_line,
                        text,
                    });
                    added_count += 1;
                    new_line += 1;
                }
                ChangeTag::Delete => {
                    let text = change.value().trim_end_matches('\n').to_string();
                    lines.push(DiffLine::Removed {
                        line_no: old_line,
                        text,
                    });
                    removed_count += 1;
                    old_line += 1;
                }
                ChangeTag::Equal => {
                    let text = change.value().trim_end_matches('\n').to_string();
                    lines.push(DiffLine::Context {
                        line_no: new_line,
                        text,
                    });
                    old_line += 1;
                    new_line += 1;
                }
            }
        }

        Self {
            agent: agent.to_string(),
            path: path.to_string(),
            lines,
            added_count,
            removed_count,
            is_new_file,
            too_large: false,
        }
    }
}

pub struct DiffViewerWidget<'a> {
    pub diff: &'a FileDiff,
    pub scroll_offset: usize,
    pub index: usize,
    pub total: usize,
}

impl<'a> DiffViewerWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let popup_area = centered_rect(92, 82, area);
        frame.render_widget(Clear, popup_area);

        let title = if self.diff.is_new_file {
            format!(
                " New file: {} [{}/{}] +{} lines ",
                self.diff.path, self.index, self.total, self.diff.added_count
            )
        } else if self.diff.too_large {
            format!(
                " {} [{}/{}] (fichier trop grand pour le diff) ",
                self.diff.path, self.index, self.total
            )
        } else {
            format!(
                " {} [{}/{}] +{} -{} ",
                self.diff.path,
                self.index,
                self.total,
                self.diff.added_count,
                self.diff.removed_count
            )
        };

        let block = Block::default()
            .title(Span::styled(title, THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.active_border_style());

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Split inner into: header (1 line) + content (fill) + hint (1 line)
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(inner);

        // Header: agent name
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                "Agent: ",
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(THEME.muted),
            ),
            Span::styled(&self.diff.agent, Style::default().fg(THEME.accent)),
        ]));
        frame.render_widget(header, chunks[0]);

        // Content: diff lines
        if self.diff.too_large {
            let msg = Paragraph::new(Span::styled(
                "(fichier trop grand — diff non calculé)",
                Style::default().fg(THEME.muted),
            ));
            frame.render_widget(msg, chunks[1]);
        } else {
            let visible_height = chunks[1].height as usize;
            let scroll = self
                .scroll_offset
                .min(self.diff.lines.len().saturating_sub(1));
            let content_width = chunks[1].width as usize;

            let rendered_lines: Vec<Line> = self
                .diff
                .lines
                .iter()
                .skip(scroll)
                .take(visible_height)
                .map(|dl| match dl {
                    DiffLine::Added { line_no, text } => Line::from(vec![
                        Span::styled(format!("{:>5} ", line_no), Style::default().fg(THEME.muted)),
                        Span::styled(
                            "+ ",
                            Style::default()
                                .fg(THEME.success)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            truncate(text, content_width.saturating_sub(8)),
                            Style::default().fg(THEME.success),
                        ),
                    ]),
                    DiffLine::Removed { line_no, text } => Line::from(vec![
                        Span::styled(format!("{:>5} ", line_no), Style::default().fg(THEME.muted)),
                        Span::styled(
                            "- ",
                            Style::default()
                                .fg(THEME.error)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            truncate(text, content_width.saturating_sub(8)),
                            Style::default().fg(THEME.error),
                        ),
                    ]),
                    DiffLine::Context { line_no, text } => Line::from(vec![
                        Span::styled(format!("{:>5} ", line_no), Style::default().fg(THEME.muted)),
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            truncate(text, content_width.saturating_sub(8)),
                            Style::default().fg(THEME.muted),
                        ),
                    ]),
                })
                .collect();

            let paragraph = Paragraph::new(rendered_lines);
            frame.render_widget(paragraph, chunks[1]);
        }

        // Hint bar
        let hint = if self.total > 1 {
            " [j/k] défiler  [n] suivant  [p] précédent  [q/Esc] fermer "
        } else {
            " [j/k] défiler  [q/Esc] fermer "
        };
        let hint_widget = Paragraph::new(Span::styled(hint, Style::default().fg(THEME.muted)));
        frame.render_widget(hint_widget, chunks[2]);
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vert = Layout::default()
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
        .split(vert[1])[1]
}
