use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::tui::events::{TuiEvent, TuiSender};
use crate::workflows;
use crate::orchestrator::Orchestrator;

/// Shared state for the currently-running workflow, if any.
/// The REPL dispatch function holds an `Arc` to this so `/abort` and
/// `/continue` can reach the in-flight orchestrator without a global.
#[derive(Clone, Default)]
pub struct ReplState {
    /// Cancel token for the running workflow (if any).
    pub cancel: Arc<Mutex<Option<CancellationToken>>>,
    /// Resume sender — calling `send(())` unblocks the next interactive pause.
    pub resume_tx: Arc<Mutex<Option<Arc<tokio::sync::mpsc::Sender<()>>>>>,
}

impl ReplState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Dispatches a slash command entered in the TUI input bar.
/// Returns `true` if the application should quit.
pub async fn dispatch(
    cmd: &str,
    tx: &TuiSender,
    config: Arc<Config>,
    state: Arc<ReplState>,
) -> Result<bool> {
    let trimmed = cmd.trim();
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
                "  /status                       — show current workflow status",
                "  /abort                        — cancel the running workflow",
                "  /continue                     — resume an interactive pause",
                "  /config                       — print active configuration",
                "  /logs                         — toggle log panel focus",
                "  /quit                         — exit cortex",
            ];
            for line in lines {
                send(tx, TuiEvent::AgentStarted { agent: "help".to_string() });
                send(tx, TuiEvent::TokenChunk {
                    agent: "help".to_string(),
                    chunk: line.to_string(),
                });
            }
        }

        "/config" => {
            send(tx, TuiEvent::TokenChunk {
                agent: "config".to_string(),
                chunk: format!("  provider: {}", config.provider.default),
            });
            send(tx, TuiEvent::TokenChunk {
                agent: "config".to_string(),
                chunk: format!("  max_parallel_workers: {}", config.limits.max_parallel_workers),
            });
            send(tx, TuiEvent::TokenChunk {
                agent: "config".to_string(),
                chunk: format!("  max_qa_iterations: {}", config.limits.max_qa_iterations),
            });
        }

        "/start" | "/run" => {
            if rest.is_empty() {
                send(tx, TuiEvent::Error {
                    agent: "repl".to_string(),
                    message: format!("usage: {} <workflow> \"<prompt>\"", command),
                });
                return Ok(false);
            }

            // Parse: /start dev "my idea" OR /start "my idea" (defaults to dev)
            let (workflow_name, prompt) = parse_workflow_and_prompt(rest);

            match workflows::get_workflow(workflow_name) {
                Err(e) => {
                    send(tx, TuiEvent::Error {
                        agent: "repl".to_string(),
                        message: e.to_string(),
                    });
                }
                Ok(wf) => {
                    send(tx, TuiEvent::AgentStarted {
                        agent: format!("workflow:{}", workflow_name),
                    });
                    let tx_clone = tx.clone();
                    let orch = Orchestrator::new(wf, Arc::clone(&config));

                    // Store the cancel token and resume sender so /abort and /continue work
                    {
                        let mut cancel_guard = state.cancel.lock().await;
                        *cancel_guard = Some(orch.cancel_token());
                    }
                    {
                        let mut resume_guard = state.resume_tx.lock().await;
                        *resume_guard = Some(orch.resume_sender());
                    }

                    let prompt_owned = prompt.to_string();
                    // Spawn so the TUI stays responsive
                    tokio::spawn(async move {
                        let _ = orch.run_with_sender(prompt_owned, false, Some(tx_clone)).await;
                    });
                }
            }
        }

        "/status" => {
            let running = state.cancel.lock().await.is_some();
            send(tx, TuiEvent::TokenChunk {
                agent: "status".to_string(),
                chunk: if running {
                    "  Workflow running.".to_string()
                } else {
                    "  No workflow running.".to_string()
                },
            });
        }

        "/abort" => {
            let mut cancel_guard = state.cancel.lock().await;
            if let Some(token) = cancel_guard.take() {
                token.cancel();
                send(tx, TuiEvent::TokenChunk {
                    agent: "abort".to_string(),
                    chunk: "  Abort signal sent — workflow will stop at the next checkpoint.".to_string(),
                });
            } else {
                send(tx, TuiEvent::TokenChunk {
                    agent: "abort".to_string(),
                    chunk: "  No workflow is currently running.".to_string(),
                });
            }
        }

        "/continue" => {
            let resume_guard = state.resume_tx.lock().await;
            if let Some(tx_resume) = resume_guard.as_ref() {
                let _ = tx_resume.try_send(());
                send(tx, TuiEvent::Resume);
                send(tx, TuiEvent::TokenChunk {
                    agent: "repl".to_string(),
                    chunk: "  Resuming workflow…".to_string(),
                });
            } else {
                send(tx, TuiEvent::TokenChunk {
                    agent: "repl".to_string(),
                    chunk: "  No workflow is paused.".to_string(),
                });
            }
        }

        "/logs" => {
            send(tx, TuiEvent::TokenChunk {
                agent: "logs".to_string(),
                chunk: "  Log panel focus toggled.".to_string(),
            });
        }

        other => {
            send(tx, TuiEvent::Error {
                agent: "repl".to_string(),
                message: format!("unknown command '{}' — type /help", other),
            });
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
