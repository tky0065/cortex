pub mod events;
pub mod layout;
pub mod widgets;

use std::io::{self, Stdout};
use std::sync::Arc;

use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame, Terminal,
};
use tokio::time::{self, Duration};
use tui_input::backend::crossterm::EventHandler;

use crate::config::Config;
use crate::tui::{
    events::{TuiEvent, TuiReceiver, TuiSender},
    layout::compute,
    widgets::{
        agent_panel::{ActiveAgent, AgentPanelWidget},
        input::InputBar,
        logs::{LogEntry, LogsWidget},
        pipeline::{AgentState, AgentStatus, PipelineWidget},
    },
};

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct App {
    input_bar: InputBar,
    logs: Vec<LogEntry>,
    pipeline: Vec<AgentState>,
    active_agents: Vec<ActiveAgent>,
    repl_state: Arc<crate::repl::ReplState>,
    /// Task 42: whether the interactive-pause popup is visible
    show_pause_popup: bool,
    /// Task 42: message shown in the pause popup
    pause_message: String,
}

impl App {
    fn new() -> Self {
        Self {
            input_bar: InputBar::new(),
            logs: vec![LogEntry::system("cortex ready — type /help for commands.")],
            pipeline: Vec::new(),
            active_agents: Vec::new(),
            repl_state: Arc::new(crate::repl::ReplState::new()),
            show_pause_popup: false,
            pause_message: String::new(),
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let layout = compute(frame);

        PipelineWidget { agents: &self.pipeline }.render(frame, layout.pipeline);
        AgentPanelWidget { agents: &self.active_agents }.render(frame, layout.agents);
        LogsWidget { entries: &self.logs, filter: None }.render(frame, layout.logs);
        self.input_bar.render(frame, layout.input);
        draw_status(frame, layout.status);

        // Task 42: overlay pause popup when active
        if self.show_pause_popup {
            let popup_area = centered_rect(60, 30, frame.area());
            frame.render_widget(Clear, popup_area);

            let body = format!(
                "\n {}\n\n [C]ontinue    [A]bort",
                self.pause_message
            );
            let block = Block::default()
                .title(" Workflow Paused ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));
            frame.render_widget(
                Paragraph::new(body)
                    .block(block)
                    .style(Style::default().fg(Color::White)),
                popup_area,
            );
        }
    }

    fn on_orchestrator_event(&mut self, event: TuiEvent) {
        match &event {
            TuiEvent::WorkflowStarted { workflow, agents } => {
                self.pipeline = agents.iter().map(|n| AgentState::idle(n)).collect();
                self.active_agents.clear();
                self.logs.push(LogEntry::system(format!(
                    "workflow '{}' started ({} agents)",
                    workflow,
                    agents.len()
                )));
            }
            TuiEvent::AgentStarted { agent } => {
                self.set_pipeline_status(agent, AgentStatus::Running);
                if !self.active_agents.iter().any(|a| &a.name == agent) {
                    self.active_agents.push(ActiveAgent::new(agent.clone()));
                }
                self.logs.push(LogEntry::agent(agent, "started"));
            }
            TuiEvent::TokenChunk { agent, chunk } => {
                // Task 41: auto-create an agent block for workers that send chunks
                // without a prior AgentStarted event (e.g. developer:src/main.rs).
                if !self.active_agents.iter().any(|a| &a.name == agent) {
                    self.active_agents.push(ActiveAgent::new(agent.clone()));
                }
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.push_chunk(chunk);
                }
                self.logs.push(LogEntry::agent(agent, chunk.clone()));
            }
            TuiEvent::AgentDone { agent } => {
                self.set_pipeline_status(agent, AgentStatus::Done);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.finish();
                }
                self.logs.push(LogEntry::agent(agent, "✓ done"));
            }
            TuiEvent::PhaseComplete { phase } => {
                self.logs.push(LogEntry::system(format!("[phase:{}] complete", phase)));
            }
            TuiEvent::Error { agent, message } => {
                self.set_pipeline_status(agent, AgentStatus::Error);
                self.logs.push(LogEntry::agent(agent, format!("✗ {}", message)));
            }
            TuiEvent::InteractivePause { message } => {
                // Task 42: show popup
                self.show_pause_popup = true;
                self.pause_message = message.clone();
                self.logs.push(LogEntry::system(format!("[pause] {}", message)));
            }
            TuiEvent::Resume => {
                // Task 42: dismiss popup
                self.show_pause_popup = false;
                self.logs.push(LogEntry::system("workflow resumed"));
            }
            TuiEvent::WorkflowStats { tokens_total } => {
                self.logs.push(LogEntry::system(format!("tokens used: {}", tokens_total)));
            }
            TuiEvent::WorkflowComplete { output_dir, files, git_hash } => {
                self.logs.push(LogEntry::system(format!(
                    "workflow complete — output: {} ({} files){}",
                    output_dir,
                    files.len(),
                    git_hash.as_deref().map(|h| format!(", git: {}", h)).unwrap_or_default(),
                )));
            }
        }
    }

    fn set_pipeline_status(&mut self, agent: &str, status: AgentStatus) {
        if let Some(a) = self.pipeline.iter_mut().find(|a| a.name == agent) {
            a.status = status;
        }
    }
}

// ---------------------------------------------------------------------------
// Popup helper — returns a centered rect within `area`
// ---------------------------------------------------------------------------

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ---------------------------------------------------------------------------
// Tui — owns the Terminal and drives the event loop
// ---------------------------------------------------------------------------

pub struct Tui {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl Tui {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Async event loop. Returns when the user presses Ctrl+C or types `/quit`.
    pub async fn run(mut self, mut event_rx: TuiReceiver, tx: TuiSender, config: Arc<Config>) -> Result<()> {
        let mut app = App::new();
        let mut stream = EventStream::new();
        let mut tick = time::interval(Duration::from_millis(100));

        loop {
            self.terminal.draw(|f| app.draw(f))?;

            tokio::select! {
                maybe = stream.next() => {
                    if let Some(Ok(event)) = maybe
                        && Self::handle_input(&mut app, &event, &tx, Arc::clone(&config)).await {
                        break;
                    }
                }
                Some(ev) = event_rx.recv() => {
                    app.on_orchestrator_event(ev);
                }
                _ = tick.tick() => {}
            }
        }

        Ok(())
    }

    /// Returns `true` when the loop should exit.
    async fn handle_input(app: &mut App, event: &Event, tx: &TuiSender, config: Arc<Config>) -> bool {
        let Event::Key(key) = event else { return false };

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }

        // Task 42: when the pause popup is visible, 'c'/'C' continues and 'a'/'A' aborts.
        if app.show_pause_popup {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    app.show_pause_popup = false;
                    match crate::repl::dispatch("/continue", tx, config, Arc::clone(&app.repl_state)).await {
                        Ok(should_quit) => return should_quit,
                        Err(e) => {
                            let _ = tx.send(TuiEvent::Error {
                                agent: "repl".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    app.show_pause_popup = false;
                    match crate::repl::dispatch("/abort", tx, config, Arc::clone(&app.repl_state)).await {
                        Ok(should_quit) => return should_quit,
                        Err(e) => {
                            let _ = tx.send(TuiEvent::Error {
                                agent: "repl".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                }
                _ => {}
            }
            return false;
        }

        if key.code == KeyCode::Enter {
            let cmd = app.input_bar.input.value().to_string();
            if !cmd.is_empty() {
                app.logs.push(LogEntry::system(format!("> {}", cmd)));
                app.input_bar.input = tui_input::Input::default();

                match crate::repl::dispatch(&cmd, tx, config, Arc::clone(&app.repl_state)).await {
                    Ok(should_quit) => return should_quit,
                    Err(e) => {
                        let _ = tx.send(TuiEvent::Error {
                            agent: "repl".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
            return false;
        }

        app.input_bar.input.handle_event(event);
        false
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

fn draw_status(frame: &mut Frame, area: ratatui::layout::Rect) {
    use ratatui::{
        style::{Color, Style},
        widgets::Paragraph,
    };
    let text = " cortex v0.1.0  │  provider: ollama  │  Ctrl+C or /quit to exit ";
    frame.render_widget(
        Paragraph::new(text).style(Style::default().bg(Color::DarkGray).fg(Color::White)),
        area,
    );
}
