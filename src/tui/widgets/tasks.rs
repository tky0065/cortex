use crate::tui::events::Task;
use crate::tui::theme::THEME;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

/// Widget for displaying the list of tasks and their completion status.
pub struct TasksWidget<'a> {
    pub tasks: &'a [Task],
}

impl<'a> TasksWidget<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let done_count = self.tasks.iter().filter(|t| t.is_done).count();
        let total_count = self.tasks.len();
        
        let title = if total_count > 0 {
            format!(" Tasks ({}/{}) ", done_count, total_count)
        } else {
            " Tasks ".to_string()
        };

        let block = Block::default()
            .title(Span::styled(title, THEME.title_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(THEME.border_style());

        if self.tasks.is_empty() {
            let msg = Paragraph::new("No tasks defined yet.")
                .block(block)
                .style(Style::default().fg(THEME.muted))
                .alignment(Alignment::Center);
            frame.render_widget(msg, area);
            return;
        }

        let lines: Vec<Line> = self.tasks.iter().map(|task| {
            let (checkbox, style) = if task.is_done {
                (
                    Span::styled("[x] ", Style::default().fg(THEME.success).add_modifier(Modifier::BOLD)),
                    Style::default().fg(THEME.muted).add_modifier(Modifier::CROSSED_OUT),
                )
            } else {
                (
                    Span::styled("[ ] ", Style::default().fg(THEME.primary)),
                    Style::default().fg(THEME.text),
                )
            };

            let description = Span::styled(task.description.clone(), style);
            Line::from(vec![checkbox, description])
        }).collect();

        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}
