#![allow(dead_code)]

use crate::tui::theme::THEME;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

/// Rendered state fed to the status bar widget each frame.
pub struct StatusBarState<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub elapsed_secs: u64,
    pub tokens_total: usize,
}

/// A single-line status bar showing provider, model, token count and elapsed time.
pub struct StatusBarWidget<'a> {
    pub state: &'a StatusBarState<'a>,
}

impl<'a> StatusBarWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let elapsed_mins = self.state.elapsed_secs / 60;
        let elapsed_s = self.state.elapsed_secs % 60;
        let tokens = self.state.tokens_total;

        let separator = Span::styled(" │ ", Style::default().fg(THEME.muted));

        let mut spans = vec![
            Span::styled(
                format!(" CORTEX v{} ", env!("CARGO_PKG_VERSION")),
                Style::default()
                    .fg(THEME.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            separator.clone(),
            Span::styled("PROVIDER: ", Style::default().fg(THEME.muted)),
            Span::styled(
                self.state.provider.to_uppercase(),
                Style::default()
                    .fg(THEME.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            separator.clone(),
            Span::styled("MODEL: ", Style::default().fg(THEME.muted)),
            Span::styled(
                self.state.model.to_uppercase(),
                Style::default()
                    .fg(THEME.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ];

        if tokens > 0 {
            spans.push(separator.clone());
            spans.push(Span::styled("TOKENS: ", Style::default().fg(THEME.muted)));
            spans.push(Span::styled(
                tokens.to_string(),
                Style::default()
                    .fg(THEME.success)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        spans.push(separator.clone());
        spans.push(Span::styled("TIME: ", Style::default().fg(THEME.muted)));
        spans.push(Span::styled(
            format!("{:02}:{:02}", elapsed_mins, elapsed_s),
            Style::default().fg(THEME.text),
        ));

        spans.push(Span::styled("  ┃  ", Style::default().fg(THEME.muted)));
        spans.push(Span::styled(
            "Ctrl+C or /quit to exit",
            Style::default()
                .fg(THEME.muted)
                .add_modifier(Modifier::ITALIC),
        ));

        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Reset)),
            area,
        );
    }
}
