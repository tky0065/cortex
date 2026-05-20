# Resume Checkpoints Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add structured `dev` workflow resume using `cortex.checkpoint.json`, with phase state, tracked file hashes, conflict detection, and documentation updates.

**Architecture:** Add a focused `src/checkpoint.rs` module for checkpoint data, persistence, redaction, hashing, and validation. Wire resume context through `RunOptions` and the orchestrator, then teach `DevWorkflow` to write checkpoints at stable phase boundaries and skip completed phases when resuming. CLI and REPL resume should load the original prompt from the checkpoint and fail before agent execution if the checkpoint is missing, invalid, unsupported, or conflicted.

**Tech Stack:** Rust, serde/serde_json, sha2, uuid, anyhow, existing `Config`, `SecretRedactor`, `RunOptions`, `Orchestrator`, `DevWorkflow`, Cargo tests.

---

## File Structure

- Create `src/checkpoint.rs`: checkpoint schema, resume context, conflict types, file hashing, validation, JSON load/write, redaction, and unit tests.
- Modify `src/main.rs`: register `mod checkpoint;`, update `cortex resume <dir>` to load a checkpoint and use the checkpoint prompt/workflow.
- Modify `src/workflows/mod.rs`: add `ResumeContext` to `RunOptions`.
- Modify `src/orchestrator.rs`: add a resume-aware run path, create a checkpoint for normal supported `dev` runs, reject invalid resume attempts before workflow execution.
- Modify `src/workflows/dev/mod.rs`: write checkpoint phase/file records and skip completed phases during resume.
- Modify `src/repl.rs`: update `/resume <dir>` to use the checkpoint-backed orchestrator path.
- Modify `README.md`: document `cortex.checkpoint.json`, safe resume behavior, and artifact differences.
- Modify `LACUNES.md`: mark lacune 9 complete and add a dated resume checkpoint lot.

---

### Task 1: Add Checkpoint Core Model

**Files:**
- Create: `src/checkpoint.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Register the module**

In `src/main.rs`, add:

```rust
mod checkpoint;
```

near the other module declarations.

- [ ] **Step 2: Write failing constructor and serialization tests**

Create `src/checkpoint.rs` with only imports needed for the tests and this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn new_checkpoint_has_required_identity_fields() {
        let config = Config::default();
        let checkpoint = Checkpoint::new("run-1", "dev", "build a todo app", &config);

        assert_eq!(checkpoint.schema_version, 1);
        assert_eq!(checkpoint.run_id, "run-1");
        assert_eq!(checkpoint.workflow, "dev");
        assert_eq!(checkpoint.prompt, "build a todo app");
        assert_eq!(checkpoint.provider, "ollama");
        assert_eq!(checkpoint.status, CheckpointStatus::Running);
        assert_eq!(checkpoint.current_phase, "started");
        assert_eq!(checkpoint.completed_phases, vec!["started".to_string()]);
        assert_eq!(checkpoint.next_action, "run_ceo");
        assert!(checkpoint.files.is_empty());
        assert!(checkpoint.dev.brief.is_none());
    }

    #[test]
    fn checkpoint_serializes_with_stable_top_level_keys() {
        let config = Config::default();
        let checkpoint = Checkpoint::new("run-1", "dev", "build a todo app", &config);
        let json = serde_json::to_value(&checkpoint).unwrap();

        for key in [
            "schema_version",
            "run_id",
            "cortex_version",
            "workflow",
            "prompt",
            "provider",
            "status",
            "current_phase",
            "completed_phases",
            "next_action",
            "dev",
            "files",
            "updated_at_unix_ms",
        ] {
            assert!(json.get(key).is_some(), "missing top-level key {key}");
        }
    }

    #[test]
    fn only_dev_supports_structured_resume_initially() {
        assert!(Checkpoint::is_resume_supported_for("dev"));
        assert!(!Checkpoint::is_resume_supported_for("marketing"));
        assert!(!Checkpoint::is_resume_supported_for("prospecting"));
        assert!(!Checkpoint::is_resume_supported_for("code-review"));
    }
}
```

- [ ] **Step 3: Run the focused tests and verify they fail**

Run:

```bash
cargo test checkpoint::tests -- --nocapture
```

Expected: FAIL because `Checkpoint`, `CheckpointStatus`, and methods are not implemented yet.

- [ ] **Step 4: Implement the minimal checkpoint model**

Add this implementation to `src/checkpoint.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointStatus {
    Running,
    Interrupted,
    Failed,
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointConflictType {
    CheckpointMissing,
    UnsupportedWorkflow,
    WorkflowMismatch,
    InvalidCheckpoint,
    FileMissing,
    FileModified,
    PhaseInconsistent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointConflict {
    pub conflict_type: CheckpointConflictType,
    pub path: Option<String>,
    pub message: String,
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DevCheckpointState {
    pub brief: Option<String>,
    pub specs_path: Option<String>,
    pub architecture_path: Option<String>,
    pub expected_files: Vec<String>,
    pub qa_iteration: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointFile {
    pub path: String,
    pub agent: String,
    pub phase: String,
    pub operation: String,
    pub bytes: u64,
    pub sha256: String,
    pub updated_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub run_id: String,
    pub cortex_version: String,
    pub workflow: String,
    pub prompt: String,
    pub provider: String,
    pub status: CheckpointStatus,
    pub current_phase: String,
    pub completed_phases: Vec<String>,
    pub next_action: String,
    pub dev: DevCheckpointState,
    pub files: Vec<CheckpointFile>,
    pub updated_at_unix_ms: u64,
}

impl Checkpoint {
    pub fn new(run_id: impl Into<String>, workflow: impl Into<String>, prompt: impl Into<String>, config: &Config) -> Self {
        Self {
            schema_version: 1,
            run_id: run_id.into(),
            cortex_version: env!("CARGO_PKG_VERSION").to_string(),
            workflow: workflow.into(),
            prompt: prompt.into(),
            provider: config.provider.default.clone(),
            status: CheckpointStatus::Running,
            current_phase: "started".to_string(),
            completed_phases: vec!["started".to_string()],
            next_action: "run_ceo".to_string(),
            dev: DevCheckpointState::default(),
            files: Vec::new(),
            updated_at_unix_ms: now_unix_ms(),
        }
    }

    pub fn is_resume_supported_for(workflow: &str) -> bool {
        workflow == "dev"
    }

    pub fn checkpoint_path(project_dir: &Path) -> PathBuf {
        project_dir.join("cortex.checkpoint.json")
    }
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
```

Remove unused imports if the compiler reports them.

- [ ] **Step 5: Run the focused tests and verify they pass**

Run:

```bash
cargo test checkpoint::tests -- --nocapture
```

Expected: PASS for the three checkpoint model tests.

- [ ] **Step 6: Commit**

Run:

```bash
git add src/main.rs src/checkpoint.rs
git commit -m "feat: add checkpoint model"
```

---

### Task 2: Add Checkpoint Persistence, Redaction, and File Validation

**Files:**
- Modify: `src/checkpoint.rs`

- [ ] **Step 1: Write failing persistence and validation tests**

Append these tests inside `src/checkpoint.rs` `mod tests`:

```rust
    #[test]
    fn checkpoint_write_load_round_trips_and_redacts_prompt() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_roundtrip_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut config = Config::default();
        config.api_keys.openai = Some("sk-test-checkpoint-secret".to_string());

        let checkpoint = Checkpoint::new(
            "run-1",
            "dev",
            "build with sk-test-checkpoint-secret",
            &config,
        );
        checkpoint.write_to(&dir, &config).unwrap();

        let raw = std::fs::read_to_string(Checkpoint::checkpoint_path(&dir)).unwrap();
        assert!(!raw.contains("sk-test-checkpoint-secret"));

        let loaded = Checkpoint::load(&dir).unwrap();
        assert_eq!(loaded.run_id, "run-1");
        assert_eq!(loaded.prompt, "build with [REDACTED]");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_file_and_validate_files_detects_unchanged_modified_and_missing() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_validate_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("specs.md"), "initial specs").unwrap();

        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);
        checkpoint
            .record_file("pm", "specs-ready", "specs.md", "created", &dir)
            .unwrap();

        assert!(checkpoint.validate_files(&dir).unwrap().is_empty());

        std::fs::write(dir.join("specs.md"), "changed specs").unwrap();
        let conflicts = checkpoint.validate_files(&dir).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, CheckpointConflictType::FileModified);
        assert_eq!(conflicts[0].path.as_deref(), Some("specs.md"));
        assert!(conflicts[0].expected_sha256.is_some());
        assert!(conflicts[0].actual_sha256.is_some());

        std::fs::remove_file(dir.join("specs.md")).unwrap();
        let conflicts = checkpoint.validate_files(&dir).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, CheckpointConflictType::FileMissing);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_checkpoint_json_returns_readable_error() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_invalid_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Checkpoint::checkpoint_path(&dir), "{not-json").unwrap();

        let err = Checkpoint::load(&dir).unwrap_err().to_string();
        assert!(err.contains("Failed to parse checkpoint"));

        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test checkpoint::tests -- --nocapture
```

Expected: FAIL because `write_to`, `load`, `record_file`, and `validate_files` are missing.

- [ ] **Step 3: Implement persistence and validation**

Add these methods and helpers to `src/checkpoint.rs`:

```rust
impl Checkpoint {
    pub fn load(project_dir: &Path) -> Result<Self> {
        let path = Self::checkpoint_path(project_dir);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read checkpoint: {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse checkpoint: {}", path.display()))
    }

    pub fn write_to(&self, project_dir: &Path, config: &Config) -> Result<()> {
        std::fs::create_dir_all(project_dir)
            .with_context(|| format!("Failed to create project dir: {}", project_dir.display()))?;
        let redactor = crate::secrets::SecretRedactor::from_config_and_env(config);
        let mut redacted = self.clone();
        redacted.prompt = redactor.redact(&redacted.prompt);
        if let Some(brief) = redacted.dev.brief.as_mut() {
            *brief = redactor.redact(brief);
        }
        let json = serde_json::to_string_pretty(&redacted)
            .context("Failed to serialize checkpoint")?;
        let path = Self::checkpoint_path(project_dir);
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write checkpoint: {}", path.display()))
    }

    pub fn record_phase_complete(&mut self, phase: impl Into<String>, next_action: impl Into<String>) {
        let phase = phase.into();
        self.current_phase = phase.clone();
        if !self.completed_phases.iter().any(|p| p == &phase) {
            self.completed_phases.push(phase);
        }
        self.next_action = next_action.into();
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn record_file(
        &mut self,
        agent: impl Into<String>,
        phase: impl Into<String>,
        path: impl Into<String>,
        operation: impl Into<String>,
        project_dir: &Path,
    ) -> Result<()> {
        let path = path.into();
        let full_path = project_dir.join(&path);
        let bytes = std::fs::metadata(&full_path)
            .with_context(|| format!("Failed to stat tracked file: {}", full_path.display()))?
            .len();
        let sha256 = sha256_file(&full_path)?;
        let record = CheckpointFile {
            path: path.clone(),
            agent: agent.into(),
            phase: phase.into(),
            operation: operation.into(),
            bytes,
            sha256,
            updated_at_unix_ms: now_unix_ms(),
        };
        if let Some(existing) = self.files.iter_mut().find(|file| file.path == path) {
            *existing = record;
        } else {
            self.files.push(record);
        }
        self.updated_at_unix_ms = now_unix_ms();
        Ok(())
    }

    pub fn validate_files(&self, project_dir: &Path) -> Result<Vec<CheckpointConflict>> {
        let mut conflicts = Vec::new();
        for file in &self.files {
            let full_path = project_dir.join(&file.path);
            if !full_path.exists() {
                conflicts.push(CheckpointConflict {
                    conflict_type: CheckpointConflictType::FileMissing,
                    path: Some(file.path.clone()),
                    message: format!("tracked file is missing: {}", file.path),
                    expected_sha256: Some(file.sha256.clone()),
                    actual_sha256: None,
                });
                continue;
            }
            let actual = sha256_file(&full_path)?;
            if actual != file.sha256 {
                conflicts.push(CheckpointConflict {
                    conflict_type: CheckpointConflictType::FileModified,
                    path: Some(file.path.clone()),
                    message: format!("tracked file was modified since checkpoint: {}", file.path),
                    expected_sha256: Some(file.sha256.clone()),
                    actual_sha256: Some(actual),
                });
            }
        }
        Ok(conflicts)
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Failed to read tracked file: {}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test checkpoint::tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add src/checkpoint.rs
git commit -m "feat: persist resume checkpoints"
```

---

### Task 3: Wire Resume Context Through Orchestrator

**Files:**
- Modify: `src/workflows/mod.rs`
- Modify: `src/orchestrator.rs`
- Modify: `src/main.rs`
- Modify: `src/repl.rs`

- [ ] **Step 1: Write failing orchestrator resume tests**

In `src/orchestrator.rs` test module, add:

```rust
    #[tokio::test]
    async fn resume_without_checkpoint_fails_before_workflow_execution() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_resume_missing_checkpoint_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Arc::new(Config::default());
        let orch = Orchestrator::new(crate::workflows::get_workflow("dev").unwrap(), config);
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

        let orch = Orchestrator::new(
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
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo test orchestrator::tests::resume_ -- --nocapture
```

Expected: FAIL because `resume_with_project_dir` and `RunOptions.resume` do not exist.

- [ ] **Step 3: Add resume context to workflow options**

In `src/workflows/mod.rs`, add:

```rust
#[derive(Clone, Debug)]
pub struct ResumeContext {
    pub checkpoint: crate::checkpoint::Checkpoint,
    pub conflicts: Vec<crate::checkpoint::CheckpointConflict>,
}
```

Then add this field to `RunOptions`:

```rust
    pub resume: Option<ResumeContext>,
```

Update every `RunOptions { ... }` literal in the repo with either:

```rust
            resume: options.resume.clone(),
```

when cloning/modifying an existing options value, or:

```rust
            resume: None,
```

in test/manual constructors that do not resume.

- [ ] **Step 4: Add orchestrator resume path**

In `src/orchestrator.rs`, add this public method inside `impl Orchestrator`:

```rust
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
```

Rename the existing `run_with_project_dir` body into a private helper:

```rust
    async fn run_with_project_dir_and_resume(
        &self,
        prompt: String,
        auto: bool,
        verbose: bool,
        tx: Option<TuiSender>,
        project_dir: std::path::PathBuf,
        resume: Option<crate::workflows::ResumeContext>,
    ) -> Result<()> {
        // existing body, using already-resolved project_dir
    }
```

Keep the public existing `run_with_project_dir(...)` as a wrapper that resolves the optional project dir and passes `resume: None`.

When constructing `RunOptions`, add:

```rust
            resume,
```

Add this helper outside `impl Orchestrator`:

```rust
fn format_checkpoint_conflicts(conflicts: &[crate::checkpoint::CheckpointConflict]) -> String {
    let mut lines = vec!["checkpoint conflicts prevent structured resume:".to_string()];
    for conflict in conflicts {
        match (&conflict.path, &conflict.expected_sha256, &conflict.actual_sha256) {
            (Some(path), Some(expected), Some(actual)) => lines.push(format!(
                "- {}: {} (expected {}, found {})",
                path, conflict.message, expected, actual
            )),
            (Some(path), Some(expected), None) => lines.push(format!(
                "- {}: {} (expected {})",
                path, conflict.message, expected
            )),
            (Some(path), _, _) => lines.push(format!("- {}: {}", path, conflict.message)),
            (None, _, _) => lines.push(format!("- {}", conflict.message)),
        }
    }
    lines.join("\n")
}
```

- [ ] **Step 5: Update CLI resume**

In `src/main.rs`, replace the `Commands::Resume` run call with:

```rust
            let checkpoint = checkpoint::Checkpoint::load(&project_dir)?;
            let wf = workflows::get_workflow(&checkpoint.workflow)?;
            let orch = Orchestrator::new(wf, Arc::new(config));
            orch.resume_with_project_dir(verbose, None, project_dir).await?;
```

Keep the existing directory existence check.

- [ ] **Step 6: Update REPL resume**

In `src/repl.rs` `/resume <project-dir>` handler, replace the hardcoded `workflows::get_workflow("dev")?` and generic prompt path with checkpoint loading:

```rust
                let checkpoint = match crate::checkpoint::Checkpoint::load(&project_dir) {
                    Ok(checkpoint) => checkpoint,
                    Err(e) => {
                        send(
                            tx,
                            TuiEvent::Error {
                                agent: "repl".to_string(),
                                message: e.to_string(),
                            },
                        );
                        return Ok(false);
                    }
                };
                let wf = workflows::get_workflow(&checkpoint.workflow)?;
```

Inside the spawned task, replace `run_with_project_dir(...)` with:

```rust
                        .resume_with_project_dir(false, Some(tx_clone), project_dir.clone())
```

- [ ] **Step 7: Run focused tests**

Run:

```bash
cargo test orchestrator::tests::resume_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Run compile check**

Run:

```bash
cargo check
```

Expected: PASS.

- [ ] **Step 9: Commit**

Run:

```bash
git add src/workflows/mod.rs src/orchestrator.rs src/main.rs src/repl.rs
git commit -m "feat: validate checkpoint resume"
```

---

### Task 4: Write Dev Workflow Checkpoints

**Files:**
- Modify: `src/workflows/dev/mod.rs`
- Modify: `src/checkpoint.rs`

- [ ] **Step 1: Add checkpoint update helpers**

In `src/checkpoint.rs`, add these dev-specific methods:

```rust
impl Checkpoint {
    pub fn set_dev_brief(&mut self, brief: impl Into<String>) {
        self.dev.brief = Some(brief.into());
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn set_dev_specs_path(&mut self, path: impl Into<String>) {
        self.dev.specs_path = Some(path.into());
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn set_dev_architecture_path(&mut self, path: impl Into<String>) {
        self.dev.architecture_path = Some(path.into());
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn set_dev_expected_files(&mut self, files: Vec<String>) {
        self.dev.expected_files = files;
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn set_dev_qa_iteration(&mut self, iteration: usize) {
        self.dev.qa_iteration = iteration;
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn mark_interrupted(&mut self) {
        self.status = CheckpointStatus::Interrupted;
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn mark_failed(&mut self) {
        self.status = CheckpointStatus::Failed;
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn mark_completed(&mut self) {
        self.status = CheckpointStatus::Completed;
        self.record_phase_complete("done", "none");
    }
}
```

- [ ] **Step 2: Add checkpoint save helper in dev workflow**

In `src/workflows/dev/mod.rs`, add near helpers:

```rust
fn save_checkpoint(opts: &RunOptions, checkpoint: &crate::checkpoint::Checkpoint) -> Result<()> {
    checkpoint.write_to(&opts.project_dir, &opts.config)
}

fn checkpoint_from_options(opts: &RunOptions, prompt: &str) -> crate::checkpoint::Checkpoint {
    opts.resume
        .as_ref()
        .map(|resume| resume.checkpoint.clone())
        .unwrap_or_else(|| {
            crate::checkpoint::Checkpoint::new(
                uuid::Uuid::new_v4().to_string(),
                "dev",
                prompt.to_string(),
                &opts.config,
            )
        })
}
```

- [ ] **Step 3: Initialize and persist checkpoint at workflow start**

Near the start of `DevWorkflow::run`, after `opts` is created and before phase work begins, add:

```rust
        let mut checkpoint = checkpoint_from_options(&opts, &prompt);
        checkpoint.status = crate::checkpoint::CheckpointStatus::Running;
        checkpoint.record_phase_complete("started", "run_ceo");
        save_checkpoint(&opts, &checkpoint)?;
```

- [ ] **Step 4: Record brief checkpoint**

After the CEO/inter-agent review produces final `brief`, add:

```rust
        checkpoint.set_dev_brief(brief.clone());
        checkpoint.record_phase_complete("brief-ready", "run_pm");
        save_checkpoint(&opts, &checkpoint)?;
```

- [ ] **Step 5: Record specs checkpoint**

After writing `specs.md` and optional `TASKS.md`, and after PM review finishes, add:

```rust
        checkpoint.set_dev_specs_path("specs.md");
        checkpoint.record_file("pm", "specs-ready", "specs.md", "created", &project_dir)?;
        if project_dir.join("TASKS.md").exists() {
            checkpoint.record_file("pm", "specs-ready", "TASKS.md", "created", &project_dir)?;
        }
        checkpoint.record_phase_complete("specs-ready", "run_tech_lead");
        save_checkpoint(&opts, &checkpoint)?;
```

- [ ] **Step 6: Record architecture checkpoint**

After Tech Lead review finishes, add:

```rust
        checkpoint.set_dev_architecture_path("architecture.md");
        checkpoint.record_file(
            "tech_lead",
            "architecture-ready",
            "architecture.md",
            "created",
            &project_dir,
        )?;
        checkpoint.record_phase_complete("architecture-ready", "run_developer");
        save_checkpoint(&opts, &checkpoint)?;
```

- [ ] **Step 7: Record development checkpoint**

After all developer workers finish and before Developer review, add:

```rust
        let written_files = parse_files_to_create(&arch);
        checkpoint.set_dev_expected_files(written_files.clone());
        for path in written_files {
            if project_dir.join(&path).exists() {
                checkpoint.record_file("developer", "development-done", path, "created", &project_dir)?;
            }
        }
        checkpoint.record_phase_complete("development-done", "run_qa");
        save_checkpoint(&opts, &checkpoint)?;
```

If `parse_files_to_create(&arch)` was consumed earlier by the developer loop, first change that earlier code to:

```rust
        let files = parse_files_to_create(&arch);
        let files_for_checkpoint = files.clone();
```

Then use `files_for_checkpoint` for the checkpoint rather than parsing again.

- [ ] **Step 8: Record QA checkpoint**

Inside the QA loop, after each QA report is produced, add:

```rust
            checkpoint.set_dev_qa_iteration(iteration + 1);
            save_checkpoint(&opts, &checkpoint)?;
```

When QA approves, before `break`, add:

```rust
                checkpoint.record_phase_complete("qa-approved", "run_devops");
                save_checkpoint(&opts, &checkpoint)?;
```

When max iterations are reached, before `break`, add:

```rust
                checkpoint.record_phase_complete("qa-max-iterations", "run_devops");
                save_checkpoint(&opts, &checkpoint)?;
```

- [ ] **Step 9: Record DevOps and done checkpoints**

After `agents::devops::run(...)` and DevOps review finish, add:

```rust
        for path in ["Dockerfile", "docker-compose.yml", "README.md"] {
            if project_dir.join(path).exists() {
                checkpoint.record_file("devops", "devops-done", path, "created", &project_dir)?;
            }
        }
        checkpoint.record_phase_complete("devops-done", "finish");
        save_checkpoint(&opts, &checkpoint)?;
```

Before returning `Ok(())`, add:

```rust
        checkpoint.mark_completed();
        save_checkpoint(&opts, &checkpoint)?;
```

- [ ] **Step 10: Run checks**

Run:

```bash
cargo check
cargo test checkpoint::tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 11: Commit**

Run:

```bash
git add src/checkpoint.rs src/workflows/dev/mod.rs
git commit -m "feat: write dev workflow checkpoints"
```

---

### Task 5: Skip Completed Dev Phases During Resume

**Files:**
- Modify: `src/checkpoint.rs`
- Modify: `src/workflows/dev/mod.rs`

- [ ] **Step 1: Add checkpoint phase query helpers**

In `src/checkpoint.rs`, add:

```rust
impl Checkpoint {
    pub fn has_completed_phase(&self, phase: &str) -> bool {
        self.completed_phases.iter().any(|completed| completed == phase)
    }

    pub fn is_resuming(&self) -> bool {
        self.status != CheckpointStatus::Completed && self.completed_phases.len() > 1
    }
}
```

Add tests:

```rust
    #[test]
    fn completed_phase_helpers_report_resume_state() {
        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);
        assert!(!checkpoint.is_resuming());
        assert!(checkpoint.has_completed_phase("started"));

        checkpoint.record_phase_complete("specs-ready", "run_tech_lead");
        assert!(checkpoint.is_resuming());
        assert!(checkpoint.has_completed_phase("specs-ready"));
        assert!(!checkpoint.has_completed_phase("architecture-ready"));
    }
```

- [ ] **Step 2: Run helper tests**

Run:

```bash
cargo test checkpoint::tests::completed_phase_helpers_report_resume_state -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Skip CEO when brief exists**

In `src/workflows/dev/mod.rs`, replace the CEO phase assignment with logic equivalent to:

```rust
        let brief = if checkpoint.has_completed_phase("brief-ready") {
            checkpoint
                .dev
                .brief
                .clone()
                .context("checkpoint phase brief-ready is missing dev.brief")?
        } else {
            let generated = {
                let first = agents::ceo::run(&prompt, &opts).await?;
                if let Some(question) = parse_clarification_needed(&first) {
                    let answer = ask_user("ceo", &question, &opts).await?;
                    if answer.trim().is_empty() {
                        first
                    } else {
                        let enriched = format!("{}\n\nAdditional context: {}", prompt, answer.trim());
                        agents::ceo::run(&enriched, &opts).await?
                    }
                } else {
                    first
                }
            };
            // Keep the existing inter-agent review loop here and return its final brief.
            generated
        };
```

Preserve the existing inter-agent review behavior for non-resume runs. Do not request review for skipped phases.

- [ ] **Step 4: Skip PM when specs are valid**

Before PM generation, add:

```rust
        let mut specs = if checkpoint.has_completed_phase("specs-ready") {
            let specs_path = checkpoint
                .dev
                .specs_path
                .clone()
                .unwrap_or_else(|| "specs.md".to_string());
            fs.read(&specs_path)
                .with_context(|| format!("Cannot read checkpoint specs file: {specs_path}"))?
        } else {
            // existing PM generation, parse, write, review, and checkpoint code
        };
```

Move the existing PM generation/write/review code into the `else` branch.

- [ ] **Step 5: Skip Tech Lead when architecture is valid**

Before Tech Lead generation, add:

```rust
        let mut arch = if checkpoint.has_completed_phase("architecture-ready") {
            let arch_path = checkpoint
                .dev
                .architecture_path
                .clone()
                .unwrap_or_else(|| "architecture.md".to_string());
            fs.read(&arch_path)
                .with_context(|| format!("Cannot read checkpoint architecture file: {arch_path}"))?
        } else {
            // existing Tech Lead generation, write, review, and checkpoint code
        };
```

Move the existing Tech Lead generation/write/review code into the `else` branch.

- [ ] **Step 6: Skip Developer when development is complete**

Wrap developer worker generation and developer review in:

```rust
        if checkpoint.has_completed_phase("development-done") {
            let _ = opts.tx.send(TuiEvent::TokenChunk {
                agent: "orchestrator".into(),
                chunk: "Resuming after development-done; skipping developer generation.".into(),
            });
        } else {
            // existing developer worker generation, review, and checkpoint code
        }
```

- [ ] **Step 7: Skip QA when already approved**

Wrap QA loop in:

```rust
        if checkpoint.has_completed_phase("qa-approved")
            || checkpoint.has_completed_phase("qa-max-iterations")
        {
            let _ = opts.tx.send(TuiEvent::TokenChunk {
                agent: "orchestrator".into(),
                chunk: "Resuming after QA; skipping QA loop.".into(),
            });
        } else {
            // existing QA loop and checkpoint code
        }
```

- [ ] **Step 8: Skip DevOps when completed**

Wrap DevOps generation/review in:

```rust
        if checkpoint.has_completed_phase("devops-done") {
            let _ = opts.tx.send(TuiEvent::TokenChunk {
                agent: "orchestrator".into(),
                chunk: "Resuming after devops-done; skipping DevOps.".into(),
            });
        } else {
            // existing DevOps generation, review, and checkpoint code
        }
```

- [ ] **Step 9: Run checks**

Run:

```bash
cargo check
cargo test checkpoint::tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 10: Commit**

Run:

```bash
git add src/checkpoint.rs src/workflows/dev/mod.rs
git commit -m "feat: resume dev workflow phases"
```

---

### Task 6: Mark Interrupted and Failed Checkpoints

**Files:**
- Modify: `src/orchestrator.rs`
- Modify: `src/checkpoint.rs`

- [ ] **Step 1: Add status update helper**

In `src/orchestrator.rs`, add:

```rust
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
```

- [ ] **Step 2: Update failure/interruption paths**

In `run_with_project_dir_and_resume` match arms:

For `RunCompletion::Workflow(Err(e))`, before finalizing run report, add:

```rust
                update_checkpoint_status(
                    &project_dir,
                    &self.config,
                    crate::checkpoint::CheckpointStatus::Failed,
                );
```

For `RunCompletion::Interrupted`, before finalizing run report, add:

```rust
                update_checkpoint_status(
                    &project_dir,
                    &self.config,
                    crate::checkpoint::CheckpointStatus::Interrupted,
                );
```

Do not mark completed here; `DevWorkflow` marks completed at the final stable boundary.

- [ ] **Step 3: Run checks**

Run:

```bash
cargo check
cargo test checkpoint::tests orchestrator::tests::resume_ -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add src/orchestrator.rs
git commit -m "feat: persist checkpoint terminal status"
```

---

### Task 7: Update Documentation and LACUNES

**Files:**
- Modify: `README.md`
- Modify: `LACUNES.md`

- [ ] **Step 1: Update README resume section**

In `README.md`, update the resume section to include:

```markdown
`cortex resume <project-dir>` uses `cortex.checkpoint.json` to continue a structured `dev` workflow run. The checkpoint stores the original prompt, completed phases, next action, and hashes for files Cortex already wrote.

Resume stops before running agents if the checkpoint is missing, invalid, belongs to an unsupported workflow, or if tracked files were changed or removed. Cortex does not overwrite local edits during structured resume.

Run artifacts:

- `cortex.checkpoint.json` controls safe resume for interrupted `dev` runs.
- `cortex.run.json` is a diagnostic timeline for success, failure, and interruption.
- `cortex.manifest.json` identifies a successfully generated project.
```

Keep nearby existing resume wording, but remove any claim that resume simply continues from files without a checkpoint.

- [ ] **Step 2: Update LACUNES lacune 9**

In `LACUNES.md`, change lacune 9 to:

```markdown
### 9. Experience de reprise de session a durcir
**Statut:** Terminé
**Preuve:** Couvert par `cortex.checkpoint.json`, qui stocke l'état de reprise du workflow `dev`: phase courante, phases terminées, prochaine action, prompt d'origine, fichiers suivis, hashes SHA-256 et détection de conflits avant reprise.
```

Keep the existing constat/importance/action text below unless it now contradicts implementation; adjust only contradictions.

- [ ] **Step 3: Add lot entry**

At the end of `LACUNES.md` "Suivi des lots", add:

```markdown
- 2026-05-20 — Lot reprise robuste terminé: `cortex.checkpoint.json`, reprise structurée du workflow `dev`, validation des hashes, refus des reprises ambiguës et documentation des artefacts. Lacune terminée: 9.
```

- [ ] **Step 4: Commit**

Run:

```bash
git add README.md LACUNES.md
git commit -m "docs: document resume checkpoints"
```

---

### Task 8: Final Verification

**Files:**
- No code changes expected unless verification exposes issues.

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt
```

Expected: command exits successfully.

- [ ] **Step 2: Run full test suite**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 3: Run compile check**

Run:

```bash
cargo check
```

Expected: PASS.

- [ ] **Step 4: Inspect final git status**

Run:

```bash
git status --short
```

Expected: only unrelated pre-existing untracked files remain, if any.

- [ ] **Step 5: Commit formatting or fixes if needed**

If `cargo fmt` or verification changed tracked files, run:

```bash
git add <changed tracked files>
git commit -m "chore: verify resume checkpoints"
```

Skip this commit if no tracked files changed after verification.

---

## Self-Review

- Spec coverage: checkpoint artifact, phase state, file hashes, conflict detection, conservative resume, `dev` scope, README, and `LACUNES.md` are covered by Tasks 1-8.
- Placeholder scan: no `TBD`, `TODO`, "implement later", or unspecified test steps remain.
- Type consistency: `Checkpoint`, `CheckpointStatus`, `CheckpointConflict`, `ResumeContext`, `RunOptions.resume`, and `resume_with_project_dir` names are consistent across tasks.
