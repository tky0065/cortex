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
    pub cwd: &'a str,
    pub git_info: Option<&'a str>,
    pub mode: &'a str,
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
        let total_width = area.width as usize;

        let sep = " │ ";

        // --- Right section (always fully visible) ---
        let time_str = format!("{:02}:{:02}", elapsed_mins, elapsed_s);
        let hint = "  Shift+Tab: mode";
        let mut right_width = sep.len() + "TIME: ".len() + time_str.len() + hint.len();
        if tokens > 0 {
            right_width += sep.len() + "TOKENS: ".len() + tokens.to_string().len();
        }

        // --- Left section budget ---
        let available = total_width.saturating_sub(right_width);

        // Compute fixed left widths (everything except the MODEL value)
        let mode_color = match self.state.mode {
            "PLAN" => THEME.warning,
            "AUTO" => THEME.success,
            "REVIEW" => Color::Rgb(249, 115, 22),
            _ => THEME.text,
        };
        let provider_upper = self.state.provider.to_uppercase();
        // Strip leading "provider/" prefix from the model name since provider is shown separately
        let model_raw = {
            let prefix = format!("{}/", self.state.provider.to_lowercase());
            let m = self.state.model.to_lowercase();
            if m.starts_with(&prefix) {
                &self.state.model[prefix.len()..]
            } else {
                self.state.model
            }
        };
        let model_upper = model_raw.to_uppercase();

        let mut left_fixed = "MODE: ".len()
            + self.state.mode.len()
            + sep.len()
            + "DIR: ".len()
            + self.state.cwd.len()
            + sep.len()
            + "PROVIDER: ".len()
            + provider_upper.len()
            + sep.len()
            + "MODEL: ".len();

        if let Some(git) = self.state.git_info {
            left_fixed += sep.len() + "GIT: ".len() + git.len();
        }

        let model_budget = available.saturating_sub(left_fixed);
        let model_display = if model_upper.len() > model_budget && model_budget > 1 {
            format!("{}…", &model_upper[..model_budget.saturating_sub(1)])
        } else {
            model_upper.clone()
        };

        // --- Build all spans ---
        let separator = Span::styled(sep, Style::default().fg(THEME.muted));

        let mut spans = vec![
            Span::styled("MODE: ", Style::default().fg(THEME.muted)),
            Span::styled(
                self.state.mode,
                Style::default().fg(mode_color).add_modifier(Modifier::BOLD),
            ),
            separator.clone(),
            Span::styled("DIR: ", Style::default().fg(THEME.muted)),
            Span::styled(
                self.state.cwd,
                Style::default()
                    .fg(THEME.primary)
                    .add_modifier(Modifier::BOLD),
            ),
        ];

        if let Some(git) = self.state.git_info {
            spans.push(separator.clone());
            spans.push(Span::styled("GIT: ", Style::default().fg(THEME.muted)));
            spans.push(Span::styled(
                git,
                Style::default()
                    .fg(THEME.accent)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        spans.push(separator.clone());
        spans.push(Span::styled("PROVIDER: ", Style::default().fg(THEME.muted)));
        spans.push(Span::styled(
            provider_upper,
            Style::default()
                .fg(THEME.primary)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(separator.clone());
        spans.push(Span::styled("MODEL: ", Style::default().fg(THEME.muted)));
        spans.push(Span::styled(
            model_display,
            Style::default()
                .fg(THEME.accent)
                .add_modifier(Modifier::BOLD),
        ));

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
        spans.push(Span::styled(time_str, Style::default().fg(THEME.text)));
        spans.push(Span::styled(hint, Style::default().fg(THEME.muted)));

        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Reset)),
            area,
        );
    }
}
