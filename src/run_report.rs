use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Success,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Pending,
    Running,
    Done,
    Error,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostStatus {
    Unknown,
    Estimated,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTimelineEvent {
    pub timestamp_unix_ms: u64,
    pub event_type: String,
    pub agent: Option<String>,
    pub phase: Option<String>,
    pub message: Option<String>,
    pub path: Option<String>,
    pub tool: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub agent: String,
    pub model: Option<String>,
    pub status: AgentRunStatus,
    pub started_at_unix_ms: Option<u64>,
    pub finished_at_unix_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub token_chunks: usize,
    pub output_chars: usize,
    pub last_progress: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRunRecord {
    pub agent: String,
    pub tool: String,
    pub label: String,
    pub timestamp_unix_ms: u64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRunRecord {
    pub agent: String,
    pub path: String,
    pub operation: String,
    pub bytes: usize,
    pub sha256: String,
    pub timestamp_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunMetrics {
    pub duration_ms: Option<u64>,
    pub tokens_total: Option<usize>,
    pub token_chunks_total: usize,
    pub output_chars_total: usize,
    pub agent_count: usize,
    pub file_count: usize,
    pub tool_call_count: usize,
    pub cost_status: CostStatus,
    pub estimated_cost_usd: Option<f64>,
    pub cost_notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFailure {
    pub failure_type: String,
    pub message: String,
    pub agent: Option<String>,
    pub phase: Option<String>,
    pub probable_cause: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunReport {
    pub schema_version: u32,
    pub run_id: String,
    pub cortex_version: String,
    pub workflow: String,
    pub prompt: String,
    pub provider: String,
    pub started_at_unix_ms: u64,
    pub finished_at_unix_ms: Option<u64>,
    pub status: RunStatus,
    pub timeline: Vec<RunTimelineEvent>,
    pub agents: Vec<AgentRunRecord>,
    pub tools: Vec<ToolRunRecord>,
    pub files: Vec<FileRunRecord>,
    pub metrics: RunMetrics,
    pub failure: Option<RunFailure>,
}

pub struct RunReportCollector {
    report: RunReport,
    #[allow(dead_code)]
    agent_index: BTreeMap<String, usize>,
    #[allow(dead_code)]
    model_by_role: BTreeMap<String, String>,
}

impl RunReportCollector {
    pub fn new(workflow: impl Into<String>, prompt: impl Into<String>, config: &Config) -> Self {
        Self {
            report: RunReport {
                schema_version: 1,
                run_id: uuid::Uuid::new_v4().to_string(),
                cortex_version: env!("CARGO_PKG_VERSION").to_string(),
                workflow: workflow.into(),
                prompt: prompt.into(),
                provider: config.provider.default.clone(),
                started_at_unix_ms: now_unix_ms(),
                finished_at_unix_ms: None,
                status: RunStatus::Running,
                timeline: Vec::new(),
                agents: Vec::new(),
                tools: Vec::new(),
                files: Vec::new(),
                metrics: RunMetrics {
                    duration_ms: None,
                    tokens_total: None,
                    token_chunks_total: 0,
                    output_chars_total: 0,
                    agent_count: 0,
                    file_count: 0,
                    tool_call_count: 0,
                    cost_status: CostStatus::Unknown,
                    estimated_cost_usd: None,
                    cost_notes:
                        "Provider-specific token accounting and pricing are not enforced yet."
                            .to_string(),
                },
                failure: None,
            },
            agent_index: BTreeMap::new(),
            model_by_role: model_map(config),
        }
    }

    pub fn report(&self) -> &RunReport {
        &self.report
    }
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn model_map(config: &Config) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ceo".to_string(), config.models.ceo.clone()),
        ("pm".to_string(), config.models.pm.clone()),
        ("tech_lead".to_string(), config.models.tech_lead.clone()),
        ("developer".to_string(), config.models.developer.clone()),
        ("qa".to_string(), config.models.qa.clone()),
        ("devops".to_string(), config.models.devops.clone()),
        ("assistant".to_string(), config.models.assistant.clone()),
        ("planner".to_string(), config.models.ceo.clone()),
        ("reviewer".to_string(), config.models.qa.clone()),
        ("security".to_string(), config.models.qa.clone()),
        ("performance".to_string(), config.models.qa.clone()),
        ("reporter".to_string(), config.models.qa.clone()),
        ("strategist".to_string(), config.models.developer.clone()),
        ("copywriter".to_string(), config.models.developer.clone()),
        ("analyst".to_string(), config.models.developer.clone()),
        (
            "social_media_manager".to_string(),
            config.models.developer.clone(),
        ),
        ("researcher".to_string(), config.models.developer.clone()),
        ("profiler".to_string(), config.models.developer.clone()),
        (
            "outreach_manager".to_string(),
            config.models.developer.clone(),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn new_report_has_required_identity_fields() {
        let config = Config::default();
        let collector = RunReportCollector::new("dev", "build a todo app", &config);
        let report = collector.report();

        assert_eq!(report.schema_version, 1);
        assert_eq!(report.workflow, "dev");
        assert_eq!(report.prompt, "build a todo app");
        assert_eq!(report.provider, "ollama");
        assert_eq!(report.status, RunStatus::Running);
        assert!(report.finished_at_unix_ms.is_none());
        assert!(!report.run_id.is_empty());
        assert_eq!(report.metrics.cost_status, CostStatus::Unknown);
        assert!(report.metrics.estimated_cost_usd.is_none());
    }

    #[test]
    fn report_serializes_with_stable_top_level_keys() {
        let config = Config::default();
        let collector = RunReportCollector::new("dev", "build a todo app", &config);
        let json = serde_json::to_value(collector.report()).unwrap();

        assert!(json.get("schema_version").is_some());
        assert!(json.get("run_id").is_some());
        assert!(json.get("cortex_version").is_some());
        assert!(json.get("workflow").is_some());
        assert!(json.get("prompt").is_some());
        assert!(json.get("provider").is_some());
        assert!(json.get("started_at_unix_ms").is_some());
        assert!(json.get("finished_at_unix_ms").is_some());
        assert!(json.get("status").is_some());
        assert!(json.get("timeline").is_some());
        assert!(json.get("agents").is_some());
        assert!(json.get("tools").is_some());
        assert!(json.get("files").is_some());
        assert!(json.get("metrics").is_some());
        assert!(json.get("failure").is_some());
    }
}
