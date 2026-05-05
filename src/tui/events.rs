use tokio::sync::mpsc;

/// A single task in a workflow.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub description: String,
    pub is_done: bool,
}

/// Events sent from the orchestrator to the TUI renderer.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TuiEvent {
    TasksUpdated {
        tasks: Vec<Task>,
    },
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
    /// Emitted after an agent completes — waits for user approval or feedback before next agent.
    AgentReviewRequest {
        agent: String,
        summary: String,
    },
    /// Emitted when an agent invokes a tool (file read/write, bash, web search…).
    /// Displayed as a structured labeled block in the agent panel (Claude Code style).
    AgentToolCall {
        agent: String,
        /// Short tool name: "read_file", "write_file", "bash", "web_search", "scan", "read_input"
        tool: String,
        /// Tool argument shown to the user (file path, command, query…)
        label: String,
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
    /// Emitted periodically to update status bar info (CWD, Git branch).
    SystemInfoUpdate {
        cwd: String,
        git_info: Option<String>,
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
    /// Emitted after a provider or model change so the status bar never shows UNKNOWN.
    ProviderChanged {
        provider: String,
        model: String,
    },
    /// Replaces the agent's stream buffer with a clean final reply (used to fix duplication).
    AgentReplaceBuffer {
        agent: String,
        content: String,
    },
    /// Emitted when the user cycles execution mode (e.g. via Shift+Tab).
    ModeChanged(crate::workflows::ExecutionMode),
    /// Emitted by the planner agent once PLAN.md has been written.
    PlanGenerated {
        path: String,
    },
    /// Pre-fills the command input bar so the user can type arguments.
    SetInputBar {
        value: String,
    },
    /// Reload the launcher panel (e.g. after creating a new workflow or agent).
    LauncherRefresh,
    /// Emitted when ESC ESC interrupts the running workflow or chat generation.
    WorkflowInterrupted {
        message: String,
    },
}

pub type TuiSender = mpsc::UnboundedSender<TuiEvent>;
pub type TuiReceiver = mpsc::UnboundedReceiver<TuiEvent>;

pub fn channel() -> (TuiSender, TuiReceiver) {
    mpsc::unbounded_channel()
}
