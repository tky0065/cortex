use std::sync::Arc;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::agent_bus::AgentBus;
use crate::config::Config;
use crate::tui::events::{Task, TuiEvent, TuiSender, channel};
use crate::workflows::{RunOptions, Workflow};

pub struct Orchestrator {
    workflow: Box<dyn Workflow>,
    config: Arc<Config>,
    cancel: CancellationToken,
    /// Sender half of the resume channel; the REPL calls `resume_tx.send(())` to unblock pauses.
    resume_tx: Arc<tokio::sync::mpsc::Sender<()>>,
    /// Receiver half — shared with RunOptions so the workflow can await resume signals.
    resume_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>>,
    /// Sender half of the answer channel; the TUI sends user text answers here.
    answer_tx: Arc<tokio::sync::mpsc::Sender<String>>,
    /// Receiver half — shared with RunOptions so agents can await answers.
    answer_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<String>>>,
    /// Reference to REPL state for updating session history (set via with_repl_state).
    pub repl_state: Option<Arc<crate::repl::ReplState>>,
}

impl Orchestrator {
    pub fn new(workflow: Box<dyn Workflow>, config: Arc<Config>) -> Self {
        let cancel = CancellationToken::new();
        let (tx, rx) = tokio::sync::mpsc::channel::<()>(4);
        let (atx, arx) = tokio::sync::mpsc::channel::<String>(4);
        Self {
            workflow,
            config,
            cancel,
            resume_tx: Arc::new(tx),
            resume_rx: Arc::new(tokio::sync::Mutex::new(rx)),
            answer_tx: Arc::new(atx),
            answer_rx: Arc::new(tokio::sync::Mutex::new(arx)),
            repl_state: None,
        }
    }

    /// Set the REPL state reference for session tracking.
    pub fn with_repl_state(mut self, repl_state: Arc<crate::repl::ReplState>) -> Self {
        self.repl_state = Some(repl_state);
        self
    }

    /// Cancel the running workflow.
    #[allow(dead_code)]
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Clone of the cancellation token — let callers cancel independently.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Sender that the REPL can use to resume an interactive pause.
    pub fn resume_sender(&self) -> Arc<tokio::sync::mpsc::Sender<()>> {
        Arc::clone(&self.resume_tx)
    }

    /// Sender that the TUI can use to deliver a text answer to a waiting agent.
    pub fn answer_sender(&self) -> Arc<tokio::sync::mpsc::Sender<String>> {
        Arc::clone(&self.answer_tx)
    }

    #[allow(dead_code)]
    pub async fn run(&self, prompt: String, auto: bool) -> Result<()> {
        self.run_with_opts(prompt, auto, false, None).await
    }

    pub async fn run_with_sender(
        &self,
        prompt: String,
        auto: bool,
        tx: Option<TuiSender>,
    ) -> Result<()> {
        self.run_with_opts(prompt, auto, false, tx).await
    }

    pub async fn run_with_opts(
        &self,
        prompt: String,
        auto: bool,
        verbose: bool,
        tx: Option<TuiSender>,
    ) -> Result<()> {
        self.run_with_project_dir(prompt, auto, verbose, tx, None)
            .await
    }

    pub async fn run_with_project_dir(
        &self,
        prompt: String,
        auto: bool,
        verbose: bool,
        tx: Option<TuiSender>,
        project_dir: Option<std::path::PathBuf>,
    ) -> Result<()> {
        // Resolve the primary event sender (TUI or throw-away).
        let tx = tx.unwrap_or_else(|| channel().0);
        let project_dir = project_dir.unwrap_or_else(|| {
            default_project_dir(
                self.workflow.name(),
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )
        });

        // Spawn a background task to watch TASKS.md for UI updates.
        spawn_task_watcher(tx.clone(), project_dir.clone(), self.cancel.clone());

        // Create a fresh AgentBus for this workflow run and share it with the REPL.
        let agent_bus = AgentBus::new();
        if let Some(ref repl_state) = self.repl_state {
            *repl_state.agent_bus.write().await = Some(Arc::clone(&agent_bus));
        }

        // When verbose, tap a clone of the sender into a logging task.
        if verbose {
            let (log_tx, mut log_rx) = channel();
            // We'll forward from a clone of the main sender.
            // Spawn the file-writer that drains log_rx.
            tokio::spawn(async move {
                use std::io::Write;
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("cortex.log");
                match file {
                    Ok(mut f) => {
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let _ = writeln!(f, "=== cortex session (unix={}) ===", ts);
                        while let Some(ev) = log_rx.recv().await {
                            if let TuiEvent::TokenChunk {
                                ref agent,
                                ref chunk,
                            } = ev
                            {
                                let _ = writeln!(f, "[{}] {}", agent, chunk);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("warning: could not open cortex.log: {}", e);
                    }
                }
            });
            // Spawn a forwarder that clones events from the main tx into log_tx.
            // Since UnboundedSender is Clone, clone tx and forward.
            let tx_clone = tx.clone();
            // We can't intercept sends directly; instead expose a "tee sender"
            // by wrapping: create a new channel whose receiver forwards to both.
            let (tee_tx, mut tee_rx) = channel();
            let real_tx = tx_clone;
            tokio::spawn(async move {
                while let Some(ev) = tee_rx.recv().await {
                    let _ = log_tx.send(ev.clone());
                    let _ = real_tx.send(ev);
                }
            });
            // Use the tee sender as the workflow sender.
            let options = RunOptions {
                auto,
                config: Arc::clone(&self.config),
                tx: tee_tx.clone(),
                project_dir: project_dir.clone(),
                cancel: self.cancel.clone(),
                resume_tx: Arc::clone(&self.resume_tx),
                resume_rx: Arc::clone(&self.resume_rx),
                answer_tx: Arc::clone(&self.answer_tx),
                answer_rx: Arc::clone(&self.answer_rx),
                verbose,
                agent_bus: Some(Arc::clone(&agent_bus)),
            };

            return tokio::select! {
                result = self.workflow.run(prompt, options) => result,
                _ = self.cancel.cancelled() => {
                    let _ = tee_tx.send(TuiEvent::TokenChunk {
                        agent: "orchestrator".into(),
                        chunk: "Workflow aborted.".into(),
                    });
                    Ok(())
                }
            };
        }

        let options = RunOptions {
            auto,
            config: Arc::clone(&self.config),
            tx: tx.clone(),
            project_dir,
            cancel: self.cancel.clone(),
            resume_tx: Arc::clone(&self.resume_tx),
            resume_rx: Arc::clone(&self.resume_rx),
            answer_tx: Arc::clone(&self.answer_tx),
            answer_rx: Arc::clone(&self.answer_rx),
            verbose,
            agent_bus: Some(Arc::clone(&agent_bus)),
        };

        tokio::select! {
            result = self.workflow.run(prompt, options) => result,
            _ = self.cancel.cancelled() => {
                let _ = tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: "Workflow aborted.".into(),
                });
                Ok(())
            }
        }
    }
}

fn default_project_dir(workflow_name: &str, cwd: std::path::PathBuf) -> std::path::PathBuf {
    if workflow_name == "dev" {
        cwd
    } else {
        cwd.join("cortex-output")
    }
}

/// Polls for a TASKS.md file in the project directory and sends TasksUpdated events.
fn spawn_task_watcher(tx: TuiSender, project_dir: std::path::PathBuf, cancel: CancellationToken) {
    tokio::spawn(async move {
        let tasks_path = project_dir.join("TASKS.md");
        let mut last_content = String::new();

        loop {
            if cancel.is_cancelled() {
                break;
            }

            if tasks_path.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&tasks_path).await {
                    if content != last_content {
                        let tasks = parse_tasks(&content);
                        let _ = tx.send(TuiEvent::TasksUpdated { tasks });
                        last_content = content;
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    });
}

fn parse_tasks(content: &str) -> Vec<Task> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with("- [ ] ") {
                Some(Task {
                    description: line[6..].to_string(),
                    is_done: false,
                })
            } else if line.starts_with("- [x] ") {
                Some(Task {
                    description: line[6..].to_string(),
                    is_done: true,
                })
            } else if line.starts_with("- [X] ") {
                Some(Task {
                    description: line[6..].to_string(),
                    is_done: true,
                })
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::default_project_dir;
    use crate::tui::events::{TuiEvent, channel};

    #[test]
    fn dev_workflow_defaults_to_current_directory() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(default_project_dir("dev", cwd.clone()), cwd);
    }

    #[test]
    fn non_dev_workflows_keep_cortex_output_directory() {
        let cwd = PathBuf::from("/tmp/demo");
        assert_eq!(
            default_project_dir("marketing", cwd.clone()),
            cwd.join("cortex-output")
        );
    }

    /// Phase events sent in sequence must arrive in the same order.
    #[tokio::test]
    async fn test_phase_transitions() {
        let (tx, mut rx) = channel();

        let phases = ["init", "plan", "build", "test", "deploy"];
        for phase in &phases {
            tx.send(TuiEvent::PhaseComplete {
                phase: phase.to_string(),
            })
            .unwrap();
        }

        for expected in &phases {
            let event = rx.recv().await.expect("channel closed prematurely");
            match event {
                TuiEvent::PhaseComplete { phase } => {
                    assert_eq!(phase, *expected, "phase arrived out of order");
                }
                other => panic!("unexpected event: {:?}", other),
            }
        }
    }

    /// Ten concurrent senders must all deliver without deadlock.
    #[tokio::test]
    async fn test_parallel_events_no_deadlock() {
        let (tx, mut rx) = channel();
        let mut handles = Vec::new();

        for i in 0..10_u32 {
            let tx = tx.clone();
            handles.push(tokio::spawn(async move {
                tx.send(TuiEvent::TokenChunk {
                    agent: format!("agent{}", i),
                    chunk: format!("chunk{}", i),
                })
                .expect("send failed");
            }));
        }

        for h in handles {
            h.await.expect("task panicked");
        }

        // Drop the last sender so the receiver will see EOF.
        drop(tx);

        let mut count = 0;
        while rx.recv().await.is_some() {
            count += 1;
        }
        assert_eq!(count, 10, "expected 10 events, got {}", count);
    }

    /// WorkflowStarted event carries agent list intact.
    #[tokio::test]
    async fn test_workflow_started_event() {
        let (tx, mut rx) = channel();
        let agents = vec!["ceo".to_string(), "pm".to_string(), "developer".to_string()];

        tx.send(TuiEvent::WorkflowStarted {
            workflow: "dev".into(),
            agents: agents.clone(),
        })
        .unwrap();

        let event = rx.recv().await.unwrap();
        match event {
            TuiEvent::WorkflowStarted {
                workflow,
                agents: got,
            } => {
                assert_eq!(workflow, "dev");
                assert_eq!(got, agents);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    /// AgentStarted followed by AgentDone arrive in the correct order.
    #[tokio::test]
    async fn test_agent_lifecycle_ordering() {
        let (tx, mut rx) = channel();

        tx.send(TuiEvent::AgentStarted {
            agent: "ceo".into(),
        })
        .unwrap();
        tx.send(TuiEvent::AgentDone {
            agent: "ceo".into(),
        })
        .unwrap();

        let e1 = rx.recv().await.unwrap();
        let e2 = rx.recv().await.unwrap();

        assert!(matches!(e1, TuiEvent::AgentStarted { agent } if agent == "ceo"));
        assert!(matches!(e2, TuiEvent::AgentDone   { agent } if agent == "ceo"));
    }
}
