#![allow(dead_code)]

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Paragraph,
    Frame,
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

        let text = if tokens > 0 {
            format!(
                " cortex v0.1.0  │  provider: {}  │  model: {}  │  tokens: {}  │  elapsed: {:02}:{:02}  │  Ctrl+C or /quit to exit ",
                self.state.provider,
                self.state.model,
                tokens,
                elapsed_mins,
                elapsed_s,
            )
        } else {
            format!(
                " cortex v0.1.0  │  provider: {}  │  model: {}  │  elapsed: {:02}:{:02}  │  Ctrl+C or /quit to exit ",
                self.state.provider,
                self.state.model,
                elapsed_mins,
                elapsed_s,
            )
        };

        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().bg(Color::DarkGray).fg(Color::White)),
            area,
        );
    }
}
