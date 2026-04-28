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
    /// Open the interactive provider picker popup.
    OpenProviderPicker,
    /// Open the interactive model picker popup.
    OpenModelPicker,
    /// Open the interactive resume picker popup.
    OpenResumePicker,
    /// Emitted when a session is selected from the resume picker.
    ResumeSelected { session_id: String },
    /// Fired (from a background task) when the model list for a provider has been fetched.
    ModelsLoaded { provider: String, models: Vec<String> },
}

pub type TuiSender = mpsc::UnboundedSender<TuiEvent>;
pub type TuiReceiver = mpsc::UnboundedReceiver<TuiEvent>;

pub fn channel() -> (TuiSender, TuiReceiver) {
    mpsc::unbounded_channel()
}
