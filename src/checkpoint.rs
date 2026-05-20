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
