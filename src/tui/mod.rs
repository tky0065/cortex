pub mod events;
pub mod layout;
pub mod widgets;
pub mod theme;

use std::collections::HashMap;
use std::io::{self, Stdout};
use std::sync::Arc;

use anyhow::Result;
use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Span,
    widgets::{Block, Borders, Clear, Paragraph},
};
use tokio::sync::RwLock;
use tokio::time::{self, Duration};
use tui_input::backend::crossterm::EventHandler;

use crate::config::Config;
use crate::tui::{
    events::{TuiEvent, TuiReceiver, TuiSender},
    layout::compute,
    theme::THEME,
    widgets::{
        agent_panel::{ActiveAgent, AgentPanelWidget},
        input::InputBar,
        logs::{LogEntry, LogsWidget},
        picker::{PickerState, PickerWidget, model_picker, provider_picker, resume_picker},
        pipeline::{AgentState, AgentStatus, PipelineWidget},
        status_bar::{StatusBarState, StatusBarWidget},
    },
};

// ---------------------------------------------------------------------------
// Popup state machine
// ---------------------------------------------------------------------------

enum PopupState {
    None,
    ProviderPicker(PickerState),
    /// First phase: pick a role. Second phase: pick a model from the provider's model list.
    ModelPicker {
        picker: PickerState,
        /// While `Some`, the user is in phase 2 selecting a model for this role.
        editing_role: Option<String>,
        /// Searchable picker of models available for the current provider (phase 2).
        model_list: Option<PickerState>,
        /// Shown while models are being fetched.
        is_loading: bool,
    },
    /// Prompt the user to enter an API key for a provider that requires one.
    ApiKeyInput {
        provider: String,
        input: tui_input::Input,
    },
    /// Resume picker: shows session history for resuming workflows.
    ResumePicker(PickerState),
    /// An agent is asking the user a clarification question; wait for text answer.
    QuestionInput {
        agent: String,
        question: String,
        input: tui_input::Input,
    },
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct App {
    input_bar: InputBar,
    logs: Vec<LogEntry>,
    pipeline: Vec<AgentState>,
    active_agents: Vec<ActiveAgent>,
    log_filter: Option<String>,
    repl_state: Arc<crate::repl::ReplState>,
    config: Arc<RwLock<Config>>,
    popup: PopupState,
    /// Interactive-pause popup
    show_pause_popup: bool,
    pause_message: String,
    /// Cached model lists per provider, populated by background fetch.
    model_cache: HashMap<String, Vec<String>>,
    /// Total tokens used in current session
    tokens_total: usize,
    /// When the current workflow started
    start_time: Option<std::time::Instant>,
}

impl App {
    fn new(config: Arc<RwLock<Config>>) -> Self {
        Self {
            input_bar: InputBar::new(),
            logs: vec![LogEntry::system("cortex ready — type /help for commands.")],
            pipeline: Vec::new(),
            active_agents: Vec::new(),
            log_filter: None,
            repl_state: Arc::new(crate::repl::ReplState::new()),
            config,
            popup: PopupState::None,
            show_pause_popup: false,
            pause_message: String::new(),
            model_cache: HashMap::new(),
            tokens_total: 0,
            start_time: None,
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let layout = compute(frame);

        let (provider, model) = self
            .config
            .try_read()
            .map(|c| (c.provider.default.clone(), c.models.assistant.clone()))
            .unwrap_or_else(|_| ("unknown".to_string(), "unknown".to_string()));

        PipelineWidget {
            agents: &self.pipeline,
        }
        .render(frame, layout.pipeline);
        AgentPanelWidget {
            agents: &self.active_agents,
            focused_agent: self.log_filter.as_deref(),
        }
        .render(frame, layout.agents);
        LogsWidget {
            entries: &self.logs,
            filter: self.log_filter.as_deref(),
        }
        .render(frame, layout.logs);
        self.input_bar.render(frame, layout.input);
        // Command palette floats above the input bar (drawn after so it's on top)
        self.input_bar
            .render_palette(frame, frame.area(), layout.input);
        
        let elapsed = self.start_time.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        StatusBarWidget {
            state: &StatusBarState {
                provider: &provider,
                model: &model,
                elapsed_secs: elapsed,
                tokens_total: self.tokens_total,
            },
        }
        .render(frame, layout.status);

        // Interactive-pause overlay
        if self.show_pause_popup {
            let popup_area = centered_rect(60, 30, frame.area());
            frame.render_widget(Clear, popup_area);
            let body = format!("\n {}\n\n [C]ontinue    [A]bort", self.pause_message);
            let block = Block::default()
                .title(" Workflow Paused ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(THEME.warning));
            frame.render_widget(
                Paragraph::new(body)
                    .block(block)
                    .style(Style::default().fg(THEME.text)),
                popup_area,
            );
        }

        // Picker overlays (drawn last so they appear on top)
        match &self.popup {
            PopupState::None => {}
            PopupState::ProviderPicker(state) => {
                PickerWidget { state }.render(frame);
            }
            PopupState::ModelPicker {
                picker,
                editing_role,
                model_list,
                is_loading,
            } => {
                PickerWidget { state: picker }.render(frame);
                if editing_role.is_some() {
                    if *is_loading {
                        draw_loading_overlay(frame);
                    } else if let Some(ml) = model_list {
                        PickerWidget { state: ml }.render(frame);
                    }
                }
            }
            PopupState::ApiKeyInput { provider, input } => {
                draw_api_key_overlay(frame, provider, input);
            }
            PopupState::ResumePicker(state) => {
                PickerWidget { state }.render(frame);
            }
            PopupState::QuestionInput { agent, question, input } => {
                draw_question_overlay(frame, agent, question, input);
            }
        }
    }

    fn on_orchestrator_event(&mut self, event: TuiEvent) {
        match &event {
            TuiEvent::WorkflowStarted { workflow, agents } => {
                self.pipeline = agents.iter().map(|n| AgentState::idle(n)).collect();
                self.active_agents.clear();
                self.tokens_total = 0;
                self.start_time = Some(std::time::Instant::now());
                self.logs.push(LogEntry::system(format!(
                    "workflow '{}' started ({} agents)",
                    workflow,
                    agents.len()
                )));
            }
            TuiEvent::AgentStarted { agent } => {
                self.set_pipeline_status(agent, AgentStatus::Running);
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.restart();
                }
                self.logs.push(LogEntry::agent(agent, "started"));
            }
            TuiEvent::AgentProgress { agent, message } => {
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.set_progress(message);
                }
                // Heartbeat messages are reflected in the agent panel status line only —
                // do NOT spam the logs panel with every 5s tick.
            }
            TuiEvent::AgentSummary { agent, summary } => {
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.set_summary(summary);
                }
                self.logs.push(LogEntry::agent(agent, summary.clone()));
            }
            TuiEvent::TokenChunk { agent, chunk } => {
                // Auto-create an agent block for workers that send chunks
                // without a prior AgentStarted event (e.g. developer:src/main.rs).
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.push_chunk(chunk);
                }
                // Tokens flow into the agent panel stream buffer — do NOT log each chunk.
            }
            TuiEvent::AgentDone { agent } => {
                self.set_pipeline_status(agent, AgentStatus::Done);
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.finish();
                }
                self.logs.push(LogEntry::agent(agent, "✓ done"));
            }
            TuiEvent::PhaseComplete { phase } => {
                self.logs
                    .push(LogEntry::system(format!("[phase:{}] complete", phase)));
            }
            TuiEvent::Error { agent, message } => {
                self.set_pipeline_status(agent, AgentStatus::Error);
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.fail(message);
                }
                self.logs.push(LogEntry::error(agent, message.clone()));
            }
            TuiEvent::ResumeSelected { session_id } => {
                // Log the selection; actual dispatch happens in handle_resume_picker.
                self.logs
                    .push(LogEntry::system(format!("resume selected: {}", session_id)));
            }
            TuiEvent::InteractivePause { message } => {
                // Task 42: show popup
                self.show_pause_popup = true;
                self.pause_message = message.clone();
                self.logs
                    .push(LogEntry::system(format!("[pause] {}", message)));
            }
            TuiEvent::UserQuestion { agent, question } => {
                self.logs.push(LogEntry::system(format!(
                    "[question:{}] {}",
                    agent, question
                )));
                self.popup = PopupState::QuestionInput {
                    agent: agent.clone(),
                    question: question.clone(),
                    input: tui_input::Input::default(),
                };
            }
            TuiEvent::Resume => {
                self.show_pause_popup = false;
                self.logs.push(LogEntry::system("workflow resumed"));
            }
            TuiEvent::WorkflowStats { tokens_total } => {
                self.tokens_total = *tokens_total;
                self.logs
                    .push(LogEntry::system(format!("tokens used: {}", tokens_total)));
            }
            TuiEvent::WorkflowComplete {
                output_dir,
                files,
                git_hash,
            } => {
                self.logs.push(LogEntry::system(format!(
                    "workflow complete — output: {} ({} files){}",
                    output_dir,
                    files.len(),
                    git_hash
                        .as_deref()
                        .map(|h| format!(", git: {}", h))
                        .unwrap_or_default(),
                )));
            }
            TuiEvent::OpenProviderPicker => {
                let current = self
                    .config
                    .try_read()
                    .map(|c| c.provider.default.clone())
                    .unwrap_or_default();
                self.popup = PopupState::ProviderPicker(provider_picker(&current));
            }
            TuiEvent::OpenModelPicker => {
                let (ceo, pm, tl, dev, qa, devops, assistant) = self
                    .config
                    .try_read()
                    .map(|c| {
                        (
                            c.models.ceo.clone(),
                            c.models.pm.clone(),
                            c.models.tech_lead.clone(),
                            c.models.developer.clone(),
                            c.models.qa.clone(),
                            c.models.devops.clone(),
                            c.models.assistant.clone(),
                        )
                    })
                    .unwrap_or_default();
                let roles: &[(&str, String)] = &[
                    ("ceo", ceo),
                    ("pm", pm),
                    ("tech_lead", tl),
                    ("developer", dev),
                    ("qa", qa),
                    ("devops", devops),
                    ("assistant", assistant),
                ];
                let refs: Vec<(&str, &str)> = roles.iter().map(|(r, m)| (*r, m.as_str())).collect();
                self.popup = PopupState::ModelPicker {
                    picker: model_picker(&refs),
                    editing_role: None,
                    model_list: None,
                    is_loading: false,
                };
            }
            TuiEvent::ModelsLoaded { provider, models } => {
                self.model_cache.insert(provider.clone(), models.clone());
                // If model picker is open in phase 2, populate its model list now.
                if let PopupState::ModelPicker {
                    editing_role,
                    model_list,
                    is_loading,
                    ..
                } = &mut self.popup
                    && editing_role.is_some()
                {
                    *is_loading = false;
                    *model_list = Some(build_model_list_picker(provider, models));
                }
            }
            TuiEvent::OpenResumePicker => {
                let sessions = self.repl_state.session_history.lock().unwrap().clone();
                self.popup = PopupState::ResumePicker(resume_picker(&sessions));
            }
            TuiEvent::ClearLogs => {
                self.logs.clear();
                self.log_filter = None;
                self.logs.push(LogEntry::system("logs cleared"));
            }
            TuiEvent::SetLogFilter { agent } => {
                self.log_filter = agent.clone();
                match agent {
                    Some(agent) => self
                        .logs
                        .push(LogEntry::system(format!("log focus: {}", agent))),
                    None => self.logs.push(LogEntry::system("log focus cleared")),
                }
            }
        }
    }

    fn ensure_agent(&mut self, agent: &str) {
        if !self.active_agents.iter().any(|a| a.name == agent) {
            self.active_agents.push(ActiveAgent::new(agent.to_string()));
        }
    }

    fn set_pipeline_status(&mut self, agent: &str, status: AgentStatus) {
        if let Some(a) = self.pipeline.iter_mut().find(|a| a.name == agent) {
            a.status = status;
        }
    }

    /// Collect all visible text (logs + active agents content) into one string.
    fn collect_all_text(&self) -> String {
        let mut out = String::new();

        // Logs section
        for entry in &self.logs {
            let agent_tag = entry
                .agent
                .as_deref()
                .map(|a| format!("[{}] ", a))
                .unwrap_or_default();
            out.push_str(&format!("{} {}{}\n", entry.timestamp, agent_tag, entry.message));
        }

        // Active agents section
        for agent in &self.active_agents {
            let content = if !agent.summary.is_empty() {
                &agent.summary
            } else {
                &agent.stream_buffer
            };
            if !content.is_empty() {
                out.push_str(&format!("\n--- {} ---\n{}\n", agent.name, content));
            }
        }

        out
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
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    /// Async event loop. Returns when the user presses Ctrl+C or types `/quit`.
    pub async fn run(
        mut self,
        mut event_rx: TuiReceiver,
        tx: TuiSender,
        config: Arc<RwLock<Config>>,
    ) -> Result<()> {
        let mut app = App::new(Arc::clone(&config));
        let mut stream = EventStream::new();
        let mut tick = time::interval(Duration::from_millis(100));

        // Pre-fetch model list for the current provider in the background.
        {
            let provider = config.read().await.provider.default.clone();
            let tx2 = tx.clone();
            tokio::spawn(async move {
                if let Ok(models) = crate::providers::models::fetch_models(&provider).await {
                    let _ = tx2.send(TuiEvent::ModelsLoaded { provider, models });
                }
            });
        }

        loop {
            self.terminal.draw(|f| app.draw(f))?;

            tokio::select! {
                maybe = stream.next() => {
                    if let Some(Ok(event)) = maybe
                        && Self::handle_input(&mut app, &event, &tx).await {
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
    async fn handle_input(app: &mut App, event: &Event, tx: &TuiSender) -> bool {
        let Event::Key(key) = event else { return false };

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }

        // Ctrl+Y — copy all visible text (logs + agent content) to the system clipboard
        if key.code == KeyCode::Char('y') && key.modifiers.contains(KeyModifiers::CONTROL) {
            let text = app.collect_all_text();
            match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
                Ok(()) => app.logs.push(LogEntry::system("✓ copied to clipboard")),
                Err(e) => app.logs.push(LogEntry::system(format!("clipboard error: {}", e))),
            }
            return false;
        }

        // --- Picker popups (highest priority) ---
        match &app.popup {
            PopupState::ProviderPicker(_) => {
                return Self::handle_provider_picker(app, key, tx).await;
            }
            PopupState::ModelPicker { .. } => {
                return Self::handle_model_picker(app, key, tx).await;
            }
            PopupState::ApiKeyInput { .. } => {
                return Self::handle_api_key_input(app, key, tx).await;
            }
            PopupState::ResumePicker(_) => {
                return Self::handle_resume_picker(app, key, tx).await;
            }
            PopupState::QuestionInput { .. } => {
                return Self::handle_question_input(app, key, tx).await;
            }
            PopupState::None => {}
        }

        // --- Pause popup ---
        if app.show_pause_popup {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    app.show_pause_popup = false;
                    match crate::repl::dispatch(
                        "/continue",
                        tx,
                        Arc::clone(&app.config),
                        Arc::clone(&app.repl_state),
                    )
                    .await
                    {
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
                    match crate::repl::dispatch(
                        "/abort",
                        tx,
                        Arc::clone(&app.config),
                        Arc::clone(&app.repl_state),
                    )
                    .await
                    {
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

        // When the command palette is open, intercept navigation keys.
        // Enter dispatches complete commands immediately and only waits for args
        // on commands like /run and /start.
        if app.input_bar.palette_open() {
            match key.code {
                KeyCode::Up => {
                    app.input_bar.palette_up();
                    return false;
                }
                KeyCode::Down => {
                    app.input_bar.palette_down();
                    return false;
                }
                KeyCode::Tab => {
                    app.input_bar.palette_down();
                    return false;
                }
                KeyCode::Esc => {
                    app.input_bar.dismiss_completions();
                    return false;
                }
                KeyCode::Enter => {
                    if let Some(value) = app.input_bar.palette_select() {
                        if value.ends_with(' ') {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Up => {
                    app.input_bar.history_up();
                    return false;
                }
                KeyCode::Down => {
                    app.input_bar.history_down();
                    return false;
                }
                KeyCode::Tab => {
                    app.input_bar.complete();
                    return false;
                }
                KeyCode::Esc => {
                    app.input_bar.dismiss_completions();
                    return false;
                }
                _ => {}
            }
        }

        // --- Normal REPL dispatch (palette is closed at this point) ---
        if key.code == KeyCode::Enter {
            let cmd = app.input_bar.input.value().to_string();
            if !cmd.is_empty() {
                app.logs.push(LogEntry::system(format!("> {}", cmd)));
                app.input_bar.push_history(cmd.clone());
                app.input_bar.input = tui_input::Input::default();
                if Self::is_quit_command(&cmd) {
                    return true;
                }
                Self::spawn_dispatch(
                    cmd,
                    tx.clone(),
                    Arc::clone(&app.config),
                    Arc::clone(&app.repl_state),
                );
            }
            return false;
        }

        app.input_bar.input.handle_event(event);
        false
    }

    fn spawn_dispatch(
        cmd: String,
        tx: TuiSender,
        config: Arc<RwLock<Config>>,
        repl_state: Arc<crate::repl::ReplState>,
    ) {
        tokio::spawn(async move {
            match crate::repl::dispatch(&cmd, &tx, config, repl_state).await {
                Ok(true) => {
                    let _ = tx.send(TuiEvent::TokenChunk {
                        agent: "repl".to_string(),
                        chunk: "  Quit requested; use Ctrl+C if the UI did not close.".to_string(),
                    });
                }
                Ok(false) => {}
                Err(e) => {
                    let _ = tx.send(TuiEvent::Error {
                        agent: "repl".to_string(),
                        message: e.to_string(),
                    });
                }
            }
        });
    }

    fn is_quit_command(cmd: &str) -> bool {
        matches!(cmd.trim().split_whitespace().next(), Some("/quit" | "/exit"))
    }

    // -------------------------------------------------------------------------
    // Provider picker key handler
    // -------------------------------------------------------------------------

    async fn handle_provider_picker(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::ProviderPicker(ref mut state) = app.popup else {
            return false;
        };

        match key.code {
            KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Up => state.move_up(),
            KeyCode::Down => state.move_down(),
            KeyCode::Backspace => state.pop_search(),
            KeyCode::Enter => {
                if let Some(id) = state.selected_id() {
                    let id_clone = id.clone();
                    // Providers that require an API key — always prompt so user can update it
                    const NEEDS_KEY: &[&str] = &["openrouter", "groq", "together"];
                    let needs_key = NEEDS_KEY.contains(&id_clone.as_str());

                    if needs_key {
                        // Always show the key input popup (allows re-entering a wrong key)
                        app.popup = PopupState::ApiKeyInput {
                            provider: id_clone,
                            input: tui_input::Input::default(),
                        };
                        return false;
                    }

                    app.popup = PopupState::None;
                    // Apply & persist
                    let mut cfg = app.config.write().await;
                    cfg.set_provider(id_clone.clone());
                    match cfg.save() {
                        Ok(()) => {
                            let _ = tx.send(TuiEvent::TokenChunk {
                                agent: "provider".to_string(),
                                chunk: format!("  ✓ provider → {} (saved)", id_clone),
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(TuiEvent::Error {
                                agent: "provider".to_string(),
                                message: format!("failed to save config: {e}"),
                            });
                        }
                    }
                    // Kick off background model fetch for the new provider
                    let tx2 = tx.clone();
                    let provider_for_fetch = id_clone.clone();
                    tokio::spawn(async move {
                        match crate::providers::models::fetch_models(&provider_for_fetch).await {
                            Ok(models) => {
                                let _ = tx2.send(TuiEvent::ModelsLoaded {
                                    provider: provider_for_fetch,
                                    models,
                                });
                            }
                            Err(e) => {
                                let _ = tx2.send(TuiEvent::Error {
                                    agent: "models".to_string(),
                                    message: format!("model fetch failed: {e}"),
                                });
                            }
                        }
                    });
                }
            }
            KeyCode::Char(c) => state.push_search(c),
            _ => {}
        }
        false
    }

    // -------------------------------------------------------------------------
    // API key input key handler
    // -------------------------------------------------------------------------

    async fn handle_api_key_input(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::ApiKeyInput {
            ref provider,
            ref mut input,
        } = app.popup
        else {
            return false;
        };
        let provider = provider.clone();

        match key.code {
            KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Enter => {
                let api_key = input.value().trim().to_string();
                app.popup = PopupState::None;
                let mut cfg = app.config.write().await;

                if api_key.is_empty() {
                    // Blank = keep existing key, just switch the provider
                    cfg.set_provider(provider.clone());
                    cfg.apply_api_keys_to_env();
                    let _ = cfg.save();
                    let _ = tx.send(TuiEvent::TokenChunk {
                        agent: "provider".to_string(),
                        chunk: format!("  ✓ provider → {} (existing key kept)", provider),
                    });
                    drop(cfg);
                } else {
                    // Set API key and apply to env immediately
                    match cfg.set_api_key(&provider, api_key.clone()) {
                        Ok(()) => {
                            cfg.apply_api_keys_to_env();
                            cfg.set_provider(provider.clone());
                            match cfg.save() {
                                Ok(()) => {
                                    let _ = tx.send(TuiEvent::TokenChunk {
                                        agent: "provider".to_string(),
                                        chunk: format!(
                                            "  ✓ provider → {} • API key saved",
                                            provider
                                        ),
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(TuiEvent::Error {
                                        agent: "provider".to_string(),
                                        message: format!("failed to save config: {e}"),
                                    });
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(TuiEvent::Error {
                                agent: "provider".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                    drop(cfg);
                }
                // Kick off background model fetch for the new provider
                let tx2 = tx.clone();
                let prov = provider.clone();
                tokio::spawn(async move {
                    match crate::providers::models::fetch_models(&prov).await {
                        Ok(models) => {
                            let _ = tx2.send(TuiEvent::ModelsLoaded {
                                provider: prov,
                                models,
                            });
                        }
                        Err(e) => {
                            let _ = tx2.send(TuiEvent::Error {
                                agent: "models".to_string(),
                                message: format!("model fetch failed: {e}"),
                            });
                        }
                    }
                });
            }
            KeyCode::Backspace => {
                use tui_input::backend::crossterm::EventHandler;
                input.handle_event(&crossterm::event::Event::Key(*key));
            }
            KeyCode::Char(_) => {
                use tui_input::backend::crossterm::EventHandler;
                input.handle_event(&crossterm::event::Event::Key(*key));
            }
            _ => {}
        }
        false
    }

    // -------------------------------------------------------------------------
    // Resume picker key handler
    // -------------------------------------------------------------------------

    async fn handle_resume_picker(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::ResumePicker(ref mut state) = app.popup else {
            return false;
        };

        match key.code {
            KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Up => state.move_up(),
            KeyCode::Down => state.move_down(),
            KeyCode::Backspace => state.pop_search(),
            KeyCode::Enter => {
                if let Some(session_id) = state.selected_id() {
                    app.popup = PopupState::None;
                    // Look up the directory for this session and dispatch /resume <dir>
                    let dir = {
                        let history = app.repl_state.session_history.lock().unwrap();
                        history
                            .iter()
                            .find(|s| s.id == session_id)
                            .map(|s| s.directory.display().to_string())
                    };
                    if let Some(dir_str) = dir {
                        let cmd = format!("/resume {}", dir_str);
                        match crate::repl::dispatch(
                            &cmd,
                            tx,
                            Arc::clone(&app.config),
                            Arc::clone(&app.repl_state),
                        )
                        .await
                        {
                            Ok(should_quit) => return should_quit,
                            Err(e) => {
                                let _ = tx.send(TuiEvent::Error {
                                    agent: "resume".to_string(),
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let _ = tx.send(TuiEvent::Error {
                            agent: "resume".to_string(),
                            message: format!("session not found: {}", session_id),
                        });
                    }
                }
            }
            KeyCode::Char(c) => state.push_search(c),
            _ => {}
        }
        false
    }

    // -------------------------------------------------------------------------
    // Model picker key handler
    // -------------------------------------------------------------------------

    async fn handle_model_picker(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::ModelPicker {
            ref mut picker,
            ref mut editing_role,
            ref mut model_list,
            ref mut is_loading,
        } = app.popup
        else {
            return false;
        };

        if let Some(role) = editing_role.clone() {
            // Phase 2: picking a model from the list
            match key.code {
                KeyCode::Esc => {
                    // Back to role selection
                    *editing_role = None;
                    *model_list = None;
                    *is_loading = false;
                }
                KeyCode::Up => {
                    if let Some(ml) = model_list {
                        ml.move_up();
                    }
                }
                KeyCode::Down => {
                    if let Some(ml) = model_list {
                        ml.move_down();
                    }
                }
                KeyCode::Backspace => {
                    if let Some(ml) = model_list {
                        ml.pop_search();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(ml) = model_list {
                        ml.push_search(c);
                    }
                }
                KeyCode::Enter => {
                    let raw_model = model_list
                        .as_ref()
                        .and_then(|ml| ml.selected_id())
                        .unwrap_or_default();
                    if !raw_model.is_empty() {
                        // Qualify with provider prefix so parse_model() routes correctly.
                        // OpenRouter model IDs (e.g. "qwen/qwen3-coder:free") contain "/"
                        // which would otherwise be misread as an unknown provider prefix.
                        let provider = app
                            .config
                            .try_read()
                            .map(|c| c.provider.default.clone())
                            .unwrap_or_else(|_| "ollama".to_string());
                        let model_str = qualify_model_string(&raw_model, &provider);
                        app.popup = PopupState::None;
                        let mut cfg = app.config.write().await;
                        match cfg.set_model(&role, model_str.clone()) {
                            Ok(()) => match cfg.save() {
                                Ok(()) => {
                                    let _ = tx.send(TuiEvent::TokenChunk {
                                        agent: "model".to_string(),
                                        chunk: format!("  ✓ {} → {} (saved)", role, model_str),
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(TuiEvent::Error {
                                        agent: "model".to_string(),
                                        message: format!(
                                            "saved in memory but failed to persist: {e}"
                                        ),
                                    });
                                }
                            },
                            Err(e) => {
                                let _ = tx.send(TuiEvent::Error {
                                    agent: "model".to_string(),
                                    message: e.to_string(),
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        } else {
            // Phase 1: selecting a role
            match key.code {
                KeyCode::Esc => {
                    app.popup = PopupState::None;
                }
                KeyCode::Up => picker.move_up(),
                KeyCode::Down => picker.move_down(),
                KeyCode::Backspace => picker.pop_search(),
                KeyCode::Enter => {
                    if let Some(role) = picker.selected_id() {
                        // Check cache first
                        let provider = app
                            .config
                            .try_read()
                            .map(|c| c.provider.default.clone())
                            .unwrap_or_else(|_| "ollama".to_string());
                        if let Some(models) = app.model_cache.get(&provider) {
                            *model_list = Some(build_model_list_picker(&provider, models));
                            *is_loading = false;
                        } else {
                            // Not cached yet — fetch in background
                            *is_loading = true;
                            let tx2 = tx.clone();
                            let prov = provider.clone();
                            tokio::spawn(async move {
                                match crate::providers::models::fetch_models(&prov).await {
                                    Ok(models) => {
                                        let _ = tx2.send(TuiEvent::ModelsLoaded {
                                            provider: prov,
                                            models,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = tx2.send(TuiEvent::Error {
                                            agent: "models".to_string(),
                                            message: format!("model fetch failed: {e}"),
                                        });
                                    }
                                }
                            });
                        }
                        *editing_role = Some(role);
                    }
                }
                KeyCode::Char(c) => picker.push_search(c),
                _ => {}
            }
        }
        false
    }

    // -------------------------------------------------------------------------
    // Question input key handler
    // -------------------------------------------------------------------------

    async fn handle_question_input(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        _tx: &TuiSender,
    ) -> bool {
        let PopupState::QuestionInput { ref mut input, .. } = app.popup else {
            return false;
        };

        match key.code {
            KeyCode::Enter => {
                let answer = input.value().to_string();
                // Close the popup before sending to avoid any borrow issues
                app.popup = PopupState::None;
                app.logs.push(LogEntry::system(format!("answer: {}", answer)));
                // Deliver the answer to the waiting agent
                let answer_guard = app.repl_state.answer_tx.lock().await;
                if let Some(ref atx) = *answer_guard {
                    let _ = atx.send(answer).await;
                }
            }
            KeyCode::Esc => {
                app.popup = PopupState::None;
                // Send empty string so the agent is unblocked
                let answer_guard = app.repl_state.answer_tx.lock().await;
                if let Some(ref atx) = *answer_guard {
                    let _ = atx.send(String::new()).await;
                }
            }
            _ => {
                input.handle_event(&crossterm::event::Event::Key(*key));
            }
        }
        false
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

// ---------------------------------------------------------------------------
// Model list picker builder
// ---------------------------------------------------------------------------

fn build_model_list_picker(
    provider: &str,
    models: &[String],
) -> crate::tui::widgets::picker::PickerState {
    use crate::tui::widgets::picker::{PickerGroup, PickerItem, PickerState};
    let items: Vec<PickerItem> = models
        .iter()
        .map(|id| PickerItem {
            id: id.clone(),
            label: id.clone(),
            description: None,
            checked: false,
        })
        .collect();
    PickerState::new(
        format!("Models — {}", provider),
        vec![PickerGroup {
            title: provider.to_string(),
            items,
        }],
    )
}

// ---------------------------------------------------------------------------
// Provider-prefix helpers
// ---------------------------------------------------------------------------

const KNOWN_PROVIDER_PREFIXES: &[&str] = &["ollama", "openrouter", "groq", "together"];

/// Ensures a raw model ID (e.g. `"qwen/qwen3-coder:free"`) is stored with a
/// provider prefix (e.g. `"openrouter/qwen/qwen3-coder:free"`).  If the model
/// string already begins with a known prefix it is returned as-is.
fn qualify_model_string(model: &str, provider: &str) -> String {
    let already_prefixed = KNOWN_PROVIDER_PREFIXES
        .iter()
        .any(|p| model.starts_with(&format!("{p}/")));
    if already_prefixed {
        model.to_string()
    } else {
        format!("{provider}/{model}")
    }
}

// ---------------------------------------------------------------------------
// API key input overlay
// ---------------------------------------------------------------------------

fn draw_api_key_overlay(frame: &mut Frame, provider: &str, input: &tui_input::Input) {
    use ratatui::layout::{Constraint, Direction, Layout};

    // Build a fixed-height (11 rows), 50%-wide rect centered on screen.
    let screen = frame.area();
    const POPUP_H: u16 = 11;
    const POPUP_W_PCT: u16 = 50;
    let popup_w = screen.width * POPUP_W_PCT / 100;
    let popup_x = screen.x + (screen.width.saturating_sub(popup_w)) / 2;
    let popup_y = screen.y + screen.height.saturating_sub(POPUP_H) / 2;
    let area = Rect::new(popup_x, popup_y, popup_w, POPUP_H.min(screen.height));

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(format!(" 🔑  API Key — {provider} "), THEME.title_style()))
        .borders(Borders::ALL)
        .border_style(THEME.border_style())
        .style(Style::default().bg(THEME.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // inner is POPUP_H - 2 (borders) = 9 rows
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top padding
            Constraint::Length(1), // hint
            Constraint::Length(1), // spacing
            Constraint::Length(3), // input box (border + 1 line of text + border)
            Constraint::Length(1), // spacing
            Constraint::Length(1), // footer hint
            Constraint::Min(0),    // leftover
        ])
        .split(inner);

    // Hint
    frame.render_widget(
        Paragraph::new(" Paste your API key (blank = keep existing).")
            .style(Style::default().fg(THEME.muted)),
        chunks[1],
    );

    // Masked input field — show dots for typed chars, placeholder when empty
    let typed = input.value();
    let display: String = if typed.is_empty() {
        String::from(" Enter API key…")
    } else {
        format!(" {}", "•".repeat(typed.len()))
    };
    let input_style = if typed.is_empty() {
        Style::default().fg(THEME.muted)
    } else {
        Style::default().fg(THEME.text)
    };
    frame.render_widget(
        Paragraph::new(display).style(input_style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(THEME.active_border_style()),
        ),
        chunks[3],
    );

    // Footer
    frame.render_widget(
        Paragraph::new(" Enter to save  •  Esc to cancel")
            .style(Style::default().fg(THEME.muted)),
        chunks[5],
    );
}

// ---------------------------------------------------------------------------
// Loading overlay
// ---------------------------------------------------------------------------

fn draw_loading_overlay(frame: &mut Frame) {
    use crate::tui::widgets::picker;
    let area = picker::centered_rect(40, 12, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(Span::styled(" Loading models… ", THEME.title_style()))
        .borders(Borders::ALL)
        .border_style(THEME.border_style())
        .style(Style::default().bg(THEME.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new("  Fetching model list from provider…")
            .style(Style::default().fg(THEME.muted)),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Question input overlay
// ---------------------------------------------------------------------------

fn draw_question_overlay(
    frame: &mut Frame,
    agent: &str,
    question: &str,
    input: &tui_input::Input,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::text::Line;

    let screen = frame.area();
    const POPUP_H: u16 = 13;
    const POPUP_W_PCT: u16 = 60;
    let popup_w = screen.width * POPUP_W_PCT / 100;
    let popup_x = screen.x + (screen.width.saturating_sub(popup_w)) / 2;
    let popup_y = screen.y + screen.height.saturating_sub(POPUP_H) / 2;
    let area = Rect::new(popup_x, popup_y, popup_w, POPUP_H.min(screen.height));

    frame.render_widget(Clear, area);

    let title = format!(" 💬  {agent} — Question ");
    let block = Block::default()
        .title(Span::styled(title, THEME.title_style()))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(THEME.warning))
        .style(Style::default().bg(THEME.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top padding
            Constraint::Length(3), // question text (wrapped)
            Constraint::Length(1), // spacing
            Constraint::Length(3), // input box
            Constraint::Length(1), // footer hint
            Constraint::Min(0),
        ])
        .split(inner);

    // Question text
    let question_text = format!(" {}", question);
    frame.render_widget(
        Paragraph::new(Line::from(question_text))
            .style(Style::default().fg(THEME.text))
            .wrap(ratatui::widgets::Wrap { trim: false }),
        chunks[1],
    );

    // Answer input field
    let typed = input.value();
    let display = if typed.is_empty() {
        " Type your answer…".to_string()
    } else {
        format!(" {}", typed)
    };
    let input_style = if typed.is_empty() {
        Style::default().fg(THEME.muted)
    } else {
        Style::default().fg(THEME.text)
    };
    frame.render_widget(
        Paragraph::new(display).style(input_style).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(THEME.active_border_style()),
        ),
        chunks[3],
    );

    // Footer
    frame.render_widget(
        Paragraph::new(" Enter to confirm  •  Esc to skip")
            .style(Style::default().fg(THEME.muted)),
        chunks[4],
    );
}
