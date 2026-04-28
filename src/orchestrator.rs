use std::sync::Arc;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::tui::events::{TuiEvent, TuiSender, channel};
use crate::workflows::{RunOptions, Workflow};

pub struct Orchestrator {
    workflow: Box<dyn Workflow>,
    config: Arc<Config>,
    cancel: CancellationToken,
    /// Sender half of the resume channel; the REPL calls `resume_tx.send(())` to unblock pauses.
    resume_tx: Arc<tokio::sync::mpsc::Sender<()>>,
    /// Receiver half — held here so it lives as long as the orchestrator.
    #[allow(dead_code)]
    resume_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>>,
}

impl Orchestrator {
    pub fn new(workflow: Box<dyn Workflow>, config: Arc<Config>) -> Self {
        let cancel = CancellationToken::new();
        let (tx, rx) = tokio::sync::mpsc::channel::<()>(4);
        Self {
            workflow,
            config,
            cancel,
            resume_tx: Arc::new(tx),
            resume_rx: Arc::new(tokio::sync::Mutex::new(rx)),
        }
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
        println!("Workflow: {} — {}", self.workflow.name(), self.workflow.description());

        if verbose {
            println!("Verbose logging enabled — writing to cortex.log");
        }

        // Resolve the primary event sender (TUI or throw-away).
        let tx = tx.unwrap_or_else(|| channel().0);

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
                            if let TuiEvent::TokenChunk { ref agent, ref chunk } = ev {
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
            let project_dir = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("cortex-output");

            let options = RunOptions {
                auto,
                config: Arc::clone(&self.config),
                tx: tee_tx.clone(),
                project_dir,
                cancel: self.cancel.clone(),
                resume_tx: Arc::clone(&self.resume_tx),
                verbose,
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

        let project_dir = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join("cortex-output");

        let options = RunOptions {
            auto,
            config: Arc::clone(&self.config),
            tx: tx.clone(),
            project_dir,
            cancel: self.cancel.clone(),
            resume_tx: Arc::clone(&self.resume_tx),
            verbose,
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::tui::events::{TuiEvent, channel};

    /// Phase events sent in sequence must arrive in the same order.
    #[tokio::test]
    async fn test_phase_transitions() {
        let (tx, mut rx) = channel();

        let phases = ["init", "plan", "build", "test", "deploy"];
        for phase in &phases {
            tx.send(TuiEvent::PhaseComplete { phase: phase.to_string() }).unwrap();
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
            TuiEvent::WorkflowStarted { workflow, agents: got } => {
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

        tx.send(TuiEvent::AgentStarted { agent: "ceo".into() }).unwrap();
        tx.send(TuiEvent::AgentDone   { agent: "ceo".into() }).unwrap();

        let e1 = rx.recv().await.unwrap();
        let e2 = rx.recv().await.unwrap();

        assert!(matches!(e1, TuiEvent::AgentStarted { agent } if agent == "ceo"));
        assert!(matches!(e2, TuiEvent::AgentDone   { agent } if agent == "ceo"));
    }
}
