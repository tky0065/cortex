use tokio::sync::mpsc;

/// Events sent from the orchestrator to the TUI renderer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TuiEvent {
    WorkflowStarted { workflow: String, agents: Vec<String> },
    AgentStarted { agent: String },
    TokenChunk { agent: String, chunk: String },
    AgentDone { agent: String },
    PhaseComplete { phase: String },
    Error { agent: String, message: String },
    /// Emitted when the workflow pauses waiting for user confirmation (interactive mode).
    InteractivePause { message: String },
    /// Emitted by the REPL `/continue` command to resume a paused workflow.
    Resume,
    /// Emitted periodically or at end to report aggregate token usage.
    WorkflowStats { tokens_total: usize },
    /// Emitted when the entire workflow finishes successfully.
    WorkflowComplete {
        output_dir: String,
        files: Vec<String>,
        git_hash: Option<String>,
    },
}

pub type TuiSender = mpsc::UnboundedSender<TuiEvent>;
pub type TuiReceiver = mpsc::UnboundedReceiver<TuiEvent>;

pub fn channel() -> (TuiSender, TuiReceiver) {
    mpsc::unbounded_channel()
}
