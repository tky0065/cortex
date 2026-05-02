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
use crate::workflows;

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
}

impl ReplState {
    pub fn new() -> Self {
        let mut state = Self::default();
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
                "  /status                       — show current workflow status",
                "  /abort                        — cancel the running workflow",
                "  /continue                     — resume an interactive pause",
                "  /agents                       — show status of all agents in the current workflow",
                "  /agent <name> \"<directive>\"  — inject a directive to a specific agent",
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
                    let mut orch = Orchestrator::new(wf, config_snapshot);

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
            // Syntax: /agent <name> "<directive>"  OR  /agent <name> <directive>
            if rest.is_empty() {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "agent".to_string(),
                        chunk: "  Usage: /agent <name> \"<directive>\"".to_string(),
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
                        TuiEvent::TokenChunk {
                            agent: "agent".to_string(),
                            chunk: "  Usage: /agent <name> \"<directive>\"".to_string(),
                        },
                    );
                } else {
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
                                chunk: "  No workflow is currently running.".to_string(),
                            },
                        );
                    }
                }
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
                        chunk: format!("  ✓ {} API key saved (tip: use /apikey {} <key> next time)", provider, provider),
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
    if rest.trim().is_empty() {
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

    let model = {
        let cfg = config.read().await;
        crate::providers::model_for_role("assistant", &cfg)?.to_string()
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
            message,
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
    s.len() > 20 && s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
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
