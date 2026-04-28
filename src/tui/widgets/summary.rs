#![allow(dead_code)]

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Data stored in App once `WorkflowComplete` is received.
#[derive(Debug, Clone)]
pub struct WorkflowSummary {
    pub output_dir: String,
    pub files: Vec<String>,
    pub git_hash: Option<String>,
}

/// Full-panel summary rendered when the workflow has completed.
pub struct SummaryWidget<'a> {
    pub summary: &'a WorkflowSummary,
}

impl<'a> SummaryWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let s = self.summary;

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                "  Workflow complete!",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Output directory
        lines.push(Line::from(vec![
            Span::styled("  Output: ", Style::default().fg(Color::Cyan)),
            Span::raw(s.output_dir.clone()),
        ]));
        lines.push(Line::from(""));

        // File tree
        lines.push(Line::from(Span::styled(
            "  Files created:",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for f in &s.files {
            lines.push(Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::raw(f.clone()),
            ]));
        }

        // Git hash
        if let Some(hash) = &s.git_hash {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Git: ", Style::default().fg(Color::Cyan)),
                Span::styled(hash.clone(), Style::default().fg(Color::Magenta)),
            ]));
        }

        // Launch command hint
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Run: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                "docker-compose up",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        let block = Block::default()
            .title(" Summary ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}
