use std::{io::Write, sync::Arc};

use anyhow::Result;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::agent_bus::AgentBus;
use crate::budget::{BudgetLimits, BudgetState, BudgetStatus};
use crate::config::Config;
use crate::tui::events::{Task, TuiEvent, TuiSender, channel};
use crate::workflows::{ExecutionMode, RunOptions, Workflow};

type FlushSender = mpsc::UnboundedSender<oneshot::Sender<()>>;
type FlushReceiver = mpsc::UnboundedReceiver<oneshot::Sender<()>>;

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
        let project_dir = project_dir.unwrap_or_else(|| {
            default_project_dir(
                self.workflow.name(),
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )
        });
        self.run_with_project_dir_and_resume(prompt, auto, verbose, tx, project_dir, None)
            .await
    }

    pub async fn resume_with_project_dir(
        &self,
        verbose: bool,
        tx: Option<TuiSender>,
        project_dir: std::path::PathBuf,
    ) -> Result<()> {
        let checkpoint_path = crate::checkpoint::Checkpoint::checkpoint_path(&project_dir);
        if !checkpoint_path.exists() {
            anyhow::bail!(
                "structured resume requires cortex.checkpoint.json in {}",
                project_dir.display()
            );
        }

        let checkpoint = crate::checkpoint::Checkpoint::load(&project_dir)?;
        if !crate::checkpoint::Checkpoint::is_resume_supported_for(&checkpoint.workflow) {
            anyhow::bail!(
                "structured resume currently supports dev; checkpoint workflow was {}",
                checkpoint.workflow
            );
        }
        if checkpoint.workflow != self.workflow.name() {
            anyhow::bail!(
                "checkpoint workflow mismatch: checkpoint={}, requested={}",
                checkpoint.workflow,
                self.workflow.name()
            );
        }

        let conflicts = checkpoint.validate_files(&project_dir)?;
        if !conflicts.is_empty() {
            anyhow::bail!("{}", format_checkpoint_conflicts(&conflicts));
        }

        self.run_with_project_dir_and_resume(
            checkpoint.prompt.clone(),
            true,
            verbose,
            tx,
            project_dir,
            Some(crate::workflows::ResumeContext {
                checkpoint,
                conflicts,
            }),
        )
        .await
    }

    async fn run_with_project_dir_and_resume(
        &self,
        prompt: String,
        auto: bool,
        verbose: bool,
        tx: Option<TuiSender>,
        project_dir: std::path::PathBuf,
        resume: Option<crate::workflows::ResumeContext>,
    ) -> Result<()> {
        // Resolve the primary event sender (TUI or throw-away).
        let tx = tx.unwrap_or_else(|| channel().0);
        let run_report_collector = Arc::new(tokio::sync::Mutex::new(
            crate::run_report::RunReportCollector::new(
                self.workflow.name(),
                prompt.clone(),
                &self.config,
            ),
        ));

        // Warn when the project directory is non-empty (except on explicit resume).
        if resume.is_none() && project_dir.exists() {
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
        let task_watcher_cancel = self.cancel.child_token();
        let task_watcher_handle =
            spawn_task_watcher(tx.clone(), project_dir.clone(), task_watcher_cancel.clone());

        // Create a fresh AgentBus for this workflow run and share it with the REPL.
        let agent_bus = AgentBus::new();
        if let Some(ref repl_state) = self.repl_state {
            *repl_state.agent_bus.write().await = Some(Arc::clone(&agent_bus));
        }

        let (log_tx, log_flush_tx) = if verbose {
            let (log_tx, flush_tx) =
                spawn_verbose_log_writer(&self.config, std::path::PathBuf::from("cortex.log"));
            (Some(log_tx), Some(flush_tx))
        } else {
            (None, None)
        };

        let (tee_tx, mut tee_rx) = channel();
        let (report_flush_tx, mut report_flush_rx): (FlushSender, FlushReceiver) =
            mpsc::unbounded_channel();
        let real_tx = tx.clone();
        let report_collector_for_tee = Arc::clone(&run_report_collector);
        let budget_state = Arc::new(tokio::sync::Mutex::new(BudgetState::new(
            self.config.provider.default.clone(),
            self.config.models.developer.clone(),
            BudgetLimits {
                max_tokens_per_run: self.config.limits.max_tokens_per_run,
                max_estimated_cost_usd: self.config.limits.max_estimated_cost_usd,
            },
        )));
        let budget_state_for_tee = Arc::clone(&budget_state);
        let cancel_for_budget = self.cancel.clone();
        let _report_tee_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(ev) = tee_rx.recv() => {
                        handle_report_event(
                            ev,
                            &report_collector_for_tee,
                            &budget_state_for_tee,
                            &cancel_for_budget,
                            log_tx.as_ref(),
                            &real_tx,
                        ).await;
                    }
                    Some(ack) = report_flush_rx.recv() => {
                        while let Ok(ev) = tee_rx.try_recv() {
                            handle_report_event(
                                ev,
                                &report_collector_for_tee,
                                &budget_state_for_tee,
                                &cancel_for_budget,
                                log_tx.as_ref(),
                                &real_tx,
                            ).await;
                        }
                        let _ = ack.send(());
                    }
                    else => break,
                }
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
            resume,
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
        let mut run_completion = match run_completion {
            RunCompletion::Workflow(Ok(())) if self.cancel.is_cancelled() => {
                let _ = tee_tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: "Workflow aborted.".into(),
                });
                RunCompletion::Interrupted
            }
            other => other,
        };

        task_watcher_cancel.cancel();
        let _ = task_watcher_handle.await;
        emit_tasks_snapshot(&tx, &project_dir).await;

        flush_ack(&report_flush_tx, "run report events").await;
        if let Some(log_flush_tx) = &log_flush_tx {
            flush_ack(log_flush_tx, "verbose log").await;
        }

        if matches!(run_completion, RunCompletion::Workflow(Ok(()))) {
            let snapshot = budget_state.lock().await.snapshot();
            if snapshot.status == BudgetStatus::Exceeded || self.cancel.is_cancelled() {
                run_completion = RunCompletion::Interrupted;
            }
        }

        match run_completion {
            RunCompletion::Workflow(Ok(())) => {
                {
                    let mut collector = run_report_collector.lock().await;
                    let snapshot = budget_state.lock().await.snapshot();
                    collector.apply_budget_snapshot(&snapshot);
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
                update_checkpoint_status(
                    &project_dir,
                    &self.config,
                    crate::checkpoint::CheckpointStatus::Failed,
                );
                {
                    let mut collector = run_report_collector.lock().await;
                    let snapshot = budget_state.lock().await.snapshot();
                    collector.apply_budget_snapshot(&snapshot);
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
                update_checkpoint_status(
                    &project_dir,
                    &self.config,
                    crate::checkpoint::CheckpointStatus::Interrupted,
                );
                {
                    let mut collector = run_report_collector.lock().await;
                    let snapshot = budget_state.lock().await.snapshot();
                    collector.apply_budget_snapshot(&snapshot);
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

fn format_checkpoint_conflicts(conflicts: &[crate::checkpoint::CheckpointConflict]) -> String {
    let mut lines = vec!["checkpoint conflicts prevent structured resume:".to_string()];
    for conflict in conflicts {
        let message = match conflict.conflict_type {
            crate::checkpoint::CheckpointConflictType::FileModified => {
                format!(
                    "tracked file was modified since checkpoint: {}",
                    conflict.message
                )
            }
            _ => conflict.message.clone(),
        };
        match &conflict.path {
            Some(path) => lines.push(format!("- {}: {}", path, message)),
            None => lines.push(format!("- {}", message)),
        }
    }
    lines.join("\n")
}

fn update_checkpoint_status(
    project_dir: &std::path::Path,
    config: &Config,
    status: crate::checkpoint::CheckpointStatus,
) {
    let Ok(mut checkpoint) = crate::checkpoint::Checkpoint::load(project_dir) else {
        return;
    };
    checkpoint.status = status;
    checkpoint.updated_at_unix_ms = crate::checkpoint::now_unix_ms();
    if let Err(e) = checkpoint.write_to(project_dir, config) {
        eprintln!("warning: could not update cortex.checkpoint.json: {e}");
    }
}

fn format_verbose_log_line(
    agent: &str,
    chunk: &str,
    redactor: &crate::secrets::SecretRedactor,
) -> String {
    format!("[{}] {}", agent, redactor.redact_text(chunk))
}

fn spawn_verbose_log_writer(
    config: &Config,
    log_path: std::path::PathBuf,
) -> (TuiSender, FlushSender) {
    let (log_tx, mut log_rx) = channel();
    let (log_flush_tx, mut log_flush_rx): (FlushSender, FlushReceiver) = mpsc::unbounded_channel();
    let log_redactor = crate::secrets::SecretRedactor::from_config_and_env(config);

    tokio::spawn(async move {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path);
        match file {
            Ok(mut f) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let _ = writeln!(f, "=== cortex session (unix={}) ===", ts);

                loop {
                    tokio::select! {
                        Some(ev) = log_rx.recv() => {
                            write_verbose_log_event(&mut f, ev, &log_redactor);
                        }
                        Some(ack) = log_flush_rx.recv() => {
                            while let Ok(ev) = log_rx.try_recv() {
                                write_verbose_log_event(&mut f, ev, &log_redactor);
                            }
                            let _ = f.flush();
                            let _ = ack.send(());
                        }
                        else => break,
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: could not open cortex.log: {}", e);
            }
        }
    });

    (log_tx, log_flush_tx)
}

fn write_verbose_log_event(
    f: &mut std::fs::File,
    ev: TuiEvent,
    redactor: &crate::secrets::SecretRedactor,
) {
    if let TuiEvent::TokenChunk { agent, chunk } = ev {
        let _ = writeln!(f, "{}", format_verbose_log_line(&agent, &chunk, redactor));
    }
}

async fn handle_report_event(
    ev: TuiEvent,
    collector: &Arc<tokio::sync::Mutex<crate::run_report::RunReportCollector>>,
    budget_state: &Arc<tokio::sync::Mutex<BudgetState>>,
    cancel: &CancellationToken,
    log_tx: Option<&TuiSender>,
    real_tx: &TuiSender,
) {
    if let TuiEvent::WorkflowStats { tokens_total } = &ev {
        let snapshot = {
            let mut budget = budget_state.lock().await;
            budget.record_tokens_total(*tokens_total as u64);
            budget.snapshot()
        };
        collector.lock().await.apply_budget_snapshot(&snapshot);
        if snapshot.status == BudgetStatus::Exceeded {
            let _ = real_tx.send(TuiEvent::WorkflowInterrupted {
                message: snapshot
                    .exceeded_reason
                    .clone()
                    .unwrap_or_else(|| "budget exceeded".to_string()),
            });
            cancel.cancel();
        }
    }

    collector.lock().await.record_event(&ev);
    if let Some(log_tx) = log_tx {
        let _ = log_tx.send(ev.clone());
    }
    let _ = real_tx.send(ev);
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

async fn flush_ack(flush_tx: &FlushSender, label: &str) {
    let (ack_tx, ack_rx) = oneshot::channel();
    if flush_tx.send(ack_tx).is_err() {
        return;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => eprintln!("warning: {label} flush channel closed"),
        Err(_) => eprintln!("warning: timed out waiting for {label} to flush"),
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
fn spawn_task_watcher(
    tx: TuiSender,
    project_dir: std::path::PathBuf,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
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

            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(2)) => {}
            }
        }
    })
}

async fn emit_tasks_snapshot(tx: &TuiSender, project_dir: &std::path::Path) {
    let tasks_path = project_dir.join("TASKS.md");
    if let Ok(content) = tokio::fs::read_to_string(&tasks_path).await {
        let tasks = parse_tasks(&content);
        let _ = tx.send(TuiEvent::TasksUpdated { tasks });
    }
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
    use tokio::sync::{mpsc, oneshot};

    use super::{
        FlushSender, RunReportOutcome, default_project_dir, finalize_run_report, flush_ack,
        spawn_verbose_log_writer, update_checkpoint_status, write_manifest,
    };
    use crate::config::Config;
    use crate::tui::events::{TuiEvent, channel};
    use crate::workflows::{RunOptions, Workflow};
    use anyhow::Result;
    use async_trait::async_trait;

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

    #[test]
    fn update_checkpoint_status_persists_terminal_status() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_status_update_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config::default();
        let checkpoint = crate::checkpoint::Checkpoint::new("run-1", "dev", "build", &config);
        checkpoint.write_to(&dir, &config).unwrap();

        update_checkpoint_status(&dir, &config, crate::checkpoint::CheckpointStatus::Failed);

        let checkpoint = crate::checkpoint::Checkpoint::load(&dir).unwrap();
        assert_eq!(
            checkpoint.status,
            crate::checkpoint::CheckpointStatus::Failed
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn update_checkpoint_status_ignores_missing_checkpoint() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_status_missing_{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config::default();
        update_checkpoint_status(
            &dir,
            &config,
            crate::checkpoint::CheckpointStatus::Interrupted,
        );

        assert!(!crate::checkpoint::Checkpoint::checkpoint_path(&dir).exists());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn flush_report_events_drains_queued_events_before_returning() {
        let recorded = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let (tee_tx, flush_tx) = spawn_recording_report_tee(Arc::clone(&recorded));

        tee_tx
            .send(TuiEvent::TokenChunk {
                agent: "sentinel".to_string(),
                chunk: "queued-before-finalize".to_string(),
            })
            .unwrap();

        flush_ack(&flush_tx, "test report events").await;

        assert_eq!(recorded.lock().await.as_slice(), ["queued-before-finalize"]);
    }

    #[tokio::test]
    async fn flush_report_events_does_not_wait_for_sender_clones_to_close() {
        let recorded = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let (tee_tx, flush_tx) = spawn_recording_report_tee(Arc::clone(&recorded));
        let held_sender = tee_tx.clone();

        tee_tx
            .send(TuiEvent::TokenChunk {
                agent: "sentinel".to_string(),
                chunk: "queued-with-held-sender".to_string(),
            })
            .unwrap();

        flush_ack(&flush_tx, "test report events").await;

        assert_eq!(
            recorded.lock().await.as_slice(),
            ["queued-with-held-sender"]
        );
        held_sender
            .send(TuiEvent::TokenChunk {
                agent: "sentinel".to_string(),
                chunk: "held-sender-still-open".to_string(),
            })
            .unwrap();
    }

    #[tokio::test]
    async fn flush_log_events_writes_queued_events_before_returning() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_orchestrator_log_flush_{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let log_path = dir.join("cortex.log");

        let config = Config::default();
        let (log_tx, log_flush_tx) = spawn_verbose_log_writer(&config, log_path.clone());
        log_tx
            .send(TuiEvent::TokenChunk {
                agent: "developer".to_string(),
                chunk: "queued log line".to_string(),
            })
            .unwrap();

        flush_ack(&log_flush_tx, "test verbose log").await;

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[developer] queued log line"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn resume_without_checkpoint_fails_before_workflow_execution() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_resume_missing_checkpoint_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(crate::workflows::get_workflow("dev").unwrap(), config);
        let err = orch
            .resume_with_project_dir(false, None, dir.clone())
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("structured resume requires cortex.checkpoint.json"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn resume_with_modified_tracked_file_fails_before_workflow_execution() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_resume_modified_checkpoint_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("specs.md"), "initial").unwrap();

        let config = Config::default();
        let mut checkpoint = crate::checkpoint::Checkpoint::new("run-1", "dev", "build", &config);
        checkpoint
            .record_file("pm", "specs-ready", "specs.md", "created", &dir)
            .unwrap();
        checkpoint.write_to(&dir, &config).unwrap();
        std::fs::write(dir.join("specs.md"), "changed").unwrap();

        let orch = super::Orchestrator::new(
            crate::workflows::get_workflow("dev").unwrap(),
            Arc::new(config),
        );
        let err = orch
            .resume_with_project_dir(false, None, dir.clone())
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("tracked file was modified since checkpoint"));
        assert!(err.contains("specs.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn workflow_ok_after_cancellation_marks_checkpoint_interrupted_without_manifest() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_cancelled_ok_checkpoint_{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(CancelThenOkWorkflow), Arc::clone(&config));
        orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir.clone()))
            .await
            .unwrap();

        let checkpoint = crate::checkpoint::Checkpoint::load(&dir).unwrap();
        assert_eq!(
            checkpoint.status,
            crate::checkpoint::CheckpointStatus::Interrupted
        );
        assert!(!dir.join("cortex.manifest.json").exists());

        let run_report = std::fs::read_to_string(dir.join("cortex.run.json")).unwrap();
        assert!(run_report.contains("\"status\": \"interrupted\""));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn cancelled_run_artifacts_remain_readable() {
        let dir = temp_test_dir("cortex_cancelled_artifacts");
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(FileThenCancelWorkflow), config);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir.clone())),
        )
        .await
        .expect("cancelled artifact workflow deadlocked")
        .unwrap();

        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "interrupted");
        assert_eq!(report["files"][0]["path"], "partial.txt");

        let checkpoint = crate::checkpoint::Checkpoint::load(&dir).unwrap();
        assert_eq!(
            checkpoint.status,
            crate::checkpoint::CheckpointStatus::Interrupted
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn stress_helpers_create_isolated_project_dir_and_parse_report_status() {
        let dir = temp_test_dir("cortex_stress_helper");
        let config = Config::default();
        let mut collector = crate::run_report::RunReportCollector::new("dev", "build", &config);
        finalize_run_report(
            &mut collector,
            &dir,
            &config,
            RunReportOutcome::Interrupted("stop".into()),
        );

        assert_eq!(read_run_report_status(&dir), "interrupted");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn orchestrator_cancellation_interrupts_slow_workflow() {
        let dir = temp_test_dir("cortex_cancel_slow_workflow");
        let (in_flight_tx, in_flight_rx) = oneshot::channel();
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(
            Box::new(SlowUntilCancelledWorkflow {
                in_flight: std::sync::Mutex::new(Some(in_flight_tx)),
            }),
            config,
        );
        let cancel = orch.cancel_token();

        let run = tokio::spawn({
            let dir = dir.clone();
            async move {
                orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir))
                    .await
            }
        });

        match tokio::time::timeout(std::time::Duration::from_secs(1), in_flight_rx).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => {
                cancel.cancel();
                run.abort();
                let _ = run.await;
                let _ = std::fs::remove_dir_all(&dir);
                panic!("workflow dropped startup signal");
            }
            Err(_) => {
                cancel.cancel();
                run.abort();
                let _ = run.await;
                let _ = std::fs::remove_dir_all(&dir);
                panic!("workflow did not start");
            }
        }
        cancel.cancel();

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .expect("orchestrator deadlocked after cancellation")
            .expect("run task panicked");

        result.unwrap();
        assert_eq!(read_run_report_status(&dir), "interrupted");
        let report = read_run_report_json(&dir);
        assert_eq!(report["failure"]["failure_type"], "interrupted");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn orchestrator_failure_does_not_deadlock_event_stream() {
        let dir = temp_test_dir("cortex_failure_event_stream");
        let (tx, rx) = channel();
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(FailingWorkflow), config);

        let run = orch.run_with_project_dir(
            "build".to_string(),
            true,
            false,
            Some(tx),
            Some(dir.clone()),
        );

        let err = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .expect("orchestrator deadlocked on workflow failure")
            .unwrap_err()
            .to_string();
        assert!(err.contains("intentional workflow failure"));

        let events = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drain_events_until_closed(rx),
        )
        .await
        .expect("event stream did not close after failure");
        assert!(events.iter().any(|event| matches!(event, TuiEvent::Error { agent, message } if agent == "failing" && message.contains("intentional workflow failure"))));
        assert_eq!(read_run_report_status(&dir), "failed");
        assert_eq!(
            read_run_report_json(&dir)["failure"]["failure_type"],
            "agent_error"
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn parallel_worker_failure_cancels_or_joins_siblings() {
        let dir = temp_test_dir("cortex_parallel_worker_failure");
        let (tx, rx) = channel();
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(ParallelWorkerFailureWorkflow), config);

        let err = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir(
                "build".to_string(),
                true,
                false,
                Some(tx),
                Some(dir.clone()),
            ),
        )
        .await
        .expect("parallel workflow deadlocked after worker failure")
        .unwrap_err()
        .to_string();

        let events = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drain_events_until_closed(rx),
        )
        .await
        .expect("event stream did not close after parallel worker failure");
        assert!(events.iter().any(|event| matches!(event, TuiEvent::Error { agent, message } if agent == "worker-2" && message.contains("worker 2 failed"))));

        assert!(err.contains("worker 2 failed"));
        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "failed");
        assert_eq!(report["metrics"]["agent_count"], 4);
        assert_eq!(report["failure"]["failure_type"], "agent_error");
        assert_eq!(report["failure"]["agent"], "worker-2");

        let agents = report["agents"].as_array().unwrap();
        for worker_id in 0..4 {
            let agent_name = format!("worker-{worker_id}");
            let agent = agents
                .iter()
                .find(|agent| agent["agent"] == agent_name)
                .unwrap_or_else(|| panic!("missing report record for {agent_name}"));

            if worker_id == 2 {
                assert_eq!(agent["status"], "error");
                assert_eq!(agent["errors"], serde_json::json!(["worker 2 failed"]));
            } else {
                assert_eq!(agent["status"], "done");
                assert!(agent["errors"].as_array().unwrap().is_empty());
            }
            assert_eq!(agent["token_chunks"], 1);
            assert!(agent["output_chars"].as_u64().unwrap() > 0);
        }

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn orchestrator_survives_dropped_event_receiver() {
        let dir = temp_test_dir("cortex_dropped_receiver");
        let (tx, rx) = channel();
        drop(rx);

        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(DroppedReceiverWorkflow), config);

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir(
                "build".to_string(),
                true,
                false,
                Some(tx),
                Some(dir.clone()),
            ),
        )
        .await
        .expect("orchestrator deadlocked when event receiver was dropped");

        result.unwrap();
        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "success");
        let agent = report["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["agent"] == "dropped_receiver")
            .expect("run report did not collect dropped_receiver events");
        assert_eq!(agent["token_chunks"], 25);
        assert!(agent["output_chars"].as_u64().unwrap() > 0);
        assert_eq!(agent["status"], "done");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn orchestrator_emits_final_tasks_state_before_shutdown() {
        let dir = temp_test_dir("cortex_final_tasks_state");
        let (tx, rx) = channel();
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(WritesTasksWorkflow), config);

        orch.run_with_project_dir(
            "build".to_string(),
            true,
            false,
            Some(tx),
            Some(dir.clone()),
        )
        .await
        .unwrap();

        let events = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            drain_events_until_closed(rx),
        )
        .await
        .expect("event stream did not close after task-writing workflow");

        let final_tasks = events
            .iter()
            .rev()
            .find_map(|event| match event {
                TuiEvent::TasksUpdated { tasks } => Some(tasks),
                _ => None,
            })
            .expect("missing final task state");
        assert_eq!(final_tasks.len(), 2);
        assert_eq!(final_tasks[0].description, "write final state");
        assert!(final_tasks[0].is_done);
        assert_eq!(final_tasks[1].description, "notify ui");
        assert!(!final_tasks[1].is_done);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn parallel_event_burst_preserves_final_state() {
        let dir = temp_test_dir("cortex_parallel_event_burst");
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(ParallelEventBurstWorkflow), config);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir.clone())),
        )
        .await
        .expect("parallel event burst deadlocked")
        .unwrap();

        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "success");
        assert_eq!(report["metrics"]["agent_count"], 10);
        assert_eq!(report["metrics"]["token_chunks_total"], 100);
        assert!(report["metrics"]["output_chars_total"].as_u64().unwrap() > 0);

        let agents = report["agents"].as_array().unwrap();
        for worker_id in 0..10 {
            let agent_name = format!("burst-{worker_id}");
            let agent = agents
                .iter()
                .find(|agent| agent["agent"] == agent_name)
                .unwrap_or_else(|| panic!("missing report record for {agent_name}"));
            let expected_output_chars: usize = (0..10)
                .map(|chunk_id| format!("worker={worker_id} chunk={chunk_id}").len())
                .sum();

            assert_eq!(agent["status"], "done");
            assert_eq!(agent["token_chunks"], 10);
            assert!(agent["errors"].as_array().unwrap().is_empty());
            assert_eq!(agent["output_chars"], expected_output_chars);
        }

        let agent_done_count = report["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["event_type"] == "agent_done")
            .count();
        assert_eq!(agent_done_count, 10);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn token_budget_exceeded_interrupts_run_and_writes_report() {
        let project_dir = temp_test_dir("cortex_budget_test");
        let mut config = Config::default();
        config.limits.max_tokens_per_run = 10;
        config.limits.max_estimated_cost_usd = 0.0;

        let orchestrator = super::Orchestrator::new(Box::new(StatsWorkflow), Arc::new(config));

        orchestrator
            .run_with_project_dir(
                "budget test".to_string(),
                true,
                false,
                None,
                Some(project_dir.clone()),
            )
            .await
            .unwrap();

        let report = read_run_report_json(&project_dir);

        assert_eq!(report["status"], "interrupted");
        assert_eq!(report["metrics"]["budget_status"], "exceeded");
        assert_eq!(
            report["metrics"]["budget_exceeded_reason"],
            "token budget exceeded: 11 > 10"
        );

        let _ = std::fs::remove_dir_all(project_dir);
    }

    #[tokio::test]
    async fn token_budget_exceeded_after_immediate_success_is_interrupted() {
        let project_dir = temp_test_dir("cortex_budget_race_test");
        let mut config = Config::default();
        config.limits.max_tokens_per_run = 10;
        config.limits.max_estimated_cost_usd = 0.0;

        let orchestrator =
            super::Orchestrator::new(Box::new(ImmediateStatsWorkflow), Arc::new(config));

        orchestrator
            .run_with_project_dir(
                "budget race test".to_string(),
                true,
                false,
                None,
                Some(project_dir.clone()),
            )
            .await
            .unwrap();

        let report = read_run_report_json(&project_dir);

        assert_eq!(report["status"], "interrupted");
        assert_eq!(report["metrics"]["budget_status"], "exceeded");
        assert_eq!(
            report["metrics"]["budget_exceeded_reason"],
            "token budget exceeded: 11 > 10"
        );

        let _ = std::fs::remove_dir_all(project_dir);
    }

    struct StatsWorkflow;

    #[async_trait]
    impl Workflow for StatsWorkflow {
        fn name(&self) -> &str {
            "stats"
        }

        fn description(&self) -> &str {
            "stats workflow"
        }

        async fn run(&self, _prompt: String, opts: RunOptions) -> Result<()> {
            let _ = opts.tx.send(TuiEvent::WorkflowStarted {
                workflow: "stats".to_string(),
                agents: vec!["developer".to_string()],
            });
            let _ = opts.tx.send(TuiEvent::WorkflowStats { tokens_total: 11 });
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Ok(())
        }
    }

    struct ImmediateStatsWorkflow;

    #[async_trait]
    impl Workflow for ImmediateStatsWorkflow {
        fn name(&self) -> &str {
            "stats"
        }

        fn description(&self) -> &str {
            "stats workflow"
        }

        async fn run(&self, _prompt: String, opts: RunOptions) -> Result<()> {
            let _ = opts.tx.send(TuiEvent::WorkflowStarted {
                workflow: "stats".to_string(),
                agents: vec!["developer".to_string()],
            });
            let _ = opts.tx.send(TuiEvent::WorkflowStats { tokens_total: 11 });
            Ok(())
        }
    }

    struct ParallelEventBurstWorkflow;

    #[async_trait]
    impl Workflow for ParallelEventBurstWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "parallel event burst workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            let mut handles = Vec::new();
            for worker_id in 0..10 {
                let tx = options.tx.clone();
                handles.push(tokio::spawn(async move {
                    let agent = format!("burst-{worker_id}");
                    tx.send(TuiEvent::AgentStarted {
                        agent: agent.clone(),
                    })
                    .ok();
                    for chunk_id in 0..10 {
                        tx.send(TuiEvent::TokenChunk {
                            agent: agent.clone(),
                            chunk: format!("worker={worker_id} chunk={chunk_id}"),
                        })
                        .ok();
                    }
                    tx.send(TuiEvent::AgentDone { agent }).ok();
                }));
            }

            for handle in handles {
                handle.await.expect("burst worker panicked");
            }
            Ok(())
        }
    }

    struct FailingWorkflow;

    #[async_trait]
    impl Workflow for FailingWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "failing test workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            options
                .tx
                .send(TuiEvent::AgentStarted {
                    agent: "failing".to_string(),
                })
                .ok();
            options
                .tx
                .send(TuiEvent::Error {
                    agent: "failing".to_string(),
                    message: "intentional workflow failure".to_string(),
                })
                .ok();
            anyhow::bail!("intentional workflow failure")
        }
    }

    struct ParallelWorkerFailureWorkflow;

    #[async_trait]
    impl Workflow for ParallelWorkerFailureWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "parallel worker failure workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            let mut handles = Vec::new();
            for worker_id in 0..4 {
                let tx = options.tx.clone();
                handles.push(tokio::spawn(async move {
                    let agent = format!("worker-{worker_id}");
                    tx.send(TuiEvent::AgentStarted {
                        agent: agent.clone(),
                    })
                    .ok();
                    tx.send(TuiEvent::TokenChunk {
                        agent: agent.clone(),
                        chunk: format!("worker {worker_id} started"),
                    })
                    .ok();
                    if worker_id == 2 {
                        tx.send(TuiEvent::Error {
                            agent,
                            message: "worker 2 failed".to_string(),
                        })
                        .ok();
                        anyhow::bail!("worker 2 failed");
                    }
                    tx.send(TuiEvent::AgentDone { agent }).ok();
                    Ok::<(), anyhow::Error>(())
                }));
            }

            let mut failure = None;
            for handle in handles {
                match handle.await {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => failure = Some(err),
                    Err(err) => failure = Some(anyhow::anyhow!("worker join failed: {err}")),
                }
            }

            if let Some(err) = failure {
                Err(err)
            } else {
                Ok(())
            }
        }
    }

    struct WritesTasksWorkflow;

    #[async_trait]
    impl Workflow for WritesTasksWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "task-writing test workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            std::fs::write(
                options.project_dir.join("TASKS.md"),
                "- [x] write final state\n- [ ] notify ui\n",
            )?;
            Ok(())
        }
    }

    struct DroppedReceiverWorkflow;

    #[async_trait]
    impl Workflow for DroppedReceiverWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "dropped receiver workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            for i in 0..25 {
                options
                    .tx
                    .send(TuiEvent::TokenChunk {
                        agent: "dropped_receiver".to_string(),
                        chunk: format!("chunk-{i}"),
                    })
                    .ok();
            }
            options
                .tx
                .send(TuiEvent::AgentDone {
                    agent: "dropped_receiver".to_string(),
                })
                .ok();
            Ok(())
        }
    }

    struct CancelThenOkWorkflow;

    #[async_trait]
    impl Workflow for CancelThenOkWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "test workflow"
        }

        async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
            let checkpoint =
                crate::checkpoint::Checkpoint::new("run-1", self.name(), prompt, &options.config);
            checkpoint.write_to(&options.project_dir, &options.config)?;
            options.cancel.cancel();
            Ok(())
        }
    }

    struct FileThenCancelWorkflow;

    #[async_trait]
    impl Workflow for FileThenCancelWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "file then cancel workflow"
        }

        async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
            let checkpoint = crate::checkpoint::Checkpoint::new(
                "run-artifact",
                self.name(),
                prompt,
                &options.config,
            );
            checkpoint.write_to(&options.project_dir, &options.config)?;
            options
                .tx
                .send(TuiEvent::FileWritten {
                    agent: "artifact".to_string(),
                    path: "partial.txt".to_string(),
                    old_content: None,
                    new_content: "partial content".to_string(),
                })
                .ok();
            options.cancel.cancel();
            Ok(())
        }
    }

    struct SlowUntilCancelledWorkflow {
        in_flight: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    }

    #[async_trait]
    impl Workflow for SlowUntilCancelledWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "slow cancellation test workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            options
                .tx
                .send(TuiEvent::AgentStarted {
                    agent: "slow".to_string(),
                })
                .ok();
            if let Some(in_flight) = self.in_flight.lock().unwrap().take() {
                let _ = in_flight.send(());
            }
            options.cancel.cancelled().await;
            options
                .tx
                .send(TuiEvent::WorkflowInterrupted {
                    message: "slow workflow observed cancellation".to_string(),
                })
                .ok();
            Ok(())
        }
    }

    fn spawn_recording_report_tee(
        recorded: Arc<tokio::sync::Mutex<Vec<String>>>,
    ) -> (crate::tui::events::TuiSender, FlushSender) {
        let (tee_tx, mut tee_rx) = channel();
        let (flush_tx, mut flush_rx): (FlushSender, mpsc::UnboundedReceiver<oneshot::Sender<()>>) =
            mpsc::unbounded_channel();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(ev) = tee_rx.recv() => {
                        record_test_event(ev, &recorded).await;
                    }
                    Some(ack) = flush_rx.recv() => {
                        while let Ok(ev) = tee_rx.try_recv() {
                            record_test_event(ev, &recorded).await;
                        }
                        let _ = ack.send(());
                    }
                    else => break,
                }
            }
        });

        (tee_tx, flush_tx)
    }

    async fn record_test_event(ev: TuiEvent, recorded: &Arc<tokio::sync::Mutex<Vec<String>>>) {
        if let TuiEvent::TokenChunk { chunk, .. } = ev {
            recorded.lock().await.push(chunk);
        }
    }

    fn temp_test_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read_run_report_json(dir: &std::path::Path) -> serde_json::Value {
        let content = std::fs::read_to_string(dir.join("cortex.run.json")).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn read_run_report_status(dir: &std::path::Path) -> String {
        read_run_report_json(dir)["status"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[allow(dead_code)]
    async fn drain_events_until_closed(mut rx: crate::tui::events::TuiReceiver) -> Vec<TuiEvent> {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
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
