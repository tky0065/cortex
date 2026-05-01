use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

/// The six regions that make up the Cortex TUI.
pub struct AppLayout {
    pub pipeline: Rect,
    pub agents: Rect,
    pub tasks: Rect,
    pub logs: Rect,
    pub input: Rect,
    pub status: Rect,
}

pub fn compute(frame: &Frame, task_count: usize) -> AppLayout {
    // Vertical split: pipeline (3) | main (fill) | input (3) | status (1)
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    // Main area: agents (80%) | right panel (20%)
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(80), Constraint::Percentage(20)])
        .split(outer[1]);

    // Right panel: tasks (dynamic) | logs (remaining)
    // Task height: 2 (borders) + task_count, capped at 60% of available height
    let task_height = (task_count + 2) as u16;
    let right_panel = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Max(task_height),
            Constraint::Min(5), // Ensure at least some logs are visible
        ])
        .split(main[1]);

    AppLayout {
        pipeline: outer[0],
        agents: main[0],
        tasks: right_panel[0],
        logs: right_panel[1],
        input: outer[2],
        status: outer[3],
    }
}
