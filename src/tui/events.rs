use tokio::sync::mpsc;

/// Events sent from the orchestrator to the TUI renderer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TuiEvent {
    WorkflowStarted {
        workflow: String,
        agents: Vec<String>,
    },
    AgentStarted {
        agent: String,
    },
    AgentProgress {
        agent: String,
        message: String,
    },
    AgentSummary {
        agent: String,
        summary: String,
    },
    TokenChunk {
        agent: String,
        chunk: String,
    },
    AgentDone {
        agent: String,
    },
    PhaseComplete {
        phase: String,
    },
    Error {
        agent: String,
        message: String,
    },
    /// Emitted when the workflow pauses waiting for user confirmation (interactive mode).
    InteractivePause {
        message: String,
    },
    /// Emitted by an agent that needs a clarification answer from the user.
    UserQuestion {
        agent: String,
        question: String,
    },
    /// Emitted by the REPL `/continue` command to resume a paused workflow.
    Resume,
    /// Emitted periodically or at end to report aggregate token usage.
    WorkflowStats {
        tokens_total: usize,
    },
    /// Emitted when the entire workflow finishes successfully.
    WorkflowComplete {
        output_dir: String,
        files: Vec<String>,
        git_hash: Option<String>,
    },
    /// Open the interactive provider picker popup.
    OpenProviderPicker,
    /// Open provider picker and then auth-method selection.
    OpenConnectProviderPicker,
    /// Open the interactive model picker popup.
    OpenModelPicker,
    /// Open the interactive resume picker popup.
    OpenResumePicker,
    /// Open the interactive skill browser/manager popup.
    OpenSkillPicker,
    /// Emitted when a session is selected from the resume picker.
    ResumeSelected {
        session_id: String,
    },
    /// Fired (from a background task) when the model list for a provider has been fetched.
    ModelsLoaded {
        provider: String,
        models: Vec<String>,
    },
    /// Show a browser authorization URL for an account connection flow.
    AuthUrl {
        provider: String,
        url: String,
        message: String,
    },
    /// Close any matching authorization popup after the flow completes.
    AuthComplete {
        provider: String,
        message: String,
    },
    /// Fired when the skills.sh leaderboard has been fetched.
    SkillsCatalogLoaded {
        skills: Vec<crate::skills::RemoteSkill>,
    },
    /// Fired when a skills.sh search result has been fetched.
    SkillSearchLoaded {
        query: String,
        skills: Vec<crate::skills::RemoteSkill>,
    },
    /// Fired when the skill picker needs to show a non-fatal loading/apply error.
    SkillPickerError {
        message: String,
    },
    /// Emitted after a file is written by an agent; old_content is None for new files.
    FileWritten {
        agent: String,
        path: String,
        old_content: Option<String>,
        new_content: String,
    },
    ClearLogs,
    SetLogFilter {
        agent: Option<String>,
    },
}

pub type TuiSender = mpsc::UnboundedSender<TuiEvent>;
pub type TuiReceiver = mpsc::UnboundedReceiver<TuiEvent>;

pub fn channel() -> (TuiSender, TuiReceiver) {
    mpsc::unbounded_channel()
}
