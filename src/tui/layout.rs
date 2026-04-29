use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

/// The five regions that make up the Cortex TUI.
pub struct AppLayout {
    pub pipeline: Rect,
    pub agents: Rect,
    pub logs: Rect,
    pub input: Rect,
    pub status: Rect,
}

pub fn compute(frame: &Frame) -> AppLayout {
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

    // Main area: agents (60%) | logs (40%)
    // Added a small horizontal margin between agents and logs
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(outer[1]);

    AppLayout {
        pipeline: outer[0],
        agents: main[0],
        logs: main[1],
        input: outer[2],
        status: outer[3],
    }
}
