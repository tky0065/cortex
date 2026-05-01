pub mod events;
pub mod layout;
pub mod theme;
pub mod widgets;

use std::collections::{HashMap, HashSet};
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
        diff_viewer::{DiffViewerWidget, FileDiff},
        input::{InputBar, PaletteContext, ResumeSuggestion, default_provider_suggestions},
        logs::{LogEntry, LogsWidget},
        picker::{
            PickerState, PickerWidget, auth_method_picker, model_picker, provider_picker,
            resume_picker,
        },
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
    ConnectProviderPicker(PickerState),
    AuthMethodPicker {
        provider: String,
        picker: PickerState,
    },
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
    AuthSecretInput {
        provider: String,
        method_id: String,
        input: tui_input::Input,
    },
    AuthUrl {
        provider: String,
        url: String,
        message: String,
        copied: bool,
    },
    /// Resume picker: shows session history for resuming workflows.
    ResumePicker(PickerState),
    /// Skill picker: browse skills.sh and manage installed skills.
    SkillPicker(SkillPickerState),
    /// An agent is asking the user a clarification question; wait for text answer.
    QuestionInput {
        agent: String,
        question: String,
        input: tui_input::Input,
    },
    /// Shows a diff popup when an agent writes a file.
    DiffViewer {
        diffs: Vec<FileDiff>,
        cursor: usize,
        scroll_offset: usize,
    },
}

struct SkillPickerState {
    picker: PickerState,
    original_enabled: HashMap<String, bool>,
    remote_by_id: HashMap<String, crate::skills::RemoteSkill>,
    installed_names: HashSet<String>,
    scope: crate::skills::SkillScope,
    loading: bool,
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
    /// Frame counter for animations (incremented every 100ms tick)
    tick_count: u64,
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
            tick_count: 0,
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
            tick_count: self.tick_count,
        }
        .render(frame, layout.agents);
        LogsWidget {
            entries: &self.logs,
            filter: self.log_filter.as_deref(),
        }
        .render(frame, layout.logs);
        self.input_bar.render(frame, layout.input);
        // Command palette floats above the input bar (drawn after so it's on top)
        let palette_context = self.palette_context();
        self.input_bar
            .render_palette(frame, frame.area(), layout.input, &palette_context);

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
            PopupState::ConnectProviderPicker(state) => {
                PickerWidget { state }.render(frame);
            }
            PopupState::AuthMethodPicker { picker, .. } => {
                PickerWidget { state: picker }.render(frame);
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
            PopupState::AuthSecretInput {
                provider,
                method_id,
                input,
            } => {
                draw_api_key_overlay(frame, &format!("{provider} {method_id}"), input);
            }
            PopupState::AuthUrl {
                provider,
                url,
                message,
                copied,
            } => {
                draw_auth_url_overlay(frame, provider, url, message, *copied);
            }
            PopupState::ResumePicker(state) => {
                PickerWidget { state }.render(frame);
            }
            PopupState::SkillPicker(state) => {
                PickerWidget {
                    state: &state.picker,
                }
                .render(frame);
                if state.loading {
                    draw_loading_overlay_with(
                        frame,
                        " Loading skills… ",
                        "  Fetching skills.sh results…",
                    );
                }
            }
            PopupState::QuestionInput {
                agent,
                question,
                input,
            } => {
                draw_question_overlay(frame, agent, question, input);
            }
            PopupState::DiffViewer {
                diffs,
                cursor,
                scroll_offset,
            } => {
                if let Some(diff) = diffs.get(*cursor) {
                    DiffViewerWidget {
                        diff,
                        scroll_offset: *scroll_offset,
                        index: cursor + 1,
                        total: diffs.len(),
                    }
                    .render(frame, frame.area());
                }
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
                if is_control_agent(agent) {
                    self.logs
                        .push(LogEntry::system(format!("{agent}: started")));
                    return;
                }
                self.set_pipeline_status(agent, AgentStatus::Running);
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.restart();
                }
                self.logs.push(LogEntry::agent(agent, "started"));
            }
            TuiEvent::AgentProgress { agent, message } => {
                if is_control_agent(agent) {
                    self.logs
                        .push(LogEntry::system(format!("{agent}: {message}")));
                    return;
                }
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.set_progress(message);
                }
                // Heartbeat messages are reflected in the agent panel status line only —
                // do NOT spam the logs panel with every 5s tick.
            }
            TuiEvent::AgentSummary { agent, summary } => {
                if is_control_agent(agent) {
                    self.logs
                        .push(LogEntry::system(format!("{agent}: {summary}")));
                    return;
                }
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.set_summary(summary);
                }
                self.logs.push(LogEntry::agent(agent, summary.clone()));
            }
            TuiEvent::TokenChunk { agent, chunk } => {
                if is_control_agent(agent) {
                    self.logs
                        .push(LogEntry::system(format!("{agent}: {}", chunk.trim())));
                    return;
                }
                // Auto-create an agent block for workers that send chunks
                // without a prior AgentStarted event (e.g. developer:src/main.rs).
                self.ensure_agent(agent);
                if let Some(a) = self.active_agents.iter_mut().find(|a| &a.name == agent) {
                    a.push_chunk(chunk);
                }
                // Tokens flow into the agent panel stream buffer — do NOT log each chunk.
            }
            TuiEvent::AgentDone { agent } => {
                if is_control_agent(agent) {
                    self.logs.push(LogEntry::system(format!("{agent}: done")));
                    return;
                }
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
                if is_control_agent(agent) {
                    self.logs.push(LogEntry::error(agent, message.clone()));
                    return;
                }
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
                let (current, custom_providers) = self
                    .config
                    .try_read()
                    .map(|c| (c.provider.default.clone(), c.custom_providers.clone()))
                    .unwrap_or_default();
                self.popup =
                    PopupState::ProviderPicker(provider_picker(&current, &custom_providers));
            }
            TuiEvent::OpenConnectProviderPicker => {
                let (current, custom_providers) = self
                    .config
                    .try_read()
                    .map(|c| (c.provider.default.clone(), c.custom_providers.clone()))
                    .unwrap_or_default();
                self.popup =
                    PopupState::ConnectProviderPicker(provider_picker(&current, &custom_providers));
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
                let provider = crate::providers::registry::normalize_provider(provider).to_string();
                self.model_cache.insert(provider.clone(), models.clone());
                let current_provider = self
                    .config
                    .try_read()
                    .map(|cfg| {
                        crate::providers::registry::normalize_provider(&cfg.provider.default)
                            .to_string()
                    })
                    .unwrap_or_else(|_| "ollama".to_string());
                // For providers with no static fallback (e.g. lmstudio), the
                // synchronous sync step couldn't set models. Do it now that we
                // have the live list.
                if provider == current_provider {
                    if let Some(first) = models.first() {
                        if let Ok(mut cfg) = self.config.try_write() {
                            let still_on_old_provider =
                                crate::providers::models::model_prefix(&cfg.models.ceo)
                                    .is_none_or(|p| p != provider);
                            if still_on_old_provider {
                                let qualified = crate::providers::models::qualify_model_string(
                                    first, &provider,
                                );
                                let _ = cfg.set_model("all", qualified);
                                let _ = cfg.save();
                            }
                        }
                    }
                }
                // If model picker is open in phase 2, populate its model list now.
                if let PopupState::ModelPicker {
                    editing_role,
                    model_list,
                    is_loading,
                    ..
                } = &mut self.popup
                    && editing_role.is_some()
                    && provider == current_provider
                {
                    *is_loading = false;
                    *model_list = Some(build_model_list_picker(&provider, models));
                }
            }
            TuiEvent::AuthUrl {
                provider,
                url,
                message,
            } => {
                let copied = arboard::Clipboard::new()
                    .and_then(|mut cb| cb.set_text(url.clone()))
                    .is_ok();
                self.popup = PopupState::AuthUrl {
                    provider: provider.clone(),
                    url: url.clone(),
                    message: message.clone(),
                    copied,
                };
                if copied {
                    self.logs
                        .push(LogEntry::system(format!("{provider} auth URL copied")));
                } else {
                    self.logs.push(LogEntry::system(format!(
                        "{provider} auth URL ready; press c to copy"
                    )));
                }
            }
            TuiEvent::AuthComplete { provider, message } => {
                if let PopupState::AuthUrl {
                    provider: popup_provider,
                    ..
                } = &self.popup
                    && popup_provider == provider
                {
                    self.popup = PopupState::None;
                }
                self.logs
                    .push(LogEntry::system(format!("{provider}: {message}")));
            }
            TuiEvent::OpenResumePicker => {
                let sessions = self.repl_state.session_history.lock().unwrap().clone();
                self.popup = PopupState::ResumePicker(resume_picker(&sessions));
            }
            TuiEvent::OpenSkillPicker => {
                self.popup = PopupState::SkillPicker(build_skill_picker(Vec::new(), true));
            }
            TuiEvent::SkillsCatalogLoaded { skills } => {
                if let PopupState::SkillPicker(state) = &mut self.popup {
                    state.loading = false;
                    replace_remote_skill_group(state, "skills.sh Leaderboard", skills.clone());
                }
            }
            TuiEvent::SkillSearchLoaded { query, skills } => {
                if let PopupState::SkillPicker(state) = &mut self.popup
                    && state.picker.search == *query
                {
                    state.loading = false;
                    replace_remote_skill_group(state, "Search results", skills.clone());
                }
            }
            TuiEvent::SkillPickerError { message } => {
                if let PopupState::SkillPicker(state) = &mut self.popup {
                    state.loading = false;
                }
                self.logs
                    .push(LogEntry::system(format!("skill picker error: {}", message)));
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
            TuiEvent::FileWritten {
                agent,
                path,
                old_content,
                new_content,
            } => {
                let diff = FileDiff::compute(agent, path, old_content.as_deref(), new_content);
                self.logs.push(LogEntry::agent(
                    agent.as_str(),
                    format!(
                        "wrote {} (+{} -{} lines)",
                        path, diff.added_count, diff.removed_count
                    ),
                ));
                match &mut self.popup {
                    PopupState::DiffViewer { diffs, cursor, .. } => {
                        diffs.push(diff);
                        *cursor = diffs.len() - 1;
                    }
                    _ => {
                        self.popup = PopupState::DiffViewer {
                            diffs: vec![diff],
                            cursor: 0,
                            scroll_offset: 0,
                        };
                    }
                }
            }
        }
    }

    fn ensure_agent(&mut self, agent: &str) {
        if !self.active_agents.iter().any(|a| a.name == agent) {
            self.active_agents.push(ActiveAgent::new(agent.to_string()));
        }
    }

    fn palette_context(&self) -> PaletteContext {
        let input = self.input_bar.input.value();
        let needs_providers = input.starts_with("/provider ")
            || input.starts_with("/apikey ")
            || input.starts_with("/connect ");
        let needs_models = input.starts_with("/model ");
        let needs_agents = input.starts_with("/focus ") || input.starts_with("/agent ");
        let needs_resume_sessions = input.starts_with("/resume ");
        let needs_skills =
            input.starts_with("/skill ") || input.starts_with("/skills ") || input.contains('$');
        let needs_project_paths = input.contains('@');

        let mut providers = Vec::new();
        if needs_providers {
            providers = default_provider_suggestions();
            if let Ok(cfg) = self.config.try_read() {
                providers.extend(cfg.custom_providers.iter().map(|(name, provider)| {
                    (
                        name.clone(),
                        format!("Custom provider at {}", provider.base_url),
                    )
                }));
            }
            providers.sort_by(|a, b| a.0.cmp(&b.0));
            providers.dedup_by(|a, b| a.0 == b.0);
        }

        let models = if needs_models {
            let current_provider = self
                .config
                .try_read()
                .map(|cfg| {
                    crate::providers::registry::normalize_provider(&cfg.provider.default)
                        .to_string()
                })
                .unwrap_or_else(|_| "ollama".to_string());
            self.model_cache
                .get(&current_provider)
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let agents = if needs_agents {
            self.active_agents
                .iter()
                .map(|agent| agent.name.clone())
                .collect()
        } else {
            Vec::new()
        };

        let resume_sessions = if needs_resume_sessions {
            self.repl_state
                .session_history
                .lock()
                .map(|history| {
                    history
                        .iter()
                        .rev()
                        .map(|session| {
                            let idea = if session.idea.len() > 55 {
                                format!("{}...", &session.idea[..55])
                            } else {
                                session.idea.clone()
                            };
                            ResumeSuggestion {
                                label: format!("{} {}", session.workflow, idea),
                                description: session.directory.display().to_string(),
                                path: session.directory.display().to_string(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let skills = if needs_skills {
            crate::skills::list()
                .map(|records| {
                    records
                        .into_iter()
                        .map(|record| (record.name, record.description))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let project_paths = if needs_project_paths {
            let prefix = input
                .split_whitespace()
                .rev()
                .find_map(|token| token.strip_prefix('@'))
                .unwrap_or_default();
            crate::mentions::path_suggestions(prefix)
                .into_iter()
                .map(|suggestion| (suggestion.path, suggestion.description))
                .collect()
        } else {
            Vec::new()
        };

        PaletteContext {
            providers,
            models,
            agents,
            resume_sessions,
            skills,
            project_paths,
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
            out.push_str(&format!(
                "{} {}{}\n",
                entry.timestamp, agent_tag, entry.message
            ));
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
                let config_snapshot = config.read().await.clone();
                if let Ok(models) =
                    crate::providers::models::fetch_models_for_config(&provider, &config_snapshot)
                        .await
                {
                    let _ = tx2.send(TuiEvent::ModelsLoaded { provider, models });
                }
            });
        }

        {
            let tx2 = tx.clone();
            tokio::spawn(async move {
                if let Ok(status) = crate::updater::check_latest().await
                    && status.update_available
                {
                    let _ = tx2.send(TuiEvent::AgentStarted {
                        agent: "update".to_string(),
                    });
                    let _ = tx2.send(TuiEvent::AgentSummary {
                        agent: "update".to_string(),
                        summary: format!(
                            "Update available: {} -> {}. Run /update to install.",
                            status.current, status.latest
                        ),
                    });
                    let _ = tx2.send(TuiEvent::AgentDone {
                        agent: "update".to_string(),
                    });
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
                _ = tick.tick() => {
                    app.tick_count += 1;
                }
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
                Err(e) => app
                    .logs
                    .push(LogEntry::system(format!("clipboard error: {}", e))),
            }
            return false;
        }

        // --- Picker popups (highest priority) ---
        match &app.popup {
            PopupState::ProviderPicker(_) => {
                return Self::handle_provider_picker(app, key, tx).await;
            }
            PopupState::ConnectProviderPicker(_) => {
                return Self::handle_connect_provider_picker(app, key, tx).await;
            }
            PopupState::AuthMethodPicker { .. } => {
                return Self::handle_auth_method_picker(app, key, tx).await;
            }
            PopupState::ModelPicker { .. } => {
                return Self::handle_model_picker(app, key, tx).await;
            }
            PopupState::ApiKeyInput { .. } => {
                return Self::handle_api_key_input(app, key, tx).await;
            }
            PopupState::AuthSecretInput { .. } => {
                return Self::handle_auth_secret_input(app, key, tx).await;
            }
            PopupState::AuthUrl { .. } => {
                return Self::handle_auth_url_popup(app, key).await;
            }
            PopupState::ResumePicker(_) => {
                return Self::handle_resume_picker(app, key, tx).await;
            }
            PopupState::SkillPicker(_) => {
                return Self::handle_skill_picker(app, key, tx).await;
            }
            PopupState::QuestionInput { .. } => {
                return Self::handle_question_input(app, key, tx).await;
            }
            PopupState::DiffViewer { .. } => {
                Self::handle_diff_viewer(app, key);
                return false;
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
        let palette_context = app.palette_context();
        if app.input_bar.palette_open(&palette_context) {
            match key.code {
                KeyCode::Up => {
                    app.input_bar.palette_up();
                    return false;
                }
                KeyCode::Down => {
                    app.input_bar.palette_down(&palette_context);
                    return false;
                }
                KeyCode::Tab => {
                    app.input_bar.palette_down(&palette_context);
                    return false;
                }
                KeyCode::Esc => {
                    app.input_bar.dismiss_completions();
                    return false;
                }
                KeyCode::Enter => {
                    if let Some(value) = app.input_bar.palette_select(&palette_context) {
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
                    if key.modifiers.contains(KeyModifiers::ALT) {
                        for agent in &mut app.active_agents {
                            if app.log_filter.is_none()
                                || app.log_filter.as_deref() == Some(&agent.name)
                                || agent.name.starts_with(&format!(
                                    "{}:",
                                    app.log_filter.as_deref().unwrap_or_default()
                                ))
                            {
                                match agent.status {
                                    crate::tui::widgets::agent_panel::AgentRunStatus::Running => {
                                        agent.scroll_offset = agent.scroll_offset.saturating_add(5)
                                    }
                                    _ => {
                                        agent.scroll_offset = agent.scroll_offset.saturating_sub(5)
                                    }
                                }
                            }
                        }
                    } else {
                        app.input_bar.history_up();
                    }
                    return false;
                }
                KeyCode::Down => {
                    if key.modifiers.contains(KeyModifiers::ALT) {
                        for agent in &mut app.active_agents {
                            if app.log_filter.is_none()
                                || app.log_filter.as_deref() == Some(&agent.name)
                                || agent.name.starts_with(&format!(
                                    "{}:",
                                    app.log_filter.as_deref().unwrap_or_default()
                                ))
                            {
                                match agent.status {
                                    crate::tui::widgets::agent_panel::AgentRunStatus::Running => {
                                        agent.scroll_offset = agent.scroll_offset.saturating_sub(5)
                                    }
                                    _ => {
                                        agent.scroll_offset = agent.scroll_offset.saturating_add(5)
                                    }
                                }
                            }
                        }
                    } else {
                        app.input_bar.history_down();
                    }
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
                KeyCode::PageUp => {
                    for agent in &mut app.active_agents {
                        if app.log_filter.is_none()
                            || app.log_filter.as_deref() == Some(&agent.name)
                            || agent.name.starts_with(&format!(
                                "{}:",
                                app.log_filter.as_deref().unwrap_or_default()
                            ))
                        {
                            match agent.status {
                                crate::tui::widgets::agent_panel::AgentRunStatus::Running => {
                                    agent.scroll_offset = agent.scroll_offset.saturating_add(15)
                                }
                                _ => agent.scroll_offset = agent.scroll_offset.saturating_sub(15),
                            }
                        }
                    }
                    return false;
                }
                KeyCode::PageDown => {
                    for agent in &mut app.active_agents {
                        if app.log_filter.is_none()
                            || app.log_filter.as_deref() == Some(&agent.name)
                            || agent.name.starts_with(&format!(
                                "{}:",
                                app.log_filter.as_deref().unwrap_or_default()
                            ))
                        {
                            match agent.status {
                                crate::tui::widgets::agent_panel::AgentRunStatus::Running => {
                                    agent.scroll_offset = agent.scroll_offset.saturating_sub(15)
                                }
                                _ => agent.scroll_offset = agent.scroll_offset.saturating_add(15),
                            }
                        }
                    }
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
        matches!(cmd.split_whitespace().next(), Some("/quit" | "/exit"))
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
                    let id_clone = crate::providers::registry::normalize_provider(&id).to_string();
                    let info = crate::providers::registry::builtin(&id_clone);
                    let is_custom = app
                        .config
                        .try_read()
                        .map(|cfg| cfg.custom_providers.contains_key(&id_clone))
                        .unwrap_or(false);
                    let needs_key = info.is_some_and(|info| info.needs_key) || is_custom;

                    if needs_key && !Self::provider_has_credential(&app.config, &id_clone).await {
                        app.popup = PopupState::ApiKeyInput {
                            provider: id_clone,
                            input: tui_input::Input::default(),
                        };
                        return false;
                    }

                    app.popup = PopupState::None;
                    Self::save_provider_choice(app, tx, id_clone.clone()).await;
                    Self::fetch_models_for_provider(app, tx, id_clone).await;
                }
            }
            KeyCode::Char(c) => state.push_search(c),
            _ => {}
        }
        false
    }

    async fn handle_connect_provider_picker(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        _tx: &TuiSender,
    ) -> bool {
        let PopupState::ConnectProviderPicker(ref mut state) = app.popup else {
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
                if let Some(provider) = state.selected_id() {
                    let methods = crate::auth::methods_for_provider(&provider);
                    app.popup = PopupState::AuthMethodPicker {
                        provider: provider.clone(),
                        picker: auth_method_picker(&provider, &methods),
                    };
                }
            }
            KeyCode::Char(c) => state.push_search(c),
            _ => {}
        }
        false
    }

    async fn handle_auth_method_picker(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::AuthMethodPicker {
            ref provider,
            ref mut picker,
        } = app.popup
        else {
            return false;
        };
        let provider = provider.clone();

        match key.code {
            KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Up => picker.move_up(),
            KeyCode::Down => picker.move_down(),
            KeyCode::Backspace => picker.pop_search(),
            KeyCode::Enter => {
                if let Some(method_id) = picker.selected_id() {
                    if let Some(method) = crate::auth::method_by_id(&provider, &method_id) {
                        if let Some(message) = crate::auth::connect_blocker(&provider, &method_id) {
                            app.popup = PopupState::None;
                            let _ = tx.send(TuiEvent::Error {
                                agent: "connect".to_string(),
                                message: message.to_string(),
                            });
                            return false;
                        }

                        if method.requires_secret {
                            app.popup = if method.id == "api_key" {
                                PopupState::ApiKeyInput {
                                    provider,
                                    input: tui_input::Input::default(),
                                }
                            } else {
                                PopupState::AuthSecretInput {
                                    provider,
                                    method_id,
                                    input: tui_input::Input::default(),
                                }
                            };
                            return false;
                        }

                        app.popup = PopupState::None;
                        match method.id {
                            "local" => {
                                Self::save_provider_choice(app, tx, provider.clone()).await;
                                Self::fetch_models_for_provider(app, tx, provider.clone()).await;
                            }
                            "google_adc" | "aws_profile" | "gitlab_oauth" => {
                                match crate::auth::record_from_secret(
                                    &provider,
                                    method.id,
                                    String::new(),
                                ) {
                                    Ok(record) => {
                                        let mut store =
                                            crate::auth::AuthStore::load().unwrap_or_default();
                                        store.set(record);
                                        if let Err(e) = store.save() {
                                            let _ = tx.send(TuiEvent::Error {
                                                agent: "connect".to_string(),
                                                message: e.to_string(),
                                            });
                                        } else {
                                            Self::save_provider_choice(app, tx, provider.clone())
                                                .await;
                                            Self::fetch_models_for_provider(
                                                app,
                                                tx,
                                                provider.clone(),
                                            )
                                            .await;
                                            let _ = tx.send(TuiEvent::TokenChunk {
                                                agent: "connect".to_string(),
                                                chunk: format!(
                                                    "  ✓ {} auth method saved ({})",
                                                    provider, method.label
                                                ),
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        let _ = tx.send(TuiEvent::Error {
                                            agent: "connect".to_string(),
                                            message: e.to_string(),
                                        });
                                    }
                                }
                            }
                            "chatgpt_browser" => {
                                let tx2 = tx.clone();
                                let config = Arc::clone(&app.config);
                                tokio::spawn(async move {
                                    match crate::providers::custom_http::chatgpt_browser_auth_with_url(
                                        |url| {
                                            let _ = tx2.send(TuiEvent::AuthUrl {
                                                provider: "openai_chatgpt".to_string(),
                                                url: url.to_string(),
                                                message: "Open this URL in your browser to connect ChatGPT Plus/Pro.".to_string(),
                                            });
                                            Ok(())
                                        },
                                    )
                                    .await
                                    {
                                        Ok(record) => {
                                            let mut store =
                                                crate::auth::AuthStore::load().unwrap_or_default();
                                            store.set(record);
                                            match store.save() {
                                                Ok(()) => {
                                                    let mut cfg = config.write().await;
                                                    sync_models_for_provider(
                                                        &mut cfg,
                                                        "openai_chatgpt",
                                                    );
                                                    cfg.set_provider("openai_chatgpt".to_string());
                                                    let _ = cfg.save();
                                                    drop(cfg);
                                                    Self::spawn_model_fetch(
                                                        Arc::clone(&config),
                                                        tx2.clone(),
                                                        "openai_chatgpt".to_string(),
                                                    );
                                                    let _ = tx2.send(TuiEvent::AuthComplete {
                                                        provider: "openai_chatgpt".to_string(),
                                                        message: "ChatGPT Plus/Pro connected"
                                                            .to_string(),
                                                    });
                                                    let _ = tx2.send(TuiEvent::TokenChunk {
                                                        agent: "connect".to_string(),
                                                        chunk: "  ✓ ChatGPT Plus/Pro connected"
                                                            .to_string(),
                                                    });
                                                }
                                                Err(e) => {
                                                    let _ = tx2.send(TuiEvent::Error {
                                                        agent: "connect".to_string(),
                                                        message: e.to_string(),
                                                    });
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx2.send(TuiEvent::Error {
                                                agent: "connect".to_string(),
                                                message: e.to_string(),
                                            });
                                        }
                                    }
                                });
                            }
                            "github_device" => {
                                let tx2 = tx.clone();
                                let config = Arc::clone(&app.config);
                                let provider = provider.clone();
                                tokio::spawn(async move {
                                    let _ = tx2.send(TuiEvent::TokenChunk {
                                        agent: "connect".to_string(),
                                        chunk: "  Open the GitHub device URL printed in the terminal to finish Copilot login.".to_string(),
                                    });
                                    match crate::auth::connect_github_copilot_device().await {
                                        Ok(record) => {
                                            let mut store =
                                                crate::auth::AuthStore::load().unwrap_or_default();
                                            store.set(record);
                                            match store.save() {
                                                Ok(()) => {
                                                    {
                                                        let mut cfg = config.write().await;
                                                        sync_models_for_provider(
                                                            &mut cfg, &provider,
                                                        );
                                                        cfg.set_provider(provider.clone());
                                                        let _ = cfg.save();
                                                    }
                                                    Self::spawn_model_fetch(
                                                        Arc::clone(&config),
                                                        tx2.clone(),
                                                        provider.clone(),
                                                    );
                                                    let _ = tx2.send(TuiEvent::TokenChunk {
                                                        agent: "connect".to_string(),
                                                        chunk: "  ✓ GitHub Copilot connected"
                                                            .to_string(),
                                                    });
                                                }
                                                Err(e) => {
                                                    let _ = tx2.send(TuiEvent::Error {
                                                        agent: "connect".to_string(),
                                                        message: e.to_string(),
                                                    });
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx2.send(TuiEvent::Error {
                                                agent: "connect".to_string(),
                                                message: e.to_string(),
                                            });
                                        }
                                    }
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            KeyCode::Char(c) => picker.push_search(c),
            _ => {}
        }
        false
    }

    async fn handle_auth_url_popup(app: &mut App, key: &crossterm::event::KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                app.popup = PopupState::None;
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                if let PopupState::AuthUrl { url, copied, .. } = &mut app.popup {
                    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(url.clone())) {
                        Ok(()) => {
                            *copied = true;
                            app.logs.push(LogEntry::system("✓ auth URL copied"));
                        }
                        Err(e) => app
                            .logs
                            .push(LogEntry::system(format!("clipboard error: {e}"))),
                    }
                }
            }
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
                    sync_models_for_provider(&mut cfg, &provider);
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
                            sync_models_for_provider(&mut cfg, &provider);
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
                            if crate::providers::registry::builtin(&provider).is_some() {
                                let mut store = crate::auth::AuthStore::load().unwrap_or_default();
                                match crate::auth::record_from_secret(
                                    &provider,
                                    "api_key",
                                    api_key.clone(),
                                )
                                .and_then(|record| {
                                    store.set(record);
                                    store.save()
                                }) {
                                    Ok(()) => {
                                        sync_models_for_provider(&mut cfg, &provider);
                                        cfg.set_provider(provider.clone());
                                        let _ = cfg.save();
                                        let _ = tx.send(TuiEvent::TokenChunk {
                                            agent: "provider".to_string(),
                                            chunk: format!(
                                                "  ✓ provider → {} • API key saved",
                                                provider
                                            ),
                                        });
                                    }
                                    Err(auth_err) => {
                                        let _ = tx.send(TuiEvent::Error {
                                            agent: "provider".to_string(),
                                            message: format!("{e}; auth store error: {auth_err}"),
                                        });
                                    }
                                }
                            } else {
                                let _ = tx.send(TuiEvent::Error {
                                    agent: "provider".to_string(),
                                    message: e.to_string(),
                                });
                            }
                        }
                    }
                    drop(cfg);
                }
                // Kick off background model fetch for the new provider
                let tx2 = tx.clone();
                let prov = crate::providers::registry::normalize_provider(&provider).to_string();
                let config_snapshot = app.config.read().await.clone();
                tokio::spawn(async move {
                    match crate::providers::models::fetch_models_for_config(&prov, &config_snapshot)
                        .await
                    {
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

    async fn handle_auth_secret_input(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::AuthSecretInput {
            ref provider,
            ref method_id,
            ref mut input,
        } = app.popup
        else {
            return false;
        };
        let provider = provider.clone();
        let method_id = method_id.clone();

        match key.code {
            KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Enter => {
                let secret = input.value().trim().to_string();
                app.popup = PopupState::None;
                let mut store = crate::auth::AuthStore::load().unwrap_or_default();
                match crate::auth::record_from_secret(&provider, &method_id, secret).and_then(
                    |record| {
                        store.set(record);
                        store.save()
                    },
                ) {
                    Ok(()) => {
                        if provider == "openai" && method_id.starts_with("chatgpt") {
                            Self::save_provider_choice(app, tx, "openai_chatgpt".to_string()).await;
                            Self::fetch_models_for_provider(app, tx, "openai_chatgpt".to_string())
                                .await;
                        } else {
                            Self::save_provider_choice(app, tx, provider.clone()).await;
                            Self::fetch_models_for_provider(app, tx, provider.clone()).await;
                        }
                        let _ = tx.send(TuiEvent::TokenChunk {
                            agent: "connect".to_string(),
                            chunk: format!("  ✓ {} connected with {}", provider, method_id),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(TuiEvent::Error {
                            agent: "connect".to_string(),
                            message: e.to_string(),
                        });
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Char(_) => {
                use tui_input::backend::crossterm::EventHandler;
                input.handle_event(&crossterm::event::Event::Key(*key));
            }
            _ => {}
        }
        false
    }

    async fn save_provider_choice(app: &mut App, tx: &TuiSender, provider: String) {
        let provider = crate::providers::registry::normalize_provider(&provider).to_string();
        let mut cfg = app.config.write().await;
        sync_models_for_provider(&mut cfg, &provider);
        cfg.set_provider(provider.clone());
        match cfg.save() {
            Ok(()) => {
                let _ = tx.send(TuiEvent::TokenChunk {
                    agent: "provider".to_string(),
                    chunk: format!("  ✓ provider → {} (saved)", provider),
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

    async fn provider_has_credential(config: &Arc<RwLock<Config>>, provider: &str) -> bool {
        let provider = crate::providers::registry::normalize_provider(provider);
        if matches!(provider, "ollama" | "lmstudio") {
            return true;
        }
        if let Ok(store) = crate::auth::AuthStore::load() {
            if provider == "openai_chatgpt"
                && store.record("openai").is_some_and(|record| {
                    matches!(record.method, crate::auth::AuthMethod::OAuth)
                        && (record
                            .access_token
                            .as_deref()
                            .is_some_and(|t| !t.is_empty())
                            || record
                                .refresh_token
                                .as_deref()
                                .is_some_and(|t| !t.is_empty()))
                })
            {
                return true;
            }
            if provider == "openai"
                && store.record(provider).is_some_and(|record| {
                    matches!(
                        record.method,
                        crate::auth::AuthMethod::ApiKey | crate::auth::AuthMethod::Pat
                    )
                })
            {
                return true;
            }
            if provider != "openai" && store.record(provider).is_some() {
                return true;
            }
        }
        let cfg = config.read().await;
        if cfg.custom_providers.contains_key(provider) {
            return cfg.custom_providers.get(provider).is_some_and(|custom| {
                custom.api_key.as_deref().is_some_and(|key| !key.is_empty())
                    || custom
                        .api_key_env
                        .as_deref()
                        .is_some_and(|env| std::env::var(env).is_ok_and(|v| !v.is_empty()))
            });
        }
        if cfg.get_api_key(provider).is_some_and(|key| !key.is_empty()) {
            return true;
        }
        crate::providers::registry::builtin(provider)
            .and_then(|info| info.env_var)
            .is_some_and(|env| std::env::var(env).is_ok_and(|value| !value.is_empty()))
    }

    async fn fetch_models_for_provider(app: &App, tx: &TuiSender, provider: String) {
        let provider = crate::providers::registry::normalize_provider(&provider).to_string();
        Self::spawn_model_fetch(Arc::clone(&app.config), tx.clone(), provider);
    }

    fn spawn_model_fetch(config: Arc<RwLock<Config>>, tx: TuiSender, provider: String) {
        let provider = crate::providers::registry::normalize_provider(&provider).to_string();
        tokio::spawn(async move {
            let config_snapshot = config.read().await.clone();
            match crate::providers::models::fetch_models_for_config(&provider, &config_snapshot)
                .await
            {
                Ok(models) => {
                    let _ = tx.send(TuiEvent::ModelsLoaded { provider, models });
                }
                Err(e) => {
                    let _ = tx.send(TuiEvent::Error {
                        agent: "models".to_string(),
                        message: format!("model fetch failed: {e}"),
                    });
                }
            }
        });
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
    // Skill picker key handler
    // -------------------------------------------------------------------------

    async fn handle_skill_picker(
        app: &mut App,
        key: &crossterm::event::KeyEvent,
        tx: &TuiSender,
    ) -> bool {
        let PopupState::SkillPicker(ref mut state) = app.popup else {
            return false;
        };

        match key.code {
            KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Up => state.picker.move_up(),
            KeyCode::Down => state.picker.move_down(),
            KeyCode::Backspace => {
                state.picker.pop_search();
                queue_skill_search(state, tx);
            }
            KeyCode::Char(' ') => {
                state.picker.toggle_selected();
                refresh_skill_picker_title(state);
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                state.scope = match state.scope {
                    crate::skills::SkillScope::Global => crate::skills::SkillScope::Project,
                    crate::skills::SkillScope::Project => crate::skills::SkillScope::Global,
                };
                refresh_skill_picker_title(state);
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if let Some(id) = state.picker.selected_id()
                    && let Some(name) = id.strip_prefix("local:")
                {
                    match crate::skills::remove(name, None) {
                        Ok(record) => {
                            state.picker.remove_item(&id);
                            state.original_enabled.remove(name);
                            state.installed_names.remove(name);
                            refresh_skill_picker_title(state);
                            app.logs
                                .push(LogEntry::system(format!("removed skill {}", record.name)));
                        }
                        Err(e) => {
                            let _ = tx.send(TuiEvent::SkillPickerError {
                                message: e.to_string(),
                            });
                        }
                    }
                }
            }
            KeyCode::Enter => {
                let checked = state
                    .picker
                    .checked_ids()
                    .into_iter()
                    .collect::<HashSet<_>>();
                let local_changes = state
                    .original_enabled
                    .iter()
                    .filter_map(|(name, original)| {
                        let id = format!("local:{name}");
                        let desired = checked.contains(&id);
                        (desired != *original).then(|| (name.clone(), desired))
                    })
                    .collect::<Vec<_>>();
                let remote_installs = checked
                    .iter()
                    .filter_map(|id| id.strip_prefix("remote:"))
                    .filter_map(|id| state.remote_by_id.get(id).cloned())
                    .collect::<Vec<_>>();
                let scope = state.scope;

                app.popup = PopupState::None;
                apply_skill_picker_changes(local_changes, remote_installs, scope, tx.clone());
            }
            KeyCode::Char(c) => {
                state.picker.push_search(c);
                queue_skill_search(state, tx);
            }
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
                            .map(|c| {
                                crate::providers::registry::normalize_provider(&c.provider.default)
                                    .to_string()
                            })
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
                            .map(|c| {
                                crate::providers::registry::normalize_provider(&c.provider.default)
                                    .to_string()
                            })
                            .unwrap_or_else(|_| "ollama".to_string());
                        if let Some(models) = app.model_cache.get(&provider) {
                            *model_list = Some(build_model_list_picker(&provider, models));
                            *is_loading = false;
                        } else {
                            // Not cached yet — fetch in background
                            *is_loading = true;
                            let tx2 = tx.clone();
                            let prov = provider.clone();
                            let config_snapshot = app.config.read().await.clone();
                            tokio::spawn(async move {
                                match crate::providers::models::fetch_models_for_config(
                                    &prov,
                                    &config_snapshot,
                                )
                                .await
                                {
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
                app.logs
                    .push(LogEntry::system(format!("answer: {}", answer)));
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

    fn handle_diff_viewer(app: &mut App, key: &crossterm::event::KeyEvent) {
        let PopupState::DiffViewer {
            diffs,
            cursor,
            scroll_offset,
        } = &app.popup
        else {
            return;
        };
        let total = diffs.len();
        let cur = *cursor;
        let scroll = *scroll_offset;
        let max_lines = diffs.get(cur).map(|d| d.lines.len()).unwrap_or(0);

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                app.popup = PopupState::None;
            }
            KeyCode::Char('n') => {
                if cur + 1 < total {
                    app.popup = PopupState::DiffViewer {
                        diffs: std::mem::take(match &mut app.popup {
                            PopupState::DiffViewer { diffs, .. } => diffs,
                            _ => unreachable!(),
                        }),
                        cursor: cur + 1,
                        scroll_offset: 0,
                    };
                } else {
                    app.popup = PopupState::None;
                }
            }
            KeyCode::Char('p') if cur > 0 => {
                app.popup = PopupState::DiffViewer {
                    diffs: std::mem::take(match &mut app.popup {
                        PopupState::DiffViewer { diffs, .. } => diffs,
                        _ => unreachable!(),
                    }),
                    cursor: cur - 1,
                    scroll_offset: 0,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let PopupState::DiffViewer { scroll_offset, .. } = &mut app.popup {
                    *scroll_offset = scroll.saturating_add(1).min(max_lines.saturating_sub(1));
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let PopupState::DiffViewer { scroll_offset, .. } = &mut app.popup {
                    *scroll_offset = scroll.saturating_sub(1);
                }
            }
            KeyCode::PageDown => {
                if let PopupState::DiffViewer { scroll_offset, .. } = &mut app.popup {
                    *scroll_offset = scroll.saturating_add(20).min(max_lines.saturating_sub(1));
                }
            }
            KeyCode::PageUp => {
                if let PopupState::DiffViewer { scroll_offset, .. } = &mut app.popup {
                    *scroll_offset = scroll.saturating_sub(20);
                }
            }
            _ => {}
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

fn is_control_agent(agent: &str) -> bool {
    matches!(
        agent,
        "apikey"
            | "auth"
            | "connect"
            | "model"
            | "models"
            | "orchestrator"
            | "provider"
            | "repl"
            | "resume"
            | "skill"
            | "update"
    )
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

fn build_skill_picker(
    remote_skills: Vec<crate::skills::RemoteSkill>,
    loading: bool,
) -> SkillPickerState {
    use crate::tui::widgets::picker::{PickerGroup, PickerItem};

    let installed = crate::skills::list().unwrap_or_default();
    let scope = default_skill_scope_for_ui();
    let mut original_enabled = HashMap::new();
    let mut installed_names = HashSet::new();
    let installed_items = installed
        .into_iter()
        .map(|record| {
            original_enabled.insert(record.name.clone(), record.enabled);
            installed_names.insert(record.name.clone());
            PickerItem {
                id: format!("local:{}", record.name),
                label: record.name,
                description: Some(format!(
                    "{} [{}]",
                    record.description,
                    skill_scope_label(record.scope)
                )),
                checked: record.enabled,
            }
        })
        .collect::<Vec<_>>();

    let mut state = SkillPickerState {
        picker: PickerState::new(
            skill_picker_title(scope, loading, 0),
            vec![
                PickerGroup {
                    title: "Installed".to_string(),
                    items: installed_items,
                },
                PickerGroup {
                    title: "skills.sh Leaderboard".to_string(),
                    items: Vec::new(),
                },
            ],
        ),
        original_enabled,
        remote_by_id: HashMap::new(),
        installed_names,
        scope,
        loading,
    };
    replace_remote_skill_group(&mut state, "skills.sh Leaderboard", remote_skills);
    state.loading = loading;
    refresh_skill_picker_title(&mut state);
    state
}

fn replace_remote_skill_group(
    state: &mut SkillPickerState,
    title: &str,
    skills: Vec<crate::skills::RemoteSkill>,
) {
    use crate::tui::widgets::picker::{PickerGroup, PickerItem};

    state.remote_by_id.clear();
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for skill in skills {
        if skill.is_duplicate || !seen.insert(skill.id.clone()) {
            continue;
        }
        if state.installed_names.contains(&skill.slug) {
            continue;
        }
        let install_count = if skill.installs == 1 {
            "1 install".to_string()
        } else {
            format!("{} installs", skill.installs)
        };
        let source_kind = if skill.source_type.is_empty() {
            skill.source.clone()
        } else {
            format!("{} · {}", skill.source, skill.source_type)
        };
        items.push(PickerItem {
            id: format!("remote:{}", skill.id),
            label: skill.name.clone(),
            description: Some(format!("{} · {}", source_kind, install_count)),
            checked: false,
        });
        state.remote_by_id.insert(skill.id.clone(), skill);
    }

    state
        .picker
        .groups
        .retain(|group| group.title == "Installed");
    state.picker.groups.push(PickerGroup {
        title: title.to_string(),
        items,
    });
    state.picker.cursor = 0;
    refresh_skill_picker_title(state);
}

fn queue_skill_search(state: &mut SkillPickerState, tx: &TuiSender) {
    let query = state.picker.search.trim().to_string();
    if query.chars().count() < 2 {
        state.loading = false;
        refresh_skill_picker_title(state);
        return;
    }

    state.loading = true;
    refresh_skill_picker_title(state);
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        match crate::skills::search_remote_skills(&query).await {
            Ok(skills) => {
                let _ = tx_clone.send(TuiEvent::SkillSearchLoaded { query, skills });
            }
            Err(e) => {
                let _ = tx_clone.send(TuiEvent::SkillPickerError {
                    message: e.to_string(),
                });
            }
        }
    });
}

fn apply_skill_picker_changes(
    local_changes: Vec<(String, bool)>,
    remote_installs: Vec<crate::skills::RemoteSkill>,
    scope: crate::skills::SkillScope,
    tx: TuiSender,
) {
    tokio::spawn(async move {
        if local_changes.is_empty() && remote_installs.is_empty() {
            return;
        }

        let _ = tx.send(TuiEvent::AgentStarted {
            agent: "skill".to_string(),
        });

        for (name, enabled) in local_changes {
            match crate::skills::set_enabled(&name, None, enabled) {
                Ok(record) => {
                    let action = if enabled { "enabled" } else { "disabled" };
                    let _ = tx.send(TuiEvent::TokenChunk {
                        agent: "skill".to_string(),
                        chunk: format!(
                            "  {} {} [{}]",
                            action,
                            record.name,
                            skill_scope_label(record.scope)
                        ),
                    });
                }
                Err(e) => {
                    let _ = tx.send(TuiEvent::Error {
                        agent: "skill".to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }

        for remote in remote_installs {
            let source = crate::skills::remote_install_display(&remote);
            let _ = tx.send(TuiEvent::TokenChunk {
                agent: "skill".to_string(),
                chunk: format!("  installing {} from {}...", remote.name, source),
            });
            match crate::skills::install_remote_skill(&remote, Some(scope)).await {
                Ok(record) => {
                    let _ = tx.send(TuiEvent::TokenChunk {
                        agent: "skill".to_string(),
                        chunk: format!(
                            "  installed {} [{}]",
                            record.name,
                            skill_scope_label(record.scope)
                        ),
                    });
                }
                Err(e) => {
                    let _ = tx.send(TuiEvent::Error {
                        agent: "skill".to_string(),
                        message: format!("failed to install {}: {}", remote.name, e),
                    });
                }
            }
        }

        let _ = tx.send(TuiEvent::AgentDone {
            agent: "skill".to_string(),
        });
    });
}

fn refresh_skill_picker_title(state: &mut SkillPickerState) {
    state.picker.title =
        skill_picker_title(state.scope, state.loading, state.picker.checked_count());
}

fn skill_picker_title(scope: crate::skills::SkillScope, loading: bool, selected: usize) -> String {
    let loading = if loading { " · loading" } else { "" };
    let selected = if selected == 1 {
        " · 1 selected".to_string()
    } else if selected > 1 {
        format!(" · {} selected", selected)
    } else {
        String::new()
    };
    format!(
        "Skills [{}]{}{}  space toggle · enter apply · d remove · g scope · esc cancel",
        skill_scope_label(scope),
        loading,
        selected
    )
}

fn default_skill_scope_for_ui() -> crate::skills::SkillScope {
    if std::env::current_dir()
        .map(|cwd| cwd.join(".git").exists() || cwd.join("Cargo.toml").exists())
        .unwrap_or(false)
    {
        crate::skills::SkillScope::Project
    } else {
        crate::skills::SkillScope::Global
    }
}

fn skill_scope_label(scope: crate::skills::SkillScope) -> &'static str {
    match scope {
        crate::skills::SkillScope::Global => "global",
        crate::skills::SkillScope::Project => "project",
    }
}

// ---------------------------------------------------------------------------
// Provider-prefix helpers
// ---------------------------------------------------------------------------

fn sync_models_for_provider(config: &mut Config, provider: &str) {
    crate::providers::models::apply_provider_defaults(config, provider);
}

fn qualify_model_string(model: &str, provider: &str) -> String {
    crate::providers::models::qualify_model_string(model, provider)
}

// ---------------------------------------------------------------------------
// API key input overlay
// ---------------------------------------------------------------------------

fn draw_api_key_overlay(frame: &mut Frame, provider: &str, input: &tui_input::Input) {
    use ratatui::layout::{Constraint, Direction, Layout};

    // Build a fixed-height (11 rows), 50%-wide rect centered on screen.
    let screen = frame.area();
    const POPUP_H: u16 = 12;
    const POPUP_W_PCT: u16 = 50;
    let popup_w = screen.width * POPUP_W_PCT / 100;
    let popup_x = screen.x + (screen.width.saturating_sub(popup_w)) / 2;
    let popup_y = screen.y + screen.height.saturating_sub(POPUP_H) / 2;
    let area = Rect::new(popup_x, popup_y, popup_w, POPUP_H.min(screen.height));

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(
            format!(" 🔑  API Key — {provider} "),
            THEME.title_style(),
        ))
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
            Constraint::Length(1), // provider-specific note
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
    let note = if provider == "openai" {
        " ChatGPT Plus/Pro is not an API key; use an OpenAI Platform key."
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(note).style(Style::default().fg(THEME.warning)),
        chunks[2],
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
        chunks[4],
    );

    // Footer
    frame.render_widget(
        Paragraph::new(" Enter to save  •  Esc to cancel").style(Style::default().fg(THEME.muted)),
        chunks[6],
    );
}

// ---------------------------------------------------------------------------
// Loading overlay
// ---------------------------------------------------------------------------

fn draw_loading_overlay(frame: &mut Frame) {
    draw_loading_overlay_with(
        frame,
        " Loading models… ",
        "  Fetching model list from provider…",
    );
}

fn draw_loading_overlay_with(frame: &mut Frame, title: &str, body: &str) {
    use crate::tui::widgets::picker;
    let area = picker::centered_rect(40, 12, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(Span::styled(title.to_string(), THEME.title_style()))
        .borders(Borders::ALL)
        .border_style(THEME.border_style())
        .style(Style::default().bg(THEME.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(body.to_string()).style(Style::default().fg(THEME.muted)),
        inner,
    );
}

// ---------------------------------------------------------------------------
// Authorization URL overlay
// ---------------------------------------------------------------------------

fn draw_auth_url_overlay(
    frame: &mut Frame,
    provider: &str,
    url: &str,
    message: &str,
    copied: bool,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Wrap;

    let screen = frame.area();
    let popup_w = screen.width.saturating_mul(68) / 100;
    let popup_h = 16.min(screen.height);
    let popup_x = screen.x + (screen.width.saturating_sub(popup_w)) / 2;
    let popup_y = screen.y + screen.height.saturating_sub(popup_h) / 2;
    let area = Rect::new(popup_x, popup_y, popup_w.max(40), popup_h);

    frame.render_widget(Clear, area);

    let title = format!(" Connect {provider} ");
    let block = Block::default()
        .title(Span::styled(title, THEME.title_style()))
        .borders(Borders::ALL)
        .border_style(THEME.active_border_style())
        .style(Style::default().bg(THEME.bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(format!(" {message}"))
            .style(Style::default().fg(THEME.text))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );

    let copy_status = if copied {
        " URL copied to clipboard"
    } else {
        " Press c to copy the URL"
    };
    frame.render_widget(
        Paragraph::new(copy_status).style(Style::default().fg(THEME.success)),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new(format!(" {url}"))
            .style(Style::default().fg(THEME.accent))
            .wrap(Wrap { trim: false }),
        chunks[3],
    );

    frame.render_widget(
        Paragraph::new(" Waiting for browser authorization...")
            .style(Style::default().fg(THEME.muted)),
        chunks[4],
    );
    frame.render_widget(
        Paragraph::new(" c copy  •  enter/esc close").style(Style::default().fg(THEME.muted)),
        chunks[5],
    );
}

// ---------------------------------------------------------------------------
// Question input overlay
// ---------------------------------------------------------------------------

fn draw_question_overlay(frame: &mut Frame, agent: &str, question: &str, input: &tui_input::Input) {
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
        Paragraph::new(" Enter to confirm  •  Esc to skip").style(Style::default().fg(THEME.muted)),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::{qualify_model_string, sync_models_for_provider};
    use crate::config::Config;

    #[test]
    fn qualify_model_string_preserves_current_provider_prefix() {
        assert_eq!(
            qualify_model_string("chatgpt/gpt-5.3-codex", "openai_chatgpt"),
            "openai_chatgpt/gpt-5.3-codex"
        );
    }

    #[test]
    fn qualify_model_string_prefixes_aggregator_model_ids() {
        assert_eq!(
            qualify_model_string("google/gemini-2.5-pro", "openrouter"),
            "openrouter/google/gemini-2.5-pro"
        );
        assert_eq!(
            qualify_model_string("openai/gpt-4.1", "vercel_ai_gateway"),
            "vercel_ai_gateway/openai/gpt-4.1"
        );
    }

    #[test]
    fn qualify_model_string_prefixes_slashy_native_model_ids() {
        assert_eq!(
            qualify_model_string(
                "accounts/fireworks/models/qwen3-coder-480b-a35b-instruct",
                "fireworks"
            ),
            "fireworks/accounts/fireworks/models/qwen3-coder-480b-a35b-instruct"
        );
    }

    #[test]
    fn sync_models_for_provider_rewrites_uniform_models() {
        let mut config = Config::default();
        config.models.assistant = "lmstudio/qwen2.5-coder:32b".to_string();
        config.models.ceo = "lmstudio/qwen2.5-coder:32b".to_string();
        config.models.pm = "lmstudio/qwen2.5-coder:32b".to_string();
        config.models.tech_lead = "lmstudio/qwen2.5-coder:32b".to_string();
        config.models.developer = "lmstudio/qwen2.5-coder:32b".to_string();
        config.models.qa = "lmstudio/qwen2.5-coder:32b".to_string();
        config.models.devops = "lmstudio/qwen2.5-coder:32b".to_string();

        sync_models_for_provider(&mut config, "openai_chatgpt");

        assert!(config.models.assistant.starts_with("openai_chatgpt/"));
        assert!(config.models.developer.starts_with("openai_chatgpt/"));
    }

    #[test]
    fn sync_models_for_provider_rewrites_all_roles_even_when_mixed() {
        let mut config = Config::default();
        config.models.ceo = "openrouter/openai/gpt-4.1".to_string();
        config.models.developer = "github_copilot/gpt-4.1".to_string();
        config.models.assistant = "lmstudio/qwen2.5-coder:32b".to_string();

        sync_models_for_provider(&mut config, "openai_chatgpt");

        assert!(config.models.ceo.starts_with("openai_chatgpt/"));
        assert!(config.models.developer.starts_with("openai_chatgpt/"));
        assert!(config.models.assistant.starts_with("openai_chatgpt/"));
    }
}
