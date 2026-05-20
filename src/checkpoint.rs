use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    pub fn new(
        run_id: impl Into<String>,
        workflow: impl Into<String>,
        prompt: impl Into<String>,
        config: &Config,
    ) -> Self {
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

    pub fn has_completed_phase(&self, phase: &str) -> bool {
        self.completed_phases
            .iter()
            .any(|completed| completed == phase)
    }

    pub fn is_resuming(&self) -> bool {
        self.status != CheckpointStatus::Completed && self.completed_phases.len() > 1
    }

    pub fn checkpoint_path(project_dir: &Path) -> PathBuf {
        project_dir.join("cortex.checkpoint.json")
    }

    pub fn load(project_dir: &Path) -> Result<Self> {
        let checkpoint_path = Self::checkpoint_path(project_dir);
        let raw = fs::read_to_string(&checkpoint_path)
            .with_context(|| format!("Failed to read checkpoint: {}", checkpoint_path.display()))?;

        serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse checkpoint: {}", checkpoint_path.display()))
    }

    pub fn write_to(&self, project_dir: &Path, config: &Config) -> Result<()> {
        fs::create_dir_all(project_dir).with_context(|| {
            format!(
                "Failed to create project directory: {}",
                project_dir.display()
            )
        })?;

        let redactor = crate::secrets::SecretRedactor::from_config_and_env(config);
        let mut checkpoint = self.clone();
        checkpoint.prompt = redactor.redact_text(&checkpoint.prompt);
        if let Some(brief) = checkpoint.dev.brief.as_mut() {
            *brief = redactor.redact_text(brief);
        }
        checkpoint.updated_at_unix_ms = now_unix_ms();

        let raw =
            serde_json::to_string_pretty(&checkpoint).context("Failed to serialize checkpoint")?;
        let checkpoint_path = Self::checkpoint_path(project_dir);
        fs::write(&checkpoint_path, raw).with_context(|| {
            format!("Failed to write checkpoint: {}", checkpoint_path.display())
        })?;

        Ok(())
    }

    pub fn record_phase_complete(
        &mut self,
        phase: impl Into<String>,
        next_action: impl Into<String>,
    ) {
        let phase = phase.into();
        self.current_phase = phase.clone();
        if !self
            .completed_phases
            .iter()
            .any(|completed| completed == &phase)
        {
            self.completed_phases.push(phase);
        }
        self.next_action = next_action.into();
        self.updated_at_unix_ms = now_unix_ms();
    }

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

    #[allow(dead_code)]
    pub fn mark_interrupted(&mut self) {
        self.status = CheckpointStatus::Interrupted;
        self.updated_at_unix_ms = now_unix_ms();
    }

    #[allow(dead_code)]
    pub fn mark_failed(&mut self) {
        self.status = CheckpointStatus::Failed;
        self.updated_at_unix_ms = now_unix_ms();
    }

    pub fn mark_completed(&mut self) {
        self.status = CheckpointStatus::Completed;
        self.record_phase_complete("done", "none");
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
        let path = normalize_checkpoint_path(&path)?;
        let file_path = project_dir.join(&path);
        let metadata = fs::metadata(&file_path)
            .with_context(|| format!("Failed to stat checkpoint file: {}", file_path.display()))?;
        let sha256 = sha256_file(&file_path)
            .with_context(|| format!("Failed to hash checkpoint file: {}", file_path.display()))?;
        let file = CheckpointFile {
            path: path.clone(),
            agent: agent.into(),
            phase: phase.into(),
            operation: operation.into(),
            bytes: metadata.len(),
            sha256,
            updated_at_unix_ms: now_unix_ms(),
        };

        if let Some(existing) = self.files.iter_mut().find(|record| record.path == path) {
            *existing = file;
        } else {
            self.files.push(file);
        }
        self.updated_at_unix_ms = now_unix_ms();

        Ok(())
    }

    pub fn validate_files(&self, project_dir: &Path) -> Result<Vec<CheckpointConflict>> {
        let mut conflicts = Vec::new();

        for file in &self.files {
            let normalized_path = normalize_checkpoint_path(&file.path)?;
            let file_path = project_dir.join(&normalized_path);
            if !file_path.exists() {
                conflicts.push(CheckpointConflict {
                    conflict_type: CheckpointConflictType::FileMissing,
                    path: Some(normalized_path.clone()),
                    message: format!("checkpoint file is missing: {normalized_path}"),
                    expected_sha256: Some(file.sha256.clone()),
                    actual_sha256: None,
                });
                continue;
            }

            let actual_sha256 = sha256_file(&file_path).with_context(|| {
                format!("Failed to hash checkpoint file: {}", file_path.display())
            })?;
            if actual_sha256 != file.sha256 {
                conflicts.push(CheckpointConflict {
                    conflict_type: CheckpointConflictType::FileModified,
                    path: Some(normalized_path.clone()),
                    message: format!("checkpoint file was modified: {normalized_path}"),
                    expected_sha256: Some(file.sha256.clone()),
                    actual_sha256: Some(actual_sha256),
                });
            }
        }

        Ok(conflicts)
    }
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)
        .with_context(|| format!("Failed to read file for sha256: {}", path.display()))?;
    Ok(sha256_bytes(&bytes))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn normalize_checkpoint_path(path: &str) -> Result<String> {
    let input = Path::new(path);
    let mut normalized = Vec::new();

    for component in input.components() {
        match component {
            Component::Normal(part) => normalized.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("Invalid checkpoint file path: {path}");
            }
        }
    }

    if normalized.is_empty() {
        anyhow::bail!("Invalid checkpoint file path: {path}");
    }

    Ok(normalized.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::path::Path;

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

    #[test]
    fn checkpoint_path_uses_project_directory() {
        let path = Checkpoint::checkpoint_path(Path::new("/tmp/project"));

        assert_eq!(path, Path::new("/tmp/project/cortex.checkpoint.json"));
        assert!(path.ends_with("cortex.checkpoint.json"));
    }

    #[test]
    fn checkpoint_conflict_captures_file_mismatch_details() {
        let conflict = CheckpointConflict {
            conflict_type: CheckpointConflictType::FileModified,
            path: Some("src/main.rs".to_string()),
            message: "file changed after checkpoint".to_string(),
            expected_sha256: Some("expected".to_string()),
            actual_sha256: Some("actual".to_string()),
        };

        assert_eq!(conflict.conflict_type, CheckpointConflictType::FileModified);
        assert_eq!(conflict.path.as_deref(), Some("src/main.rs"));
        assert_eq!(conflict.message, "file changed after checkpoint");
        assert_eq!(conflict.expected_sha256.as_deref(), Some("expected"));
        assert_eq!(conflict.actual_sha256.as_deref(), Some("actual"));
    }

    #[test]
    fn checkpoint_conflict_types_serialize_as_snake_case() {
        let cases = [
            (
                CheckpointConflictType::CheckpointMissing,
                "checkpoint_missing",
            ),
            (
                CheckpointConflictType::UnsupportedWorkflow,
                "unsupported_workflow",
            ),
            (
                CheckpointConflictType::WorkflowMismatch,
                "workflow_mismatch",
            ),
            (
                CheckpointConflictType::InvalidCheckpoint,
                "invalid_checkpoint",
            ),
            (CheckpointConflictType::FileMissing, "file_missing"),
            (CheckpointConflictType::FileModified, "file_modified"),
            (
                CheckpointConflictType::PhaseInconsistent,
                "phase_inconsistent",
            ),
        ];

        for (conflict_type, expected) in cases {
            assert_eq!(serde_json::to_value(conflict_type).unwrap(), expected);
        }
    }

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
        let dir =
            std::env::temp_dir().join(format!("cortex_checkpoint_validate_{}", std::process::id()));
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
        assert_eq!(
            conflicts[0].conflict_type,
            CheckpointConflictType::FileModified
        );
        assert_eq!(conflicts[0].path.as_deref(), Some("specs.md"));
        assert!(conflicts[0].expected_sha256.is_some());
        assert!(conflicts[0].actual_sha256.is_some());

        std::fs::remove_file(dir.join("specs.md")).unwrap();
        let conflicts = checkpoint.validate_files(&dir).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(
            conflicts[0].conflict_type,
            CheckpointConflictType::FileMissing
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_file_normalizes_dot_segments_and_replaces_equivalent_paths() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_normalize_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("specs.md"), "initial specs").unwrap();

        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);
        checkpoint
            .record_file("pm", "specs-ready", "./specs.md", "created", &dir)
            .unwrap();

        assert_eq!(checkpoint.files.len(), 1);
        assert_eq!(checkpoint.files[0].path, "specs.md");

        checkpoint
            .record_file("pm", "specs-ready", "specs.md", "updated", &dir)
            .unwrap();
        checkpoint
            .record_file("pm", "specs-ready", "./specs.md", "updated-again", &dir)
            .unwrap();

        assert_eq!(checkpoint.files.len(), 1);
        assert_eq!(checkpoint.files[0].path, "specs.md");
        assert_eq!(checkpoint.files[0].operation, "updated-again");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_file_replaces_existing_record_with_updated_hash_for_same_normalized_path() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_hash_refresh_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("src.rs"), "initial").unwrap();

        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);
        checkpoint
            .record_file("developer", "development-done", "./src.rs", "created", &dir)
            .unwrap();
        let first_hash = checkpoint.files[0].sha256.clone();

        std::fs::write(dir.join("src.rs"), "fixed").unwrap();
        checkpoint
            .record_file("developer", "development-done", "src.rs", "modified", &dir)
            .unwrap();

        assert_eq!(checkpoint.files.len(), 1);
        assert_eq!(checkpoint.files[0].path, "src.rs");
        assert_eq!(checkpoint.files[0].operation, "modified");
        assert_ne!(checkpoint.files[0].sha256, first_hash);
        assert!(checkpoint.validate_files(&dir).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_file_rejects_parent_and_absolute_paths() {
        let dir =
            std::env::temp_dir().join(format!("cortex_checkpoint_reject_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);

        assert!(
            checkpoint
                .record_file("pm", "specs-ready", "../outside.txt", "created", &dir)
                .is_err()
        );
        assert!(
            checkpoint
                .record_file(
                    "pm",
                    "specs-ready",
                    dir.join("specs.md").to_string_lossy().to_string(),
                    "created",
                    &dir
                )
                .is_err()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_files_errors_for_invalid_tracked_path() {
        let dir = std::env::temp_dir().join(format!(
            "cortex_checkpoint_invalid_path_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);
        checkpoint.files.push(CheckpointFile {
            path: "../outside.txt".to_string(),
            agent: "pm".to_string(),
            phase: "specs-ready".to_string(),
            operation: "created".to_string(),
            bytes: 0,
            sha256: sha256_bytes(b"outside"),
            updated_at_unix_ms: now_unix_ms(),
        });

        let err = checkpoint.validate_files(&dir).unwrap_err().to_string();
        assert!(err.contains("Invalid checkpoint file path"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_checkpoint_json_returns_readable_error() {
        let dir =
            std::env::temp_dir().join(format!("cortex_checkpoint_invalid_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Checkpoint::checkpoint_path(&dir), "{not-json").unwrap();

        let err = Checkpoint::load(&dir).unwrap_err().to_string();
        assert!(err.contains("Failed to parse checkpoint"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dev_checkpoint_helpers_update_state_status_and_timestamp() {
        let config = Config::default();
        let mut checkpoint = Checkpoint::new("run-1", "dev", "build", &config);

        checkpoint.set_dev_brief("brief");
        checkpoint.set_dev_specs_path("specs.md");
        checkpoint.set_dev_architecture_path("architecture.md");
        checkpoint.set_dev_expected_files(vec!["src/main.rs".to_string()]);
        checkpoint.set_dev_qa_iteration(2);

        assert_eq!(checkpoint.dev.brief.as_deref(), Some("brief"));
        assert_eq!(checkpoint.dev.specs_path.as_deref(), Some("specs.md"));
        assert_eq!(
            checkpoint.dev.architecture_path.as_deref(),
            Some("architecture.md")
        );
        assert_eq!(checkpoint.dev.expected_files, vec!["src/main.rs"]);
        assert_eq!(checkpoint.dev.qa_iteration, 2);

        checkpoint.mark_interrupted();
        assert_eq!(checkpoint.status, CheckpointStatus::Interrupted);

        checkpoint.mark_failed();
        assert_eq!(checkpoint.status, CheckpointStatus::Failed);

        checkpoint.mark_completed();
        assert_eq!(checkpoint.status, CheckpointStatus::Completed);
        assert_eq!(checkpoint.current_phase, "done");
        assert_eq!(checkpoint.next_action, "none");
        assert!(checkpoint.completed_phases.contains(&"done".to_string()));
    }
}
