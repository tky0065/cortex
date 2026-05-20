use std::sync::Arc;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

use crate::agent_bus::AgentBus;
use crate::config::Config;
use crate::tui::events::{Task, TuiEvent, TuiSender, channel};
use crate::workflows::{ExecutionMode, RunOptions, Workflow};

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
    /// Execution mode controlling planning/pause behaviour.
    execution_mode: ExecutionMode,
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
            execution_mode: ExecutionMode::default(),
        }
    }

    /// Set the REPL state reference for session tracking.
    pub fn with_repl_state(mut self, repl_state: Arc<crate::repl::ReplState>) -> Self {
        self.repl_state = Some(repl_state);
        self
    }

    /// Set the execution mode for this run.
    pub fn with_execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = mode;
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
        let run_report_collector = Arc::new(tokio::sync::Mutex::new(
            crate::run_report::RunReportCollector::new(
                self.workflow.name(),
                prompt.clone(),
                &self.config,
            ),
        ));

        // Warn when the project directory is non-empty (except on explicit resume).
        if project_dir.exists() {
            let is_nonempty = std::fs::read_dir(&project_dir)
                .map(|mut d| d.next().is_some())
                .unwrap_or(false);
            if is_nonempty {
                let _ = tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: format!(
                        "WARNING: output directory '{}' already contains files. \
                         Cortex will write new files and may overwrite existing ones. \
                         Use 'cortex resume <dir>' to continue a previous run instead.",
                        project_dir.display()
                    ),
                });
            }
        }

        // Spawn a background task to watch TASKS.md for UI updates.
        spawn_task_watcher(tx.clone(), project_dir.clone(), self.cancel.clone());

        // Create a fresh AgentBus for this workflow run and share it with the REPL.
        let agent_bus = AgentBus::new();
        if let Some(ref repl_state) = self.repl_state {
            *repl_state.agent_bus.write().await = Some(Arc::clone(&agent_bus));
        }

        let log_tx = if verbose {
            let (log_tx, mut log_rx) = channel();
            let log_redactor = crate::secrets::SecretRedactor::from_config_and_env(&self.config);
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
                                let _ = writeln!(
                                    f,
                                    "{}",
                                    format_verbose_log_line(agent, chunk, &log_redactor)
                                );
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("warning: could not open cortex.log: {}", e);
                    }
                }
            });

            Some(log_tx)
        } else {
            None
        };

        let (tee_tx, mut tee_rx) = channel();
        let real_tx = tx.clone();
        let report_collector_for_tee = Arc::clone(&run_report_collector);
        let tee_handle = tokio::spawn(async move {
            while let Some(ev) = tee_rx.recv().await {
                report_collector_for_tee.lock().await.record_event(&ev);
                if let Some(log_tx) = &log_tx {
                    let _ = log_tx.send(ev.clone());
                }
                let _ = real_tx.send(ev);
            }
        });

        let is_auto = auto || self.execution_mode == ExecutionMode::Auto;
        let options = RunOptions {
            auto: is_auto,
            execution_mode: self.execution_mode.clone(),
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
            agent_tools: None,
        };

        let run_completion = tokio::select! {
            result = self.workflow.run(prompt.clone(), options) => RunCompletion::Workflow(result),
            _ = self.cancel.cancelled() => {
                let _ = tee_tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: "Workflow aborted.".into(),
                });
                RunCompletion::Interrupted
            }
        };

        flush_report_events(tee_tx, tee_handle).await;

        match run_completion {
            RunCompletion::Workflow(Ok(())) => {
                {
                    let mut collector = run_report_collector.lock().await;
                    finalize_run_report(
                        &mut collector,
                        &project_dir,
                        &self.config,
                        RunReportOutcome::Success,
                    );
                }
                write_manifest(&project_dir, self.workflow.name(), &prompt, &self.config);
                Ok(())
            }
            RunCompletion::Workflow(Err(e)) => {
                {
                    let mut collector = run_report_collector.lock().await;
                    finalize_run_report(
                        &mut collector,
                        &project_dir,
                        &self.config,
                        RunReportOutcome::Failed(e.to_string()),
                    );
                }
                Err(e)
            }
            RunCompletion::Interrupted => {
                {
                    let mut collector = run_report_collector.lock().await;
                    finalize_run_report(
                        &mut collector,
                        &project_dir,
                        &self.config,
                        RunReportOutcome::Interrupted("Workflow aborted.".to_string()),
                    );
                }
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

fn format_verbose_log_line(
    agent: &str,
    chunk: &str,
    redactor: &crate::secrets::SecretRedactor,
) -> String {
    format!("[{}] {}", agent, redactor.redact_text(chunk))
}

enum RunCompletion {
    Workflow(Result<()>),
    Interrupted,
}

enum RunReportOutcome {
    Success,
    Failed(String),
    Interrupted(String),
}

async fn flush_report_events(tee_tx: TuiSender, tee_handle: tokio::task::JoinHandle<()>) {
    drop(tee_tx);
    match tokio::time::timeout(std::time::Duration::from_secs(2), tee_handle).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("warning: run report event tee failed: {e}"),
        Err(_) => eprintln!("warning: timed out waiting for run report events to flush"),
    }
}

fn finalize_run_report(
    collector: &mut crate::run_report::RunReportCollector,
    project_dir: &std::path::Path,
    config: &Config,
    outcome: RunReportOutcome,
) {
    match outcome {
        RunReportOutcome::Success => collector.finish_success(),
        RunReportOutcome::Failed(message) => collector.finish_error(message),
        RunReportOutcome::Interrupted(message) => collector.finish_interrupted(message),
    }
    if let Err(e) = collector.write_to(project_dir, config) {
        eprintln!("warning: could not write cortex.run.json: {e}");
    }
}

/// Write a `cortex.manifest.json` to the project directory on successful run completion.
/// Failures are non-fatal and silently ignored — the manifest is informational only.
fn write_manifest(project_dir: &std::path::Path, workflow: &str, prompt: &str, config: &Config) {
    use std::collections::HashMap;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut models: HashMap<&str, &str> = HashMap::new();
    let cfg_models = &config.models;
    models.insert("ceo", &cfg_models.ceo);
    models.insert("developer", &cfg_models.developer);
    models.insert("qa", &cfg_models.qa);

    let redactor = crate::secrets::SecretRedactor::from_config_and_env(config);
    let redacted_prompt = redactor.redact_text(prompt);

    let manifest = serde_json::json!({
        "cortex_version": env!("CARGO_PKG_VERSION"),
        "workflow": workflow,
        "provider": config.provider.default,
        "models": models,
        "prompt": redacted_prompt,
        "timestamp_unix": timestamp,
        "verification": [
            "cargo build",
            "cargo test",
            "docker build ."
        ]
    });

    let path = project_dir.join("cortex.manifest.json");
    if let Ok(json) = serde_json::to_string_pretty(&manifest) {
        let _ = std::fs::write(path, json);
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
            if let Some(desc) = line.strip_prefix("- [ ] ") {
                Some(Task {
                    description: desc.to_string(),
                    is_done: false,
                })
            } else {
                line.strip_prefix("- [x] ")
                    .or_else(|| line.strip_prefix("- [X] "))
                    .map(|desc| Task {
                        description: desc.to_string(),
                        is_done: true,
                    })
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
    use std::sync::Arc;

    use super::{
        RunReportOutcome, default_project_dir, finalize_run_report, flush_report_events,
        write_manifest,
    };
    use crate::config::Config;
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

    #[test]
    fn manifest_redacts_prompt_secrets() {
        let dir =
            std::env::temp_dir().join(format!("cortex_manifest_redact_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut config = Config::default();
        config.api_keys.openai = Some("sk-test-manifest-secret".to_string());

        write_manifest(
            &dir,
            "dev",
            "build a tool with key sk-test-manifest-secret",
            &config,
        );

        let content = std::fs::read_to_string(dir.join("cortex.manifest.json")).unwrap();
        assert!(content.contains("[REDACTED]"));
        assert!(!content.contains("sk-test-manifest-secret"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verbose_log_line_redacts_secrets() {
        let redactor = crate::secrets::SecretRedactor::from_values(["log-secret-123456"]);
        let line =
            super::format_verbose_log_line("developer", "received log-secret-123456", &redactor);

        assert_eq!(line, "[developer] received [REDACTED]");
        assert!(!line.contains("log-secret-123456"));
    }

    #[test]
    fn finalized_report_writes_success_status() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_orchestrator_report_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config::default();
        let mut collector = crate::run_report::RunReportCollector::new("dev", "build", &config);
        finalize_run_report(&mut collector, &dir, &config, RunReportOutcome::Success);

        let content = std::fs::read_to_string(dir.join("cortex.run.json")).unwrap();
        assert!(content.contains("\"status\": \"success\""));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finalized_report_writes_failed_status() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_orchestrator_report_failed_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config::default();
        let mut collector = crate::run_report::RunReportCollector::new("dev", "build", &config);
        finalize_run_report(
            &mut collector,
            &dir,
            &config,
            RunReportOutcome::Failed("provider failed".to_string()),
        );

        let content = std::fs::read_to_string(dir.join("cortex.run.json")).unwrap();
        assert!(content.contains("\"status\": \"failed\""));
        assert!(content.contains("provider failed"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn flush_report_events_drains_queued_events_before_returning() {
        let (tee_tx, mut tee_rx) = channel();
        let recorded = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let recorded_for_tee = Arc::clone(&recorded);

        let tee_handle = tokio::spawn(async move {
            while let Some(ev) = tee_rx.recv().await {
                if let TuiEvent::TokenChunk { chunk, .. } = ev {
                    recorded_for_tee.lock().await.push(chunk);
                }
            }
        });

        tee_tx
            .send(TuiEvent::TokenChunk {
                agent: "sentinel".to_string(),
                chunk: "queued-before-finalize".to_string(),
            })
            .unwrap();

        flush_report_events(tee_tx, tee_handle).await;

        assert_eq!(recorded.lock().await.as_slice(), ["queued-before-finalize"]);
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
