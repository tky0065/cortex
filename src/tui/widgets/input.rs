use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use tui_input::Input;

pub struct InputBar {
    pub input: Input,
}

impl InputBar {
    pub fn new() -> Self {
        Self { input: Input::default() }
    }

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

        // Place cursor after the "> " prompt prefix (2 chars) plus input offset
        let cursor_x = area.x + 1 + 2 + (self.input.cursor() - scroll) as u16;
        let cursor_y = area.y + 1;
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    use tui_input::backend::crossterm::EventHandler;

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
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
        // Simulate typing by handling key events
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        for ch in "hello world".chars() {
            bar.input.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            )));
        }
        terminal
            .draw(|f| {
                let area = f.area();
                bar.render(f, area);
            })
            .unwrap();
    }
}
