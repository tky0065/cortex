use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::Config;
use crate::orchestrator::Orchestrator;
use crate::tui::events::{TuiEvent, TuiSender};
use crate::workflows;

const ASSISTANT_PREAMBLE: &str = "\
You are Cortex, a helpful AI assistant embedded in an agentic software development CLI. \
You help users understand the tool, suggest workflows, and answer questions about software development.\n\
\n\
Available slash commands the user can run:\n\
  /start <workflow> \"<idea>\"  — launch a workflow (dev, marketing, prospecting, code-review)\n\
  /resume <project-dir>         — resume an interrupted workflow\n\
  /status                       — show current workflow status\n\
  /abort                        — cancel the running workflow\n\
  /continue                     — resume an interactive pause\n\
  /config                       — print active configuration\n\
  /model [<role> <model>]       — show or change a role's model (role: ceo/pm/tech_lead/developer/qa/devops/assistant/all)\n\
  /provider [<name>]            — show or change the default provider (ollama/openrouter/groq/together)\n\
  /logs                         — toggle log panel focus\n\
  /help                         — show all commands\n\
  /quit                         — exit cortex\n\
\n\
When the user describes a project idea, suggest the right /start command. \
Keep answers concise and practical.";

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
                "  /status                       — show current workflow status",
                "  /abort                        — cancel the running workflow",
                "  /continue                     — resume an interactive pause",
                "  /config                       — print active configuration",
                "  /model [<role> <model>]       — show or change a role's model",
                "  /provider [<name>]            — show or change the default provider",
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
                match cfg.set_model(role, model_str.to_string()) {
                    Ok(()) => {
                        if let Err(e) = cfg.save() {
                            send(
                                tx,
                                TuiEvent::Error {
                                    agent: "model".to_string(),
                                    message: format!("saved in memory but failed to persist: {e}"),
                                },
                            );
                        } else {
                            send(
                                tx,
                                TuiEvent::TokenChunk {
                                    agent: "model".to_string(),
                                    chunk: format!("  ✓ {} → {} (saved)", role, model_str),
                                },
                            );
                        }
                    }
                    Err(e) => {
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
                let name = rest.to_string();
                let mut cfg = config.write().await;
                cfg.set_provider(name.clone());
                if let Err(e) = cfg.save() {
                    send(
                        tx,
                        TuiEvent::Error {
                            agent: "provider".to_string(),
                            message: format!("saved in memory but failed to persist: {e}"),
                        },
                    );
                } else {
                    send(
                        tx,
                        TuiEvent::TokenChunk {
                            agent: "provider".to_string(),
                            chunk: format!("  ✓ provider → {} (saved)", name),
                        },
                    );
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

                        // Update session status when workflow completes
                        {
                            let mut history = state_for_spawn.session_history.lock().unwrap();
                            if let Some(session) = history.iter_mut().find(|s| s.id == session_id) {
                                match &result {
                                    Ok(()) => session.status = SessionStatus::Completed,
                                    Err(_) => session.status = SessionStatus::Failed,
                                }
                                // Try to get git hash if available
                                session.git_hash = Some(
                                    std::process::Command::new("git")
                                        .arg("rev-parse")
                                        .arg("HEAD")
                                        .output()
                                        .ok()
                                        .and_then(|output| String::from_utf8(output.stdout).ok())
                                        .map(|s| s.trim().to_string())
                                        .unwrap_or_default(),
                                );
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
                    let _ = orch.run_with_sender(prompt, true, Some(tx_clone)).await;
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

/// Sends a free-form user message to the assistant agent and streams the reply
/// back to the TUI. Maintains per-session conversation history in `state`.
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
            agent: "assistant".to_string(),
        },
    );
    send(
        tx,
        TuiEvent::AgentProgress {
            agent: "assistant".to_string(),
            message: "Preparation de la reponse".to_string(),
        },
    );

    let result =
        crate::providers::complete_chat(&model, ASSISTANT_PREAMBLE, history_snapshot, message)
            .await;

    match result {
        Ok(reply) => {
            // Persist both turns to history
            {
                let mut hist = state.chat_history.lock().await;
                hist.push(rig::completion::Message::user(message));
                hist.push(rig::completion::Message::assistant(&reply));
            }

            send(
                tx,
                TuiEvent::AgentSummary {
                    agent: "assistant".to_string(),
                    summary: crate::workflows::summarize_output(&reply),
                },
            );
            // Stream reply line-by-line so the log panel shows it nicely
            for line in reply.lines() {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "assistant".to_string(),
                        chunk: line.to_string(),
                    },
                );
            }
            send(
                tx,
                TuiEvent::AgentDone {
                    agent: "assistant".to_string(),
                },
            );
        }
        Err(e) => {
            send(
                tx,
                TuiEvent::Error {
                    agent: "assistant".to_string(),
                    message: format!("chat error: {e}"),
                },
            );
        }
    }

    Ok(false)
}
