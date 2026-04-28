use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use tui_input::Input;

/// All slash commands with their short descriptions.
const COMMANDS: &[(&str, &str)] = &[
    ("/start",    "Launch a workflow"),
    ("/run",      "Alias for /start"),
    ("/resume",   "Resume an interrupted workflow"),
    ("/status",   "Show current workflow status"),
    ("/abort",    "Cancel the running workflow"),
    ("/continue", "Resume an interactive pause"),
    ("/config",   "Print active configuration"),
    ("/model",    "Switch model"),
    ("/provider", "Connect provider"),
    ("/logs",     "Toggle log panel"),
    ("/help",     "Show all commands"),
    ("/quit",     "Exit cortex"),
    ("/exit",     "Exit cortex"),
];

pub struct InputBar {
    pub input: Input,
    /// Past commands, oldest first.
    history: Vec<String>,
    /// Current position while navigating history (None = live draft).
    history_idx: Option<usize>,
    /// Text that was in the input before Up was first pressed.
    saved_draft: String,
    // Legacy fields kept for test compatibility
    /// @deprecated — use palette_idx instead
    pub completion_prefix: Option<String>,
    /// @deprecated — use palette_idx instead
    pub completion_idx: Option<usize>,
    /// Index of the currently highlighted row in the command palette.
    palette_idx: usize,
}

impl InputBar {
    pub fn new() -> Self {
        Self {
            input: Input::default(),
            history: Vec::new(),
            history_idx: None,
            saved_draft: String::new(),
            completion_prefix: None,
            completion_idx: None,
            palette_idx: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Command palette
    // -----------------------------------------------------------------------

    /// Returns true when the command palette should be visible.
    pub fn palette_open(&self) -> bool {
        let v = self.input.value();
        v.starts_with('/') && !v.contains(' ')
    }

    /// Commands that match what the user has typed so far.
    pub fn palette_matches(&self) -> Vec<(&'static str, &'static str)> {
        let prefix = self.input.value();
        if !prefix.starts_with('/') {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .map(|(cmd, desc)| (*cmd, *desc))
            .collect()
    }

    /// Move the palette cursor up.
    pub fn palette_up(&mut self) {
        if self.palette_idx > 0 {
            self.palette_idx -= 1;
        }
    }

    /// Move the palette cursor down.
    pub fn palette_down(&mut self) {
        let len = self.palette_matches().len();
        if len > 0 && self.palette_idx + 1 < len {
            self.palette_idx += 1;
        }
    }

    /// Select the currently highlighted palette item — replaces the input value.
    /// Returns the selected command string (e.g. "/start"), or None if palette is empty.
    pub fn palette_select(&mut self) -> Option<String> {
        let matches = self.palette_matches();
        if matches.is_empty() {
            return None;
        }
        let idx = self.palette_idx.min(matches.len() - 1);
        let (cmd, _) = matches[idx];
        // Commands that take arguments get a trailing space; others are complete.
        let value = if REQUIRES_ARGS.contains(&cmd) {
            format!("{} ", cmd)
        } else {
            cmd.to_string()
        };
        self.input = Input::new(value.clone());
        self.palette_idx = 0;
        Some(value)
    }

    // -----------------------------------------------------------------------
    // History
    // -----------------------------------------------------------------------

    pub fn push_history(&mut self, cmd: String) {
        if cmd.is_empty() {
            return;
        }
        if self.history.last().map(|s| s.as_str()) != Some(&cmd) {
            self.history.push(cmd);
        }
        self.history_idx = None;
        self.saved_draft.clear();
        self.completion_prefix = None;
        self.completion_idx = None;
        self.palette_idx = 0;
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let new_idx = match self.history_idx {
            None => {
                self.saved_draft = self.input.value().to_string();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_idx = Some(new_idx);
        self.input = Input::new(self.history[new_idx].clone());
        self.completion_prefix = None;
        self.completion_idx = None;
    }

    pub fn history_down(&mut self) {
        match self.history_idx {
            None => {}
            Some(i) if i + 1 >= self.history.len() => {
                self.history_idx = None;
                self.input = Input::new(self.saved_draft.clone());
                self.completion_prefix = None;
                self.completion_idx = None;
            }
            Some(i) => {
                self.history_idx = Some(i + 1);
                self.input = Input::new(self.history[i + 1].clone());
                self.completion_prefix = None;
                self.completion_idx = None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Legacy Tab-completion (kept for test compatibility)
    // -----------------------------------------------------------------------

    pub fn active_suggestions(&self) -> Vec<&'static str> {
        let prefix = self.completion_prefix.as_deref().unwrap_or_else(|| self.input.value());
        if prefix.is_empty() || !prefix.starts_with('/') {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .map(|(cmd, _)| *cmd)
            .collect()
    }

    pub fn complete(&mut self) {
        if self.completion_prefix.is_none() {
            self.completion_prefix = Some(self.input.value().to_string());
        }
        let suggestions = self.active_suggestions();
        if suggestions.is_empty() {
            self.completion_prefix = None;
            return;
        }
        if suggestions.len() == 1 {
            let cmd = suggestions[0];
            let new_val = if REQUIRES_ARGS.contains(&cmd) {
                format!("{} ", cmd)
            } else {
                cmd.to_string()
            };
            self.input = Input::new(new_val);
            self.completion_prefix = None;
            self.completion_idx = None;
            return;
        }
        let next_idx = match self.completion_idx {
            None => 0,
            Some(i) => (i + 1) % suggestions.len(),
        };
        self.completion_idx = Some(next_idx);
        self.input = Input::new(suggestions[next_idx].to_string());
    }

    pub fn dismiss_completions(&mut self) {
        self.completion_prefix = None;
        self.completion_idx = None;
        self.palette_idx = 0;
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    /// Render just the input bar into `area` (3 rows).
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let inner_width = area.width.saturating_sub(4) as usize;
        let scroll = self.input.visual_scroll(inner_width);
        let widget = Paragraph::new(format!("> {}", self.input.value()))
            .scroll((0, scroll as u16))
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title(" Command ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            );
        frame.render_widget(widget, area);

        let cursor_x = area.x + 1 + 2 + (self.input.cursor() - scroll) as u16;
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    /// Render the command palette as a floating overlay anchored above `input_area`.
    /// Call this after `render()`, passing the full terminal area and the input bar rect.
    pub fn render_palette(&self, frame: &mut Frame, full_area: Rect, input_area: Rect) {
        if !self.palette_open() {
            return;
        }
        let matches = self.palette_matches();
        if matches.is_empty() {
            return;
        }

        // Palette height: one row per match + 2 border rows, capped so it doesn't exceed screen.
        let palette_h = (matches.len() as u16 + 2)
            .min(full_area.height.saturating_sub(input_area.height + 2));
        if palette_h < 3 {
            return;
        }

        // Position the palette directly above the input bar, same horizontal span.
        let palette_area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(palette_h),
            width: input_area.width,
            height: palette_h,
        };

        let cursor = self.palette_idx.min(matches.len().saturating_sub(1));
        let cmd_col = matches.iter().map(|(c, _)| c.len()).max().unwrap_or(8) + 3;

        use ratatui::widgets::Clear;
        frame.render_widget(Clear, palette_area);

        let items: Vec<ListItem> = matches
            .iter()
            .enumerate()
            .map(|(i, (cmd, desc))| {
                let selected = i == cursor;
                let (cmd_style, desc_style, row_bg) = if selected {
                    (
                        Style::default()
                            .fg(Color::White)
                            .bg(Color::Rgb(180, 80, 20))
                            .add_modifier(Modifier::BOLD),
                        Style::default()
                            .fg(Color::Rgb(220, 180, 160))
                            .bg(Color::Rgb(180, 80, 20)),
                        Color::Rgb(180, 80, 20),
                    )
                } else {
                    (
                        Style::default().fg(Color::White),
                        Style::default().fg(Color::DarkGray),
                        Color::Reset,
                    )
                };
                let cmd_padded = format!(" {:<width$}", cmd, width = cmd_col);
                let line = Line::from(vec![
                    Span::styled(cmd_padded, cmd_style),
                    Span::styled(desc.to_string(), desc_style),
                ])
                .style(Style::default().bg(row_bg));
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .style(Style::default().bg(Color::Rgb(18, 18, 18))),
        );
        frame.render_widget(list, palette_area);
    }
}

/// Commands that require arguments and get a trailing space when selected.
const REQUIRES_ARGS: &[&str] = &["/start", "/run", "/resume", "/model", "/provider"];

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use tui_input::backend::crossterm::EventHandler;

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn type_into(bar: &mut InputBar, text: &str) {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        for ch in text.chars() {
            bar.input.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            )));
        }
    }

    #[test]
    fn renders_input_bar() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                InputBar::new().render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_with_text() {
        let mut terminal = make_terminal();
        let mut bar = InputBar::new();
        type_into(&mut bar, "hello world");
        terminal
            .draw(|f| {
                let area = f.area();
                bar.render(f, area);
            })
            .unwrap();
    }

    // -----------------------------------------------------------------------
    // History tests
    // -----------------------------------------------------------------------

    #[test]
    fn history_navigates_up_and_down() {
        let mut bar = InputBar::new();
        bar.push_history("/start dev \"idea1\"".to_string());
        bar.push_history("/status".to_string());

        bar.history_up();
        assert_eq!(bar.input.value(), "/status");

        bar.history_up();
        assert_eq!(bar.input.value(), "/start dev \"idea1\"");

        bar.history_down();
        assert_eq!(bar.input.value(), "/status");

        bar.history_down();
        assert_eq!(bar.input.value(), "");
        assert!(bar.history_idx.is_none());
    }

    #[test]
    fn history_deduplicates() {
        let mut bar = InputBar::new();
        bar.push_history("/status".to_string());
        bar.push_history("/status".to_string());
        assert_eq!(bar.history.len(), 1);
    }

    #[test]
    fn history_restores_draft() {
        let mut bar = InputBar::new();
        bar.push_history("/help".to_string());
        type_into(&mut bar, "/sta");

        bar.history_up();
        assert_eq!(bar.input.value(), "/help");

        bar.history_down();
        assert_eq!(bar.input.value(), "/sta");
    }

    #[test]
    fn history_up_at_top_stays() {
        let mut bar = InputBar::new();
        bar.push_history("/help".to_string());
        bar.history_up();
        bar.history_up();
        assert_eq!(bar.input.value(), "/help");
    }

    // -----------------------------------------------------------------------
    // Palette tests
    // -----------------------------------------------------------------------

    #[test]
    fn tab_completes_unique_prefix() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/sta");
        let suggestions = bar.active_suggestions();
        assert!(suggestions.len() >= 2);
    }

    #[test]
    fn tab_cycles_completions() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/s");
        let count = bar.active_suggestions().len();
        assert!(count > 1);

        bar.complete();
        assert_eq!(bar.completion_idx, Some(0));
        bar.complete();
        assert_eq!(bar.completion_idx, Some(1));
        bar.complete();
        assert_eq!(bar.completion_idx, Some(2 % count));
    }

    #[test]
    fn tab_single_match_inserts() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/hel");
        bar.complete();
        assert!(bar.input.value().starts_with("/help"));
        assert!(bar.completion_idx.is_none());
    }

    #[test]
    fn no_suggestions_without_slash() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "hello");
        assert!(bar.active_suggestions().is_empty());
    }

    #[test]
    fn dismiss_clears_completion_idx() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/s");
        bar.complete();
        assert!(bar.completion_idx.is_some());
        bar.dismiss_completions();
        assert!(bar.completion_idx.is_none());
    }

    #[test]
    fn palette_opens_on_slash() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/");
        assert!(bar.palette_open());
        assert!(!bar.palette_matches().is_empty());
    }

    #[test]
    fn palette_filters_as_typed() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/mod");
        let m = bar.palette_matches();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].0, "/model");
    }

    #[test]
    fn palette_navigation() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/");
        assert_eq!(bar.palette_idx, 0);
        bar.palette_down();
        assert_eq!(bar.palette_idx, 1);
        bar.palette_up();
        assert_eq!(bar.palette_idx, 0);
    }

    #[test]
    fn palette_select_inserts_command() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/hel");
        let selected = bar.palette_select();
        assert_eq!(selected, Some("/help".to_string()));
        assert_eq!(bar.input.value(), "/help");
    }
}


