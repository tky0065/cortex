use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::agent_bus::{AgentBus, AgentDirective};
use crate::config::Config;
use crate::orchestrator::Orchestrator;
use crate::tui::events::{TuiEvent, TuiSender};
use crate::workflows::{self, ExecutionMode};

// ASSISTANT_PREAMBLE is the system prompt for the agentic assistant loop.
// It lives in assistant.rs and is re-exported here for reference in tests / help text.
#[allow(dead_code)]
const ASSISTANT_PREAMBLE: &str = crate::assistant::PREAMBLE;

/// Information about a workflow session for history tracking.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    /// Unique identifier for the session.
    pub id: String,
    /// Name of the workflow (dev, marketing, prospecting).
    pub workflow: String,
    /// Original user prompt/idea.
    pub idea: String,
    /// Project directory where the workflow ran.
    pub directory: PathBuf,
    /// When the session was started.
    pub timestamp: DateTime<Utc>,
    /// Current status of the session.
    pub status: SessionStatus,
    /// Optional git hash if available.
    pub git_hash: Option<String>,
}

/// Status of a workflow session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    /// Session is currently running.
    Running,
    /// Session completed successfully.
    Completed,
    /// Session was interrupted/aborted.
    Interrupted,
    /// Session failed with an error.
    Failed,
}

/// Shared state for the currently-running workflow, if any.
#[derive(Clone, Default)]
pub struct ReplState {
    /// Cancel token for the running workflow (if any).
    pub cancel: Arc<Mutex<Option<CancellationToken>>>,
    /// Resume sender — calling `send(())` unblocks the next interactive pause.
    pub resume_tx: Arc<Mutex<Option<Arc<tokio::sync::mpsc::Sender<()>>>>>,
    /// Answer sender — calling `send(text)` delivers a user answer to a waiting agent.
    pub answer_tx: Arc<Mutex<Option<Arc<tokio::sync::mpsc::Sender<String>>>>>,
    /// Conversation history for free-form chat mode (user + assistant messages).
    pub chat_history: Arc<Mutex<Vec<rig::completion::Message>>>,
    /// History of past workflow sessions.
    pub session_history: Arc<StdMutex<Vec<SessionInfo>>>,
    /// Directory where session history is stored.
    pub history_dir: PathBuf,
    /// The active AgentBus for the currently running workflow (set by the Orchestrator).
    pub agent_bus: Arc<RwLock<Option<Arc<AgentBus>>>>,
    /// Current execution mode (Normal/Plan/Auto/Review).
    pub execution_mode: Arc<Mutex<ExecutionMode>>,
    /// Original user message pending plan approval (chat mode only).
    pub pending_chat_message: Arc<Mutex<Option<String>>>,
}

impl ReplState {
    pub fn new() -> Self {
        let mut state = Self {
            execution_mode: Arc::new(Mutex::new(ExecutionMode::default())),
            pending_chat_message: Arc::new(Mutex::new(None)),
            ..Self::default()
        };
        // Initialize history directory
        if let Some(mut home) = dirs::home_dir() {
            home.push(".cortex");
            let _ = std::fs::create_dir_all(&home);
            state.history_dir = home;
        }
        // Load existing session history
        let _ = state.load_history();
        state
    }

    /// Load session history from disk.
    fn load_history(&mut self) -> Result<()> {
        let history_path = self.history_dir.join("sessions.json");
        if !history_path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(history_path)?;
        let sessions: Vec<SessionInfo> = serde_json::from_str(&content)?;
        *self.session_history.lock().unwrap() = sessions;
        Ok(())
    }

    /// Save session history to disk.
    fn save_history(&self) -> Result<()> {
        let history_path = self.history_dir.join("sessions.json");
        let sessions = self.session_history.lock().unwrap();
        let content = serde_json::to_string_pretty(&*sessions)?;
        std::fs::write(history_path, content)?;
        Ok(())
    }

    /// Add a new session to the history.
    pub fn add_session(&self, session: SessionInfo) -> Result<()> {
        let mut history = self.session_history.lock().unwrap();
        history.push(session);
        // Keep only the last 50 sessions to prevent unbounded growth.
        if history.len() > 50 {
            let excess = history.len() - 50;
            history.drain(0..excess);
        }
        // Persist to disk.
        drop(history);
        self.save_history()?;
        Ok(())
    }
}

/// Dispatches a command or free-form message entered in the TUI input bar.
/// - Input starting with `/` → slash command
/// - Everything else        → conversational chat with the assistant agent
///
/// Returns `true` if the application should quit.
pub async fn dispatch(
    cmd: &str,
    tx: &TuiSender,
    config: Arc<RwLock<Config>>,
    state: Arc<ReplState>,
) -> Result<bool> {
    let trimmed = cmd.trim();

    // Route free-form input (not a slash command) to the chat assistant
    if !trimmed.starts_with('/') {
        return chat_message(trimmed, tx, config, state).await;
    }

    let (command, rest) = trimmed
        .split_once(char::is_whitespace)
        .map(|(c, r)| (c, r.trim()))
        .unwrap_or((trimmed, ""));

    match command {
        "/quit" | "/exit" => return Ok(true),

        "/help" => {
            let lines = vec![
                "  /start <workflow> \"<idea>\"  — launch a workflow",
                "  /run   <workflow> \"<prompt>\" — alias for /start",
                "  /resume <project-dir>         — resume an interrupted workflow",
                "  /init [--force]               — scan this project and generate/update AGENTS.md",
                "  /mode [normal|plan|auto|review] — show or set execution mode (Shift+Tab to cycle)",
                "  /approve                      — approve a plan and start execution (Plan mode)",
                "  /status                       — show current workflow status",
                "  /abort                        — cancel the running workflow",
                "  /continue                     — resume an interactive pause",
                "  /agents                       — show status of all agents in the current workflow",
                "  /agent list                          — list all custom agents (~/.cortex/agents/)",
                "  /agent create <name> [desc]          — generate a custom agent with Cortex AI",
                "  /agent <name> \"<directive>\"         — inject a directive to a running agent",
                "  /workflow list                       — list built-in and custom workflows",
                "  /workflow create <name> [desc]       — generate a custom workflow with Cortex AI",
                "  /config                       — print active configuration",
                "  /model [<role> <model>]       — show or change a role's model",
                "  /provider [<name>]            — show or change the default provider",
                "  /connect [provider method]    — connect provider auth",
                "  /apikey <provider> <key>      — set an API key",
                "  /websearch [enable|disable]   — toggle web search context injection for all agents",
                "  /skill                        — browse, install, enable, disable, and remove skills",
                "  /update [check|<version>]      — check for or install Cortex updates",
                "  /focus <agent>                — show only logs for one agent",
                "  /clear                        — clear visible logs",
                "  /logs                         — toggle log panel focus",
                "  /quit                         — exit cortex",
            ];
            for line in lines {
                send(
                    tx,
                    TuiEvent::AgentStarted {
                        agent: "help".to_string(),
                    },
                );
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "help".to_string(),
                        chunk: line.to_string(),
                    },
                );
            }
        }

        "/config" => {
            let cfg = config.read().await;
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  provider: {}", cfg.provider.default),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  ceo:        {}", cfg.models.ceo),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  pm:         {}", cfg.models.pm),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  tech_lead:  {}", cfg.models.tech_lead),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  developer:  {}", cfg.models.developer),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  qa:         {}", cfg.models.qa),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  devops:     {}", cfg.models.devops),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  assistant:  {}", cfg.models.assistant),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!(
                        "  max_parallel_workers: {}",
                        cfg.limits.max_parallel_workers
                    ),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  max_qa_iterations: {}", cfg.limits.max_qa_iterations),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  web_search_enabled: {}", cfg.tools.web_search_enabled),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!("  skills_enabled: {}", cfg.tools.skills_enabled),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!(
                        "  max_skill_context_chars: {}",
                        cfg.tools.max_skill_context_chars
                    ),
                },
            );
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "config".to_string(),
                    chunk: format!(
                        "  web_search_key: {}",
                        if cfg.api_keys.web_search.is_some() {
                            "set"
                        } else {
                            "not set"
                        }
                    ),
                },
            );
        }

        "/websearch" => {
            let arg = rest.trim();
            if arg.is_empty() {
                let cfg = config.read().await;
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "websearch".to_string(),
                        chunk: format!(
                            "  web_search_enabled: {}  (use /websearch enable|disable to change)",
                            cfg.tools.web_search_enabled
                        ),
                    },
                );
            } else {
                let enable = match arg {
                    "enable" => Some(true),
                    "disable" => Some(false),
                    _ => {
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "websearch".to_string(),
                                message: "usage: /websearch [enable|disable]".to_string(),
                            },
                        );
                        None
                    }
                };
                if let Some(enabled) = enable {
                    let mut cfg = config.write().await;
                    cfg.set_web_search_enabled(enabled);
                    if let Err(e) = cfg.save() {
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "websearch".to_string(),
                                message: format!("saved in memory but failed to persist: {e}"),
                            },
                        );
                    } else {
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "websearch".to_string(),
                                chunk: format!("  ✓ web_search_enabled → {} (saved)", enabled),
                            },
                        );
                        if enabled && config.read().await.api_keys.web_search.is_none() {
                            send(tx, TuiEvent::TokenChunk {
                                agent: "websearch".to_string(),
                                chunk: "  ⚠ Tip: set your Brave Search API key with /apikey web_search <key>".to_string(),
                            });
                        }
                    }
                }
            }
        }

        "/update" => {
            handle_update_command(rest, tx).await;
        }

        "/init" => {
            let force = rest.split_whitespace().any(|arg| arg == "--force");
            let config_snapshot = Arc::new(config.read().await.clone());
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::project_context::init_current_project(
                    config_snapshot,
                    Some(tx_clone.clone()),
                    force,
                )
                .await
                {
                    send(
                        &tx_clone,
                        TuiEvent::Error {
                            agent: "init".to_string(),
                            message: e.to_string(),
                        },
                    );
                }
            });
        }

        "/skill" | "/skills" => {
            handle_skill_command(rest, tx).await;
        }

        "/apikey" => {
            let (provider, key_str) = rest
                .split_once(char::is_whitespace)
                .map(|(p, k)| (p.trim(), k.trim()))
                .unwrap_or((rest, ""));

            if provider.is_empty() || key_str.is_empty() {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "apikey".to_string(),
                        message: "usage: /apikey <provider> <key>".to_string(),
                    },
                );
                return Ok(false);
            }

            let mut cfg = config.write().await;
            match cfg.set_api_key(provider, key_str.to_string()) {
                Ok(()) => {
                    cfg.apply_api_keys_to_env();
                    if let Err(e) = cfg.save() {
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "apikey".to_string(),
                                message: format!("saved in memory but failed to persist: {e}"),
                            },
                        );
                    } else {
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "apikey".to_string(),
                                chunk: format!("  ✓ {} API key saved", provider),
                            },
                        );
                    }
                }
                Err(e) => {
                    send(
                        tx,
                        TuiEvent::Error {
                            agent: "apikey".to_string(),
                            message: e.to_string(),
                        },
                    );
                }
            }
        }

        "/connect" => {
            handle_connect_command(rest, tx, config).await;
        }

        "/model" => {
            if rest.is_empty() {
                // Open the interactive model picker popup
                send(tx, TuiEvent::OpenModelPicker);
            } else {
                // /model <role> <model-string>
                let (role, model_str) = rest
                    .split_once(char::is_whitespace)
                    .map(|(r, m)| (r.trim(), m.trim()))
                    .unwrap_or((rest, ""));

                if model_str.is_empty() {
                    send(tx, TuiEvent::Error {
                        agent: "model".to_string(),
                        message: "usage: /model <role> <model-string>  (role: ceo/pm/tech_lead/developer/qa/devops/assistant/all)".to_string(),
                    });
                    return Ok(false);
                }

                let mut cfg = config.write().await;
                let set_result = cfg.set_model(role, model_str.to_string());
                let (provider_snap, model_snap) =
                    (cfg.provider.default.clone(), cfg.models.assistant.clone());
                match set_result {
                    Ok(()) => {
                        if let Err(e) = cfg.save() {
                            drop(cfg);
                            send(
                                tx,
                                TuiEvent::Error {
                                    agent: "model".to_string(),
                                    message: format!("saved in memory but failed to persist: {e}"),
                                },
                            );
                        } else {
                            drop(cfg);
                            send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "model".to_string(),
                                    chunk: format!("  ✓ {} → {} (saved)", role, model_str),
                                },
                            );
                            send(
                                tx,
                                TuiEvent::ProviderChanged {
                                    provider: provider_snap,
                                    model: model_snap,
                                },
                            );
                        }
                    }
                    Err(e) => {
                        drop(cfg);
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "model".to_string(),
                                message: e.to_string(),
                            },
                        );
                    }
                }
            }
        }

        "/provider" => {
            if rest.is_empty() {
                // Open the interactive provider picker popup
                send(tx, TuiEvent::OpenProviderPicker);
            } else {
                match set_provider_and_sync_models(rest, &config, tx).await {
                    Ok(SetProviderOutcome {
                        provider, model, ..
                    }) => {
                        let model_suffix = model
                            .as_deref()
                            .map(|model| format!("; model → {model}"))
                            .unwrap_or_default();
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "provider".to_string(),
                                chunk: format!(
                                    "  ✓ provider → {}{} (saved)",
                                    provider, model_suffix
                                ),
                            },
                        );
                        // Update cached display so status bar never shows UNKNOWN.
                        if let Ok(cfg) = config.try_read() {
                            send(
                                tx,
                                TuiEvent::ProviderChanged {
                                    provider: cfg.provider.default.clone(),
                                    model: cfg.models.assistant.clone(),
                                },
                            );
                        }
                    }
                    Err(e) => {
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "provider".to_string(),
                                message: e.to_string(),
                            },
                        );
                    }
                }
            }
        }

        "/mode" => {
            let arg = rest.trim().to_lowercase();
            if arg.is_empty() {
                let mode = state.execution_mode.lock().await;
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "mode".to_string(),
                        chunk: format!(
                            "  mode: {}  (Shift+Tab to cycle, or /mode normal|plan|auto|review)",
                            mode.label()
                        ),
                    },
                );
            } else {
                let new_mode = match arg.as_str() {
                    "normal" => Some(ExecutionMode::Normal),
                    "plan" => Some(ExecutionMode::Plan),
                    "auto" => Some(ExecutionMode::Auto),
                    "review" => Some(ExecutionMode::Review),
                    _ => {
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "mode".to_string(),
                                message: "usage: /mode [normal|plan|auto|review]".to_string(),
                            },
                        );
                        None
                    }
                };
                if let Some(mode) = new_mode {
                    let label = mode.label();
                    *state.execution_mode.lock().await = mode.clone();
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "mode".to_string(),
                            chunk: format!("  ✓ mode → {}", label),
                        },
                    );
                    send(tx, TuiEvent::ModeChanged(mode));
                }
            }
        }

        // /approve is an alias for /continue — used after Plan mode generates PLAN.md
        "/approve" => {
            let resume_guard = state.resume_tx.lock().await;
            if let Some(tx_resume) = resume_guard.as_ref() {
                let _ = tx_resume.try_send(());
                send(tx, TuiEvent::Resume);
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "repl".to_string(),
                        chunk: "  Plan approved — executing workflow…".to_string(),
                    },
                );
            } else {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "repl".to_string(),
                        chunk: "  No plan is pending approval.".to_string(),
                    },
                );
            }
        }

        "/start" | "/run" => {
            if rest.is_empty() {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "repl".to_string(),
                        message: format!("usage: {} <workflow> \"<prompt>\"", command),
                    },
                );
                return Ok(false);
            }

            // Parse: /start dev "my idea" OR /start "my idea" (defaults to dev)
            let (workflow_name, prompt) = parse_workflow_and_prompt(rest);

            // Snapshot current config for this run
            let config_snapshot = Arc::new(config.read().await.clone());

            match workflows::get_workflow(workflow_name) {
                Err(e) => {
                    let hint = if is_known_agent_role(workflow_name) {
                        format!(
                            "{}. '{}' is an agent role, not a workflow. Try: /run dev \"{}\"",
                            e, workflow_name, prompt
                        )
                    } else {
                        e.to_string()
                    };
                    send(
                        tx,
                        TuiEvent::Error {
                            agent: "repl".to_string(),
                            message: hint,
                        },
                    );
                }
                Ok(wf) => {
                    send(
                        tx,
                        TuiEvent::AgentStarted {
                            agent: format!("workflow:{}", workflow_name),
                        },
                    );
                    let tx_clone = tx.clone();
                    let tx_done = tx.clone();
                    let current_mode = state.execution_mode.lock().await.clone();
                    let mut orch =
                        Orchestrator::new(wf, config_snapshot).with_execution_mode(current_mode);

                    // Create session info for tracking
                    let prompt_owned = prompt.to_string();
                    let session_id = Uuid::new_v4().to_string();
                    let session_info = SessionInfo {
                        id: session_id.clone(),
                        workflow: workflow_name.to_string(),
                        idea: prompt_owned.clone(),
                        directory: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                        timestamp: Utc::now(),
                        status: SessionStatus::Running,
                        git_hash: None,
                    };

                    // Register repl_state with the orchestrator so it can use it internally
                    orch = orch.with_repl_state(state.clone());

                    // Store the cancel token and resume sender so /abort and /continue work
                    {
                        let mut cancel_guard = state.cancel.lock().await;
                        *cancel_guard = Some(orch.cancel_token());
                    }
                    {
                        let mut resume_guard = state.resume_tx.lock().await;
                        *resume_guard = Some(orch.resume_sender());
                    }
                    {
                        let mut answer_guard = state.answer_tx.lock().await;
                        *answer_guard = Some(orch.answer_sender());
                    }

                    // Clone state for use inside the spawn
                    let state_for_spawn = Arc::clone(&state);

                    // Spawn so the TUI stays responsive
                    tokio::spawn(async move {
                        // Add session to history when workflow starts
                        let _ = state_for_spawn.add_session(session_info.clone());

                        let result = orch
                            .run_with_sender(prompt_owned.clone(), false, Some(tx_clone))
                            .await;

                        // Notify the TUI of the final outcome
                        match &result {
                            Ok(()) => {
                                let output_dir = if session_info.workflow == "dev" {
                                    session_info.directory.clone()
                                } else {
                                    session_info.directory.join("cortex-output")
                                };
                                let git_hash = std::process::Command::new("git")
                                    .args(["rev-parse", "HEAD"])
                                    .current_dir(&output_dir)
                                    .output()
                                    .ok()
                                    .and_then(|o| String::from_utf8(o.stdout).ok())
                                    .map(|s| s.trim().to_string())
                                    .filter(|s| !s.is_empty());
                                let files = list_output_files(&output_dir);
                                let _ = tx_done.send(TuiEvent::WorkflowComplete {
                                    output_dir: output_dir.to_string_lossy().to_string(),
                                    files,
                                    git_hash: git_hash.clone(),
                                });

                                // Update session history
                                let mut history = state_for_spawn.session_history.lock().unwrap();
                                if let Some(session) =
                                    history.iter_mut().find(|s| s.id == session_id)
                                {
                                    session.status = SessionStatus::Completed;
                                    session.git_hash = git_hash;
                                }
                            }
                            Err(e) => {
                                let _ = tx_done.send(TuiEvent::Error {
                                    agent: "orchestrator".to_string(),
                                    message: e.to_string(),
                                });

                                let mut history = state_for_spawn.session_history.lock().unwrap();
                                if let Some(session) =
                                    history.iter_mut().find(|s| s.id == session_id)
                                {
                                    session.status = SessionStatus::Failed;
                                }
                            }
                        }
                        let _ = state_for_spawn.save_history();
                    });
                }
            }
        }

        "/status" => {
            let running = state.cancel.lock().await.is_some();
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "status".to_string(),
                    chunk: if running {
                        "  Workflow running.".to_string()
                    } else {
                        "  No workflow running.".to_string()
                    },
                },
            );
        }

        "/agents" => {
            let bus_guard = state.agent_bus.read().await;
            if let Some(bus) = bus_guard.as_ref() {
                let all = bus.get_all_statuses().await;
                if all.is_empty() {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "agents".to_string(),
                            chunk: "  No agent activity recorded yet.".to_string(),
                        },
                    );
                } else {
                    for (name, record) in &all {
                        let output_hint = match &record.output {
                            Some(o) => format!(" ({} chars output)", o.len()),
                            None => String::new(),
                        };
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "agents".to_string(),
                                chunk: format!("  {:12} [{}]{}", name, record.status, output_hint),
                            },
                        );
                    }
                }
            } else {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "agents".to_string(),
                        chunk: "  No workflow is currently running.".to_string(),
                    },
                );
            }
        }

        "/agent" => {
            // Subcommands: list, create <name>
            // Fallthrough: /agent <name> "<directive>"
            let first_token = rest.split_whitespace().next().unwrap_or("");
            if first_token == "list" {
                let project_root = std::env::current_dir().ok();
                let agents =
                    crate::agent_loader::AgentLoader::list_agents(project_root.as_deref());
                if agents.is_empty() {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "agent".to_string(),
                            chunk: "  No custom agents found in ~/.cortex/agents/ or .cortex/agents/".to_string(),
                        },
                    );
                } else {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "agent".to_string(),
                            chunk: format!("  {} custom agent(s):", agents.len()),
                        },
                    );
                    for def in agents {
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "agent".to_string(),
                                chunk: format!(
                                    "    {:<20} {} (model: {})",
                                    def.name, def.description, def.model
                                ),
                            },
                        );
                    }
                }
            } else if first_token == "create" {
                // Syntax: /agent create <name> [optional description]
                let tokens: Vec<&str> = rest.splitn(3, char::is_whitespace).collect();
                let name = tokens.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                let description = tokens
                    .get(2)
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty());

                if name.is_empty() {
                    send(
                        tx,
                        TuiEvent::SetInputBar {
                            value: "/agent create ".to_string(),
                        },
                    );
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "agent".to_string(),
                            chunk: "  Type agent name after /agent create".to_string(),
                        },
                    );
                } else {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "agent".to_string(),
                            chunk: format!("  Generating '{name}' agent with Cortex AI..."),
                        },
                    );
                    let model = config.read().await.models.assistant.clone();
                    match ai_generate_agent(&name, description.as_deref(), &model).await {
                        Ok(content) => match save_agent_file(&name, &content) {
                            Ok(path) => {
                                send(
                                    tx,
                                    TuiEvent::TokenChunk {
                                        agent: "agent".to_string(),
                                        chunk: format!("  Created: {}", path.display()),
                                    },
                                );
                                send(tx, TuiEvent::LauncherRefresh);
                            }
                            Err(e) => send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "agent".to_string(),
                                    chunk: format!("  Error saving file: {e}"),
                                },
                            ),
                        },
                        Err(e) => {
                            send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "agent".to_string(),
                                    chunk: format!(
                                        "  Generation failed ({e}), using template..."
                                    ),
                                },
                            );
                            match handle_agent_create(&name, &model) {
                                Ok(path) => {
                                    send(
                                        tx,
                                        TuiEvent::TokenChunk {
                                            agent: "agent".to_string(),
                                            chunk: format!("  Created: {}", path.display()),
                                        },
                                    );
                                    send(tx, TuiEvent::LauncherRefresh);
                                }
                                Err(e2) => send(
                                    tx,
                                    TuiEvent::TokenChunk {
                                        agent: "agent".to_string(),
                                        chunk: format!("  Error: {e2}"),
                                    },
                                ),
                            }
                        }
                    }
                }
            } else if rest.is_empty() {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "agent".to_string(),
                        chunk: "  Usage: /agent list | /agent create <name> | /agent <name> \"<directive>\"".to_string(),
                    },
                );
            } else {
                let (agent_name, instruction) = rest
                    .splitn(2, char::is_whitespace)
                    .collect::<Vec<_>>()
                    .as_slice()
                    .try_into()
                    .map(|[a, b]: &[&str; 2]| {
                        let inst = b.trim().trim_matches('"');
                        (a.to_string(), inst.to_string())
                    })
                    .unwrap_or_else(|_| (rest.to_string(), String::new()));

                if instruction.is_empty() {
                    send(
                        tx,
                        TuiEvent::SetInputBar {
                            value: format!("/agent {} \"", agent_name),
                        },
                    );
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "agent".to_string(),
                            chunk: format!("  Type prompt after /agent {agent_name} \""),
                        },
                    );
                } else {
                    // If no workflow is running and a custom agent def exists → run standalone.
                    let no_workflow = state.cancel.lock().await.is_none();
                    if no_workflow {
                        let project_root = std::env::current_dir().ok();
                        if let Ok(Some(def)) = crate::agent_loader::AgentLoader::load_agent(
                            &agent_name,
                            project_root.as_deref(),
                        ) {
                            let tx2 = tx.clone();
                            let prompt_text = instruction.clone();
                            tokio::spawn(async move {
                                let _ = tx2.send(TuiEvent::AgentStarted {
                                    agent: def.name.clone(),
                                });
                                match crate::providers::complete_chat(
                                    &def.model,
                                    &def.system_prompt,
                                    vec![],
                                    &prompt_text,
                                )
                                .await
                                {
                                    Ok(response) => {
                                        let _ = tx2.send(TuiEvent::TokenChunk {
                                            agent: def.name.clone(),
                                            chunk: response,
                                        });
                                        let _ = tx2.send(TuiEvent::AgentDone {
                                            agent: def.name.clone(),
                                        });
                                    }
                                    Err(e) => {
                                        let _ = tx2.send(TuiEvent::Error {
                                            agent: def.name.clone(),
                                            message: e.to_string(),
                                        });
                                    }
                                }
                            });
                            return Ok(false);
                        }
                    }
                    // Workflow is running → send directive to the agent bus.
                    let bus_guard = state.agent_bus.read().await;
                    if let Some(bus) = bus_guard.as_ref() {
                        match bus.send_directive(AgentDirective {
                            target_agent: agent_name.clone(),
                            instruction: instruction.clone(),
                        }) {
                            Ok(()) => send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "agent".to_string(),
                                    chunk: format!(
                                        "  Directive sent to '{}': {}",
                                        agent_name, instruction
                                    ),
                                },
                            ),
                            Err(e) => send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "agent".to_string(),
                                    chunk: format!("  Failed to send directive: {e}"),
                                },
                            ),
                        }
                    } else {
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "agent".to_string(),
                                chunk: format!(
                                    "  Agent '{}' not found. Use /agent create {0} to create it.",
                                    agent_name
                                ),
                            },
                        );
                    }
                }
            }
        }

        "/workflow" => {
            let first_token = rest.split_whitespace().next().unwrap_or("");
            if first_token == "list" {
                let workflows = crate::workflows::available_workflows_dynamic();
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "workflow".to_string(),
                        chunk: format!("  {} workflow(s):", workflows.len()),
                    },
                );
                for (name, desc) in workflows {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "workflow".to_string(),
                            chunk: format!("    {:<20} {}", name, desc),
                        },
                    );
                }
            } else if first_token == "create" {
                // Syntax: /workflow create <name> [optional description]
                let tokens: Vec<&str> = rest.splitn(3, char::is_whitespace).collect();
                let name = tokens.get(1).map(|s| s.trim()).unwrap_or("").to_string();
                let description = tokens
                    .get(2)
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty());

                if name.is_empty() {
                    send(
                        tx,
                        TuiEvent::SetInputBar {
                            value: "/workflow create ".to_string(),
                        },
                    );
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "workflow".to_string(),
                            chunk: "  Type workflow name after /workflow create".to_string(),
                        },
                    );
                } else {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "workflow".to_string(),
                            chunk: format!("  Generating '{name}' workflow with Cortex AI..."),
                        },
                    );
                    let model = config.read().await.models.assistant.clone();
                    match ai_generate_workflow(&name, description.as_deref(), &model).await {
                        Ok(content) => match save_workflow_file(&name, &content) {
                            Ok(path) => {
                                send(
                                    tx,
                                    TuiEvent::TokenChunk {
                                        agent: "workflow".to_string(),
                                        chunk: format!("  Created: {}", path.display()),
                                    },
                                );
                                // Generate agent files for every step that lacks one.
                                generate_workflow_agents(&content, &model, tx).await;
                                send(tx, TuiEvent::LauncherRefresh);
                            }
                            Err(e) => send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "workflow".to_string(),
                                    chunk: format!("  Error saving file: {e}"),
                                },
                            ),
                        },
                        Err(e) => {
                            send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "workflow".to_string(),
                                    chunk: format!(
                                        "  Generation failed ({e}), using template..."
                                    ),
                                },
                            );
                            match handle_workflow_create(&name) {
                                Ok(path) => {
                                    send(
                                        tx,
                                        TuiEvent::TokenChunk {
                                            agent: "workflow".to_string(),
                                            chunk: format!("  Created: {}", path.display()),
                                        },
                                    );
                                    send(tx, TuiEvent::LauncherRefresh);
                                }
                                Err(e2) => send(
                                    tx,
                                    TuiEvent::TokenChunk {
                                        agent: "workflow".to_string(),
                                        chunk: format!("  Error: {e2}"),
                                    },
                                ),
                            }
                        }
                    }
                }
            } else {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "workflow".to_string(),
                        chunk: "  Usage: /workflow list | /workflow create <name> [description]"
                            .to_string(),
                    },
                );
            }
        }

        "/abort" => {
            let mut cancel_guard = state.cancel.lock().await;
            if let Some(token) = cancel_guard.take() {
                token.cancel();
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "abort".to_string(),
                        chunk: "  Abort signal sent — workflow will stop at the next checkpoint."
                            .to_string(),
                    },
                );

                // Update the last running session to Interrupted
                {
                    let mut history = state.session_history.lock().unwrap();
                    if let Some(session) = history.iter_mut().last()
                        && matches!(session.status, SessionStatus::Running)
                    {
                        session.status = SessionStatus::Interrupted;
                    }
                }
                let _ = state.save_history();
            } else {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "abort".to_string(),
                        chunk: "  No workflow is currently running.".to_string(),
                    },
                );
            }
        }

        "/continue" => {
            let resume_guard = state.resume_tx.lock().await;
            if let Some(tx_resume) = resume_guard.as_ref() {
                let _ = tx_resume.try_send(());
                send(tx, TuiEvent::Resume);
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "repl".to_string(),
                        chunk: "  Resuming workflow…".to_string(),
                    },
                );
            } else {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "repl".to_string(),
                        chunk: "  No workflow is paused.".to_string(),
                    },
                );
            }
        }

        "/logs" => {
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "logs".to_string(),
                    chunk: "  Log panel focus toggled.".to_string(),
                },
            );
        }

        "/clear" => {
            send(tx, TuiEvent::ClearLogs);
        }

        "/focus" => {
            if rest.is_empty() || rest == "all" || rest == "off" {
                send(tx, TuiEvent::SetLogFilter { agent: None });
            } else {
                send(
                    tx,
                    TuiEvent::SetLogFilter {
                        agent: Some(rest.to_string()),
                    },
                );
            }
        }

        "/resume" => {
            if rest.is_empty() {
                // Open the interactive resume picker popup
                send(tx, TuiEvent::OpenResumePicker);
            } else {
                let project_dir = std::path::PathBuf::from(rest);
                if !project_dir.exists() {
                    send(
                        tx,
                        TuiEvent::Error {
                            agent: "repl".to_string(),
                            message: format!("directory does not exist: {}", project_dir.display()),
                        },
                    );
                    return Ok(false);
                }

                let config_snapshot = Arc::new(config.read().await.clone());
                let wf = workflows::get_workflow("dev")?;
                let tx_clone = tx.clone();
                let tx_done = tx.clone();
                let orch = Orchestrator::new(wf, config_snapshot);

                {
                    let mut cancel_guard = state.cancel.lock().await;
                    *cancel_guard = Some(orch.cancel_token());
                }
                {
                    let mut resume_guard = state.resume_tx.lock().await;
                    *resume_guard = Some(orch.resume_sender());
                }
                {
                    let mut answer_guard = state.answer_tx.lock().await;
                    *answer_guard = Some(orch.answer_sender());
                }

                let prompt = format!(
                    "Resume and complete the project in: {}",
                    project_dir.display()
                );
                tokio::spawn(async move {
                    let result = orch
                        .run_with_project_dir(
                            prompt,
                            true,
                            false,
                            Some(tx_clone),
                            Some(project_dir.clone()),
                        )
                        .await;
                    match result {
                        Ok(()) => {
                            let files = list_output_files(&project_dir);
                            let _ = tx_done.send(TuiEvent::WorkflowComplete {
                                output_dir: project_dir.to_string_lossy().to_string(),
                                files,
                                git_hash: None,
                            });
                        }
                        Err(e) => {
                            let _ = tx_done.send(TuiEvent::Error {
                                agent: "orchestrator".to_string(),
                                message: e.to_string(),
                            });
                        }
                    }
                });

                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "repl".to_string(),
                        chunk: format!("  Resuming project at: {}", rest),
                    },
                );
            }
        }

        other => {
            send(
                tx,
                TuiEvent::Error {
                    agent: "repl".to_string(),
                    message: format!("unknown command '{}' — type /help", other),
                },
            );
        }
    }

    Ok(false)
}

fn send(tx: &TuiSender, event: TuiEvent) {
    let _ = tx.send(event);
}

async fn handle_connect_command(rest: &str, tx: &TuiSender, config: Arc<RwLock<Config>>) {
    let args = rest.split_whitespace().collect::<Vec<_>>();
    if args.is_empty() {
        send(tx, TuiEvent::OpenConnectProviderPicker);
        return;
    }

    let provider = args[0];
    let methods = crate::auth::methods_for_provider(provider);
    if methods.is_empty() {
        send(
            tx,
            TuiEvent::Error {
                agent: "connect".to_string(),
                message: format!("unknown provider '{provider}'"),
            },
        );
        return;
    }

    if args.len() == 1 {
        for method in methods {
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "connect".to_string(),
                    chunk: format!(
                        "  {} — {} ({})",
                        method.id, method.label, method.description
                    ),
                },
            );
        }
        return;
    }

    let method_id = args[1];
    let secret = args
        .get(2..)
        .map(|parts| parts.join(" "))
        .unwrap_or_default();

    // If the second argument looks like an API key (not a known auth method),
    // treat it as /apikey <provider> <key> to avoid confusing error messages.
    if crate::auth::method_by_id(provider, method_id).is_none() && looks_like_api_key(method_id) {
        let mut cfg = config.write().await;
        match cfg.set_api_key(provider, method_id.to_string()) {
            Ok(()) => {
                cfg.apply_api_keys_to_env();
                let _ = cfg.save();
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "connect".to_string(),
                        chunk: format!(
                            "  ✓ {} API key saved (tip: use /apikey {} <key> next time)",
                            provider, provider
                        ),
                    },
                );
            }
            Err(e) => {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "connect".to_string(),
                        message: format!("{e}  — use /apikey {provider} <key> to set an API key"),
                    },
                );
            }
        }
        return;
    }

    let Some(method) = crate::auth::method_by_id(provider, method_id) else {
        send(
            tx,
            TuiEvent::Error {
                agent: "connect".to_string(),
                message: format!(
                    "unknown auth method '{method_id}' for provider '{provider}'. \
                     To set an API key use: /apikey {provider} <key>"
                ),
            },
        );
        return;
    };

    if let Some(message) = crate::auth::connect_blocker(provider, method_id) {
        send(
            tx,
            TuiEvent::Error {
                agent: "connect".to_string(),
                message: message.to_string(),
            },
        );
        return;
    }

    let record = match method.id {
        "chatgpt_browser" => {
            match crate::providers::custom_http::chatgpt_browser_auth_with_url(|url| {
                send(
                    tx,
                    TuiEvent::AuthUrl {
                        provider: "openai_chatgpt".to_string(),
                        url: url.to_string(),
                        message: "Open this URL in your browser to connect ChatGPT Plus/Pro."
                            .to_string(),
                    },
                );
                Ok(())
            })
            .await
            {
                Ok(record) => record,
                Err(e) => {
                    send(
                        tx,
                        TuiEvent::Error {
                            agent: "connect".to_string(),
                            message: e.to_string(),
                        },
                    );
                    return;
                }
            }
        }
        "github_device" => match crate::auth::connect_github_copilot_device().await {
            Ok(record) => record,
            Err(e) => {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "connect".to_string(),
                        message: e.to_string(),
                    },
                );
                return;
            }
        },
        _ => match crate::auth::record_from_secret(provider, method_id, secret) {
            Ok(record) => record,
            Err(e) => {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "connect".to_string(),
                        message: e.to_string(),
                    },
                );
                return;
            }
        },
    };

    let selected_provider = if provider == "openai" && method.id.starts_with("chatgpt") {
        "openai_chatgpt"
    } else {
        provider
    };

    let mut store = crate::auth::AuthStore::load().unwrap_or_default();
    store.set(record);
    if let Err(e) = store.save() {
        send(
            tx,
            TuiEvent::Error {
                agent: "connect".to_string(),
                message: e.to_string(),
            },
        );
        return;
    }

    match set_provider_and_sync_models(selected_provider, &config, tx).await {
        Ok(SetProviderOutcome {
            provider, model, ..
        }) => {
            let model_suffix = model
                .as_deref()
                .map(|model| format!("; model → {model}"))
                .unwrap_or_default();
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "connect".to_string(),
                    chunk: format!("  ✓ {provider} connected with {method_id}{model_suffix}"),
                },
            );
            // Update cached display so status bar never shows UNKNOWN.
            if let Ok(cfg) = config.try_read() {
                send(
                    tx,
                    TuiEvent::ProviderChanged {
                        provider: cfg.provider.default.clone(),
                        model: cfg.models.assistant.clone(),
                    },
                );
            }
        }
        Err(e) => send(
            tx,
            TuiEvent::Error {
                agent: "connect".to_string(),
                message: format!("connected but failed to persist provider config: {e}"),
            },
        ),
    }
}

struct SetProviderOutcome {
    provider: String,
    model: Option<String>,
}

async fn set_provider_and_sync_models(
    provider: &str,
    config: &Arc<RwLock<Config>>,
    tx: &TuiSender,
) -> Result<SetProviderOutcome> {
    let provider = crate::providers::registry::normalize_provider(provider)
        .trim()
        .to_string();
    if provider.is_empty() {
        anyhow::bail!("provider cannot be empty");
    }

    let config_snapshot = config.read().await.clone();
    let fetched_models = if crate::providers::models::default_model_for_config(
        &provider,
        &config_snapshot,
    )
    .is_none()
        || matches!(provider.as_str(), "lmstudio")
    {
        crate::providers::models::fetch_models_for_config(&provider, &config_snapshot)
            .await
            .ok()
    } else {
        None
    };

    let selected_model = fetched_models
        .as_ref()
        .and_then(|models| models.first())
        .cloned()
        .or_else(|| {
            crate::providers::models::default_model_for_config(&provider, &config_snapshot)
        });

    let qualified_model = selected_model
        .as_deref()
        .map(|model| crate::providers::models::qualify_model_string(model, &provider));

    {
        let mut cfg = config.write().await;
        if let Some(model) = &qualified_model {
            let _ = cfg.set_model("all", model.clone());
        } else {
            crate::providers::models::apply_provider_defaults(&mut cfg, &provider);
        }
        cfg.set_provider(provider.clone());
        cfg.save()
            .map_err(|e| anyhow::anyhow!("saved in memory but failed to persist: {e}"))?;
    }

    if let Some(models) = fetched_models
        && !models.is_empty()
    {
        send(
            tx,
            TuiEvent::ModelsLoaded {
                provider: provider.clone(),
                models,
            },
        );
    } else if qualified_model.is_none() {
        send(
            tx,
            TuiEvent::TokenChunk {
                agent: "provider".to_string(),
                chunk: format!(
                    "  ⚠ no models found for {provider}; load a model, then run /model assistant <model>"
                ),
            },
        );
    }

    Ok(SetProviderOutcome {
        provider,
        model: qualified_model,
    })
}

async fn handle_update_command(rest: &str, tx: &TuiSender) {
    send(
        tx,
        TuiEvent::AgentStarted {
            agent: "update".to_string(),
        },
    );

    let arg = rest.trim();
    if arg.is_empty() || arg == "install" {
        match crate::updater::check_latest().await {
            Ok(status) if !status.update_available => {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "update".to_string(),
                        chunk: format!("  cortex is up to date ({})", status.current),
                    },
                );
                send(
                    tx,
                    TuiEvent::AgentDone {
                        agent: "update".to_string(),
                    },
                );
            }
            Ok(status) => match crate::updater::update(Some(&status.latest)).await {
                Ok(outcome) => {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "update".to_string(),
                            chunk: format!(
                                "  updated cortex {} -> {} at {}",
                                outcome.previous,
                                outcome.installed,
                                outcome.binary_path.display()
                            ),
                        },
                    );
                    if outcome.restart_required {
                        send(
                            tx,
                            TuiEvent::TokenChunk {
                                agent: "update".to_string(),
                                chunk: "  restart your terminal to use the new version".to_string(),
                            },
                        );
                    }
                    send(
                        tx,
                        TuiEvent::AgentDone {
                            agent: "update".to_string(),
                        },
                    );
                }
                Err(e) => {
                    send(
                        tx,
                        TuiEvent::Error {
                            agent: "update".to_string(),
                            message: e.to_string(),
                        },
                    );
                }
            },
            Err(e) => {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "update".to_string(),
                        message: e.to_string(),
                    },
                );
            }
        }
        return;
    }

    if arg == "check" || arg == "--check" {
        match crate::updater::check_latest().await {
            Ok(status) if status.update_available => {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "update".to_string(),
                        chunk: format!(
                            "  update available: {} -> {}. Run /update to install.",
                            status.current, status.latest
                        ),
                    },
                );
                send(
                    tx,
                    TuiEvent::AgentDone {
                        agent: "update".to_string(),
                    },
                );
            }
            Ok(status) => {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "update".to_string(),
                        chunk: format!("  cortex is up to date ({})", status.current),
                    },
                );
                send(
                    tx,
                    TuiEvent::AgentDone {
                        agent: "update".to_string(),
                    },
                );
            }
            Err(e) => {
                send(
                    tx,
                    TuiEvent::Error {
                        agent: "update".to_string(),
                        message: e.to_string(),
                    },
                );
            }
        }
        return;
    }

    let version = arg.trim_start_matches("--version").trim();
    let version = if version.is_empty() { arg } else { version };
    match crate::updater::update(Some(version)).await {
        Ok(outcome) => {
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "update".to_string(),
                    chunk: format!(
                        "  updated cortex {} -> {} at {}",
                        outcome.previous,
                        outcome.installed,
                        outcome.binary_path.display()
                    ),
                },
            );
            send(
                tx,
                TuiEvent::AgentDone {
                    agent: "update".to_string(),
                },
            );
        }
        Err(e) => {
            send(
                tx,
                TuiEvent::Error {
                    agent: "update".to_string(),
                    message: e.to_string(),
                },
            );
        }
    }
}

async fn handle_skill_command(rest: &str, tx: &TuiSender) {
    let trimmed = rest.trim();
    if trimmed.is_empty() || trimmed == "add" || trimmed == "install" {
        send(tx, TuiEvent::OpenSkillPicker);
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            match crate::skills::fetch_catalog("all-time", 0, 100).await {
                Ok(skills) => {
                    let _ = tx_clone.send(TuiEvent::SkillsCatalogLoaded { skills });
                }
                Err(e) => {
                    let _ = tx_clone.send(TuiEvent::SkillPickerError {
                        message: e.to_string(),
                    });
                }
            }
        });
        return;
    }

    send(
        tx,
        TuiEvent::AgentStarted {
            agent: "skill".to_string(),
        },
    );

    let args = match crate::skills::parse_command_line(rest) {
        Ok(args) => args,
        Err(e) => {
            send(
                tx,
                TuiEvent::Error {
                    agent: "skill".to_string(),
                    message: e.to_string(),
                },
            );
            return;
        }
    };

    match crate::skills::run_command_async(&args).await {
        Ok(lines) => {
            for line in lines {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "skill".to_string(),
                        chunk: format!("  {line}"),
                    },
                );
            }
            send(
                tx,
                TuiEvent::AgentDone {
                    agent: "skill".to_string(),
                },
            );
        }
        Err(e) => {
            send(
                tx,
                TuiEvent::Error {
                    agent: "skill".to_string(),
                    message: e.to_string(),
                },
            );
        }
    }
}

/// Parses `dev "build a todo app"` or `"build a todo app"` (defaults to dev).
fn parse_workflow_and_prompt(rest: &str) -> (&str, &str) {
    // If the first token doesn't start with a quote, treat it as the workflow name
    if !rest.starts_with('"') {
        if let Some((maybe_wf, remainder)) = rest.split_once(char::is_whitespace) {
            let remainder = remainder.trim().trim_matches('"');
            return (maybe_wf, remainder);
        }
        return ("dev", rest.trim_matches('"'));
    }
    ("dev", rest.trim_matches('"'))
}

fn is_known_agent_role(name: &str) -> bool {
    matches!(
        name,
        "ceo"
            | "pm"
            | "tech_lead"
            | "developer"
            | "qa"
            | "devops"
            | "strategist"
            | "copywriter"
            | "analyst"
            | "social_media_manager"
            | "researcher"
            | "profiler"
            | "outreach_manager"
            | "reviewer"
            | "security"
            | "performance"
            | "reporter"
    )
}

// ---------------------------------------------------------------------------
// Free-form chat handler
// ---------------------------------------------------------------------------

/// Sends a free-form user message to the assistant agent and runs the agentic
/// loop (tool calls, file I/O, terminal commands). Maintains conversation history.
async fn chat_message(
    message: &str,
    tx: &TuiSender,
    config: Arc<RwLock<Config>>,
    state: Arc<ReplState>,
) -> Result<bool> {
    if message.is_empty() {
        return Ok(false);
    }

    let is_plan_mode = *state.execution_mode.lock().await == ExecutionMode::Plan;

    let model = {
        let cfg = config.read().await;
        crate::providers::model_for_role("assistant", &cfg)?.to_string()
    };

    // In Plan mode: inject plan-only instructions so the assistant describes
    // its approach without writing files or running commands.
    let effective_message = if is_plan_mode {
        format!(
            "[PLAN MODE] You are in plan mode. Analyze the request and describe step-by-step \
             what you would do — but do NOT write any files, execute commands, or make changes. \
             Produce a clear implementation plan. End your response with the line: PLAN READY.\n\n\
             User request: {}",
            message
        )
    } else {
        message.to_string()
    };

    // Snapshot history before the call (avoid holding the lock across await)
    let history_snapshot = { state.chat_history.lock().await.clone() };

    send(
        tx,
        TuiEvent::AgentStarted {
            agent: "cortex".to_string(),
        },
    );

    // Register a cancellation token so /abort can stop the assistant.
    let cancel = tokio_util::sync::CancellationToken::new();
    {
        let mut guard = state.cancel.lock().await;
        *guard = Some(cancel.clone());
    }

    let result = {
        let bus = state.agent_bus.read().await.clone();
        crate::assistant::run(
            &effective_message,
            history_snapshot,
            &model,
            tx,
            Arc::clone(&config),
            bus,
            cancel,
        )
        .await
    };

    // Clear the token when done.
    {
        let mut guard = state.cancel.lock().await;
        *guard = None;
    }

    match result {
        Ok((reply, full_history)) => {
            if is_plan_mode {
                // Store the original message so Approve can re-run it without restrictions.
                *state.pending_chat_message.lock().await = Some(message.to_string());

                // Write the plan to PLAN.md in the current directory.
                let plan_path = std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join("PLAN.md");
                let plan_content = format!(
                    "# Plan\n\n**Request:** {}\n\n{}\n\n---\n\
                     *Generated by Cortex — type `/approve` or press Enter in the popup to execute.*\n",
                    message,
                    crate::assistant::strip_tool_calls_for_display(&reply)
                );
                let _ = std::fs::write(&plan_path, &plan_content);

                send(
                    tx,
                    TuiEvent::AgentReplaceBuffer {
                        agent: "cortex".to_string(),
                        content: crate::assistant::strip_tool_calls_for_display(&reply),
                    },
                );
                send(
                    tx,
                    TuiEvent::AgentDone {
                        agent: "cortex".to_string(),
                    },
                );
                // Open PlanReview popup.
                send(
                    tx,
                    TuiEvent::PlanGenerated {
                        path: plan_path.display().to_string(),
                    },
                );
            } else {
                // Persist the full conversation history (includes tool calls/results)
                // so subsequent prompts have complete context.
                {
                    let mut hist = state.chat_history.lock().await;
                    *hist = full_history;
                }

                // Replace the accumulated stream buffer with the clean final reply so that
                // multi-iteration tool loops don't display duplicated content.
                send(
                    tx,
                    TuiEvent::AgentReplaceBuffer {
                        agent: "cortex".to_string(),
                        content: crate::assistant::strip_tool_calls_for_display(&reply),
                    },
                );
                send(
                    tx,
                    TuiEvent::AgentSummary {
                        agent: "cortex".to_string(),
                        summary: crate::workflows::summarize_output(&reply),
                    },
                );
                send(
                    tx,
                    TuiEvent::AgentDone {
                        agent: "cortex".to_string(),
                    },
                );
            }
        }
        Err(e) => {
            send(
                tx,
                TuiEvent::Error {
                    agent: "cortex".to_string(),
                    message: format!("cortex error: {e}"),
                },
            );
        }
    }

    Ok(false)
}

/// Returns true if the string looks like an API key rather than an auth method ID.
/// Heuristic: length > 20 and only alphanumeric / dash / underscore characters.
fn looks_like_api_key(s: &str) -> bool {
    s.len() > 20
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn list_output_files(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

/// Remove a ``` ... ``` code-block wrapper if the LLM added one around the .md content.
fn strip_code_block(s: &str) -> String {
    let s = s.trim();
    let after_fence = match s.strip_prefix("```") {
        Some(rest) => rest,
        None => return s.to_string(),
    };
    // skip optional language tag (markdown, md, yaml, …)
    let after_lang = after_fence.trim_start_matches(|c: char| c.is_alphanumeric() || c == '-');
    // strip trailing ```
    after_lang
        .trim_start_matches('\n')
        .trim_end()
        .strip_suffix("```")
        .map(|inner| inner.trim().to_string())
        .unwrap_or_else(|| s.to_string())
}

/// Finds the first standalone `---` line and returns everything from there.
/// Handles local models that prepend conversational text before the YAML frontmatter.
fn find_frontmatter_start(s: &str) -> Option<&str> {
    if s.trim_start().starts_with("---") {
        return Some(s.trim_start());
    }
    if let Some(pos) = s.find("\n---") {
        return Some(&s[pos + 1..]);
    }
    None
}

fn save_agent_file(name: &str, content: &str) -> anyhow::Result<std::path::PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?
        .join(".cortex")
        .join("agents");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        anyhow::bail!("agent '{}' already exists at {}", name, path.display());
    }
    std::fs::write(&path, content)?;
    Ok(path)
}

fn save_workflow_file(name: &str, content: &str) -> anyhow::Result<std::path::PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?
        .join(".cortex")
        .join("workflows");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        anyhow::bail!("workflow '{}' already exists at {}", name, path.display());
    }
    std::fs::write(&path, content)?;
    Ok(path)
}

/// For each agent referenced in a workflow definition, generate and save an
/// agent file if one does not already exist. Called after `/workflow create`
/// succeeds so every new workflow is immediately runnable.
async fn generate_workflow_agents(workflow_content: &str, model: &str, tx: &TuiSender) {
    let Ok(def) = crate::custom_defs::parse_workflow_def(workflow_content) else {
        return;
    };
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let agents_dir = home.join(".cortex").join("agents");
    let _ = std::fs::create_dir_all(&agents_dir);

    let mut seen = std::collections::HashSet::new();
    for step in &def.agents {
        if !seen.insert(step.agent.clone()) {
            continue;
        }
        let agent_path = agents_dir.join(format!("{}.md", step.agent));
        if agent_path.exists() {
            continue;
        }
        send(
            tx,
            TuiEvent::TokenChunk {
                agent: "workflow".to_string(),
                chunk: format!("  Generating agent '{}'...", step.agent),
            },
        );
        let role_hint = format!("{} (used in the {} workflow)", step.role, def.name);
        match ai_generate_agent(&step.agent, Some(&role_hint), model).await {
            Ok(agent_content) => match std::fs::write(&agent_path, &agent_content) {
                Ok(()) => send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "workflow".to_string(),
                        chunk: format!("  Created agent: ~/.cortex/agents/{}.md", step.agent),
                    },
                ),
                Err(e) => send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "workflow".to_string(),
                        chunk: format!("  Error saving agent '{}': {}", step.agent, e),
                    },
                ),
            },
            Err(e) => send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "workflow".to_string(),
                    chunk: format!("  Failed to generate agent '{}': {}", step.agent, e),
                },
            ),
        }
    }
}

async fn ai_generate_agent(
    name: &str,
    description: Option<&str>,
    model: &str,
) -> anyhow::Result<String> {
    let desc_ctx = description
        .map(|d| format!("\nAgent purpose provided by the user: {}", d))
        .unwrap_or_default();

    let preamble = "You are an expert Cortex agent definition generator. \
        You produce richly detailed, production-ready agent .md files that serve as \
        comprehensive system prompts for specialized AI agents. \
        Output ONLY the raw Markdown+YAML content starting with --- \
        (no code fences, no explanations, no extra text before or after).";

    let prompt = format!(
        r#"Generate a complete, production-quality Cortex agent definition for an agent named "{name}".{desc_ctx}

STRICT RULES — follow every one:

1. `description` MUST use a YAML block scalar (`description: >`) containing:
   - One opening sentence explaining what this agent does and when to use it
   - 2 to 3 `<example>` blocks, each with: a "Context:" paragraph, a `user:` line, an `assistant:` line, and a `<commentary>` block explaining why this agent is the right choice for this situation
   Escape any special YAML characters inside the block.

2. `model` must be exactly: {model}

3. `tools` MUST be a YAML inline list — pick the relevant subset from:
   Read, Write, Edit, Bash, Glob, Grep, WebFetch, WebSearch
   Example: `tools: [Read, Glob, Grep]`
   Use an empty list if the agent needs no tools: `tools: []`

4. The body (after the closing ---) MUST contain these labelled sections:
   ## Role
   ## Focus Areas  (bullet list of key capabilities)
   ## Approach     (numbered step-by-step methodology)
   ## Output Format
   ## Constraints  (what the agent must never do or assume)
   Minimum 400 words of actionable, expert guidance. No generic filler.

QUALITY REFERENCE — this is a real example for a DIFFERENT agent (code-reviewer).
Do NOT copy it. Use it only to understand the expected structure and depth:

---
name: code-reviewer
description: >
  Use this agent when you need a deep, systematic code review that catches logic
  bugs, security vulnerabilities, and architectural issues — not just style problems.

  <example>
  Context: A developer submitted a PR adding JWT authentication middleware to a Node.js API.
  user: "Review this middleware for correctness and security issues."
  assistant: "I'll audit the JWT validation logic for algorithm confusion attacks, verify expiry and signature checks, review error handling to avoid leaking internals, and check that secrets come from environment variables only."
  <commentary>
  Security-sensitive code requires reviewing trust boundaries and failure modes that
  a style linter will never catch. This agent reasons about attack vectors, not formatting.
  </commentary>
  </example>

  <example>
  Context: A refactor touched 20 files and the developer wants a second pair of eyes.
  user: "Does this refactor introduce regressions or break any invariants?"
  assistant: "I'll trace the call graph before and after, identify changed public contracts, check for silent behaviour changes in edge cases, and flag any assumptions that are now violated."
  <commentary>
  Large refactors need cross-file analysis to catch emergent breakage. Use this agent
  when correctness and backward compatibility matter across a wide surface area.
  </commentary>
  </example>
model: ollama/qwen2.5-coder:32b
tools: [Read, Glob, Grep, Bash]
---

## Role

You are a senior software engineer specialising in adversarial code review...

## Focus Areas

- Security: injection, authentication bypass, secrets in code, unsafe deserialization
- Correctness: off-by-one errors, race conditions, null/undefined handling, error propagation
- Architecture: coupling, cohesion, violation of SOLID principles, circular dependencies
- Performance: N+1 queries, blocking I/O in hot paths, memory leaks
- Testability: side-effect isolation, dependency injection, mockability

## Approach

1. Read the diff or file list to understand the change surface
2. Map data flow from inputs to outputs, noting trust boundaries
3. Check each changed function for correctness, edge cases, and error handling
4. Audit security-sensitive paths (auth, crypto, file I/O, external calls)
5. Assess architectural impact on the rest of the codebase
6. Write findings grouped by severity: Critical / High / Medium / Low / Suggestion

## Output Format

Return a structured report:
**Summary** — one-paragraph verdict
**Critical** — must fix before merge (numbered list)
**High** — should fix soon (numbered list)
**Medium / Low** — improvements worth tracking
**Suggestions** — optional polish

## Constraints

- Never approve code with unhandled secrets or hardcoded credentials
- Do not rewrite code unless explicitly asked — report issues only
- Base findings on the actual code, not assumptions about intent
---

NOW generate the complete agent definition for "{name}". Follow the exact same structure and quality level. Output ONLY the raw Markdown starting with ---"#
    );

    let response =
        crate::providers::complete_chat(model, preamble, vec![], &prompt).await?;
    let cleaned = strip_code_block(&response);
    match find_frontmatter_start(&cleaned) {
        Some(content) => Ok(content.to_string()),
        None => anyhow::bail!("generated content is missing YAML frontmatter"),
    }
}

async fn ai_generate_workflow(
    name: &str,
    description: Option<&str>,
    model: &str,
) -> anyhow::Result<String> {
    let desc_ctx = description
        .map(|d| format!("\nWorkflow purpose provided by the user: {}", d))
        .unwrap_or_default();

    let preamble = "You are an expert Cortex workflow definition generator. \
        Workflows chain specialised agents sequentially — each agent's output becomes \
        the next agent's input. You design coherent multi-agent pipelines. \
        Output ONLY the raw Markdown+YAML starting with --- \
        (no code fences, no explanations, no text before or after).";

    let prompt = format!(
        r#"Generate a complete, production-quality Cortex workflow definition named "{name}".{desc_ctx}

STRICT RULES:

1. `description` — 2 to 3 sentences explaining: what problem this workflow solves,
   when to use it, and what the final output looks like.

2. `agents` — 3 to 5 steps. Each step needs:
   - `role`: a human-readable label for what this step does (e.g. "researcher", "analyst", "writer")
   - `agent`: the agent file name to invoke (snake-case, no extension)
   Name agents based on their specialised function.
   Each agent's system prompt MUST instruct the agent to use the injected
   `## Web Search Results` section as its primary data source and to NEVER
   invent or hallucinate information.

3. Body — the workflow narrative MUST cover:
   ## Purpose       (what problem it solves, expected output)
   ## Agent Pipeline (describe each step's input, task, and output in order)
   ## Data Flow     (what information passes between agents and how it accumulates)
   ## When to Use   (2-3 use cases with concrete examples)
   Minimum 200 words of specific, actionable content.

QUALITY REFERENCE — a real example for a DIFFERENT workflow (content-production).
Do NOT copy it. Use it only to understand the structure:

---
name: content-production
description: >
  End-to-end content creation pipeline that takes a topic or brief and produces
  a polished, SEO-optimised article ready for publication. Use this workflow when
  you need research-backed, well-structured long-form content with quality review.
agents:
  - role: researcher
    agent: researcher
  - role: outline-writer
    agent: content-strategist
  - role: copywriter
    agent: copywriter
  - role: editor
    agent: editor
---

## Purpose

Produces publication-ready long-form content from a brief...

## Agent Pipeline

1. **researcher** — searches for primary sources, statistics, and expert quotes...
2. **content-strategist** — structures an outline from the research...
3. **copywriter** — writes the full article following the outline...
4. **editor** — reviews for clarity, tone, SEO, and factual accuracy...

## Data Flow

Each agent receives the accumulated context from all prior steps plus its specific input...

## When to Use

- Blog posts and technical articles that require research and multiple review passes
- Marketing copy that needs fact-checking before publication
---

NOW generate the complete workflow definition for "{name}". Follow the exact same structure and quality. Output ONLY the raw Markdown starting with ---"#
    );

    let response =
        crate::providers::complete_chat(model, preamble, vec![], &prompt).await?;
    let cleaned = strip_code_block(&response);
    match find_frontmatter_start(&cleaned) {
        Some(content) => Ok(content.to_string()),
        None => anyhow::bail!("generated content is missing YAML frontmatter"),
    }
}

fn handle_agent_create(name: &str, model: &str) -> anyhow::Result<std::path::PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?
        .join(".cortex")
        .join("agents");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        anyhow::bail!("agent '{}' already exists at {}", name, path.display());
    }
    let template = format!(
        "---\nname: {name}\ndescription: Describe what this agent does\nmodel: {model}\ntools: []\n---\n# {name} Agent\n\nYou are an expert ...\n"
    );
    std::fs::write(&path, template)?;
    Ok(path)
}

fn handle_workflow_create(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?
        .join(".cortex")
        .join("workflows");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", name));
    if path.exists() {
        anyhow::bail!("workflow '{}' already exists at {}", name, path.display());
    }
    let template = format!(
        "---\nname: {name}\ndescription: Describe what this workflow does\nagents:\n  - role: step1\n    agent: your-agent-name\n---\n# {name} Workflow\n\nDescribe this workflow here.\n"
    );
    std::fs::write(&path, template)?;
    Ok(path)
}
