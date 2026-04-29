#![allow(dead_code)]

use crate::tui::theme::THEME;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
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
                "  ✨ Workflow complete!",
                Style::default()
                    .fg(THEME.success)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
        ];

        // Output directory
        lines.push(Line::from(vec![
            Span::styled("  📂 Output: ", Style::default().fg(THEME.primary)),
            Span::styled(s.output_dir.clone(), Style::default().fg(THEME.text)),
        ]));
        lines.push(Line::from(""));

        // File tree
        lines.push(Line::from(Span::styled(
            "  📄 Files created:",
            Style::default()
                .fg(THEME.warning)
                .add_modifier(Modifier::BOLD),
        )));
        for f in &s.files {
            lines.push(Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(f.clone(), Style::default().fg(THEME.text)),
            ]));
        }

        // Git hash
        if let Some(hash) = &s.git_hash {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  🔖 Git: ", Style::default().fg(THEME.primary)),
                Span::styled(hash.clone(), Style::default().fg(THEME.secondary)),
            ]));
        }

        // Launch command hint
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  🚀 Launch: ", Style::default().fg(THEME.primary)),
            Span::styled(
                "docker-compose up",
                Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
            ),
        ]));

        let block = Block::default()
            .title(Span::styled(" Summary ", THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.border_style());

        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}
