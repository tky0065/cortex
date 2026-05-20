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
    agent_index: BTreeMap<String, usize>,
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

    pub fn record_event(&mut self, event: &crate::tui::events::TuiEvent) {
        match event {
            crate::tui::events::TuiEvent::WorkflowStarted { workflow, agents } => {
                self.report.workflow = workflow.clone();
                for agent in agents {
                    self.ensure_agent(agent);
                }
                self.push_timeline("workflow_started", None, None, Some(workflow), None, None);
            }
            crate::tui::events::TuiEvent::AgentStarted { agent } => {
                let now = now_unix_ms();
                let index = self.ensure_agent(agent);
                let record = &mut self.report.agents[index];
                record.status = AgentRunStatus::Running;
                record.started_at_unix_ms.get_or_insert(now);
                record.finished_at_unix_ms = None;
                record.duration_ms = None;
                self.push_timeline("agent_started", Some(agent), None, None, None, None);
            }
            crate::tui::events::TuiEvent::AgentProgress { agent, message } => {
                let index = self.ensure_agent(agent);
                self.report.agents[index].last_progress = Some(message.clone());
                self.push_timeline(
                    "agent_progress",
                    Some(agent),
                    None,
                    Some(message),
                    None,
                    None,
                );
            }
            crate::tui::events::TuiEvent::AgentSummary { agent, summary } => {
                self.ensure_agent(agent);
                self.push_timeline(
                    "agent_summary",
                    Some(agent),
                    None,
                    Some(summary),
                    None,
                    None,
                );
            }
            crate::tui::events::TuiEvent::TokenChunk { agent, chunk } => {
                let index = self.ensure_agent(agent);
                let record = &mut self.report.agents[index];
                record.token_chunks += 1;
                record.output_chars += chunk.len();
                self.refresh_counts();
            }
            crate::tui::events::TuiEvent::AgentDone { agent } => {
                let now = now_unix_ms();
                let index = self.ensure_agent(agent);
                let record = &mut self.report.agents[index];
                record.status = AgentRunStatus::Done;
                record.finished_at_unix_ms = Some(now);
                record.duration_ms = duration_between(record.started_at_unix_ms, Some(now));
                self.push_timeline("agent_done", Some(agent), None, None, None, None);
            }
            crate::tui::events::TuiEvent::PhaseComplete { phase } => {
                self.push_timeline("phase_complete", None, Some(phase), None, None, None);
            }
            crate::tui::events::TuiEvent::Error { agent, message } => {
                let now = now_unix_ms();
                let index = self.ensure_agent(agent);
                let record = &mut self.report.agents[index];
                record.status = AgentRunStatus::Error;
                record.finished_at_unix_ms = Some(now);
                record.duration_ms = duration_between(record.started_at_unix_ms, Some(now));
                record.errors.push(message.clone());
                self.push_timeline("error", Some(agent), None, Some(message), None, None);
            }
            crate::tui::events::TuiEvent::AgentToolCall { agent, tool, label } => {
                self.ensure_agent(agent);
                self.report.tools.push(ToolRunRecord {
                    agent: agent.clone(),
                    tool: tool.clone(),
                    label: label.clone(),
                    timestamp_unix_ms: now_unix_ms(),
                    status: "started".to_string(),
                });
                self.push_timeline(
                    "tool_call",
                    Some(agent),
                    None,
                    Some(label),
                    None,
                    Some(tool),
                );
                self.refresh_counts();
            }
            crate::tui::events::TuiEvent::WorkflowStats { tokens_total } => {
                self.report.metrics.tokens_total = Some(*tokens_total);
                self.push_timeline(
                    "workflow_stats",
                    None,
                    None,
                    Some(&format!("tokens_total={tokens_total}")),
                    None,
                    None,
                );
            }
            crate::tui::events::TuiEvent::WorkflowComplete {
                output_dir, files, ..
            } => {
                self.push_timeline(
                    "workflow_complete",
                    None,
                    None,
                    Some(output_dir),
                    None,
                    None,
                );
                for path in files {
                    self.report.files.push(FileRunRecord {
                        agent: "workflow".to_string(),
                        path: path.clone(),
                        operation: "reported".to_string(),
                        bytes: 0,
                        sha256: String::new(),
                        timestamp_unix_ms: now_unix_ms(),
                    });
                }
                self.refresh_counts();
            }
            crate::tui::events::TuiEvent::FileWritten {
                agent,
                path,
                old_content,
                new_content,
            } => {
                self.record_file_written(agent, path, old_content.is_none(), new_content);
            }
            crate::tui::events::TuiEvent::WorkflowInterrupted { message } => {
                for agent in &mut self.report.agents {
                    if agent.status == AgentRunStatus::Running {
                        let now = now_unix_ms();
                        agent.status = AgentRunStatus::Interrupted;
                        agent.finished_at_unix_ms = Some(now);
                        agent.duration_ms = duration_between(agent.started_at_unix_ms, Some(now));
                    }
                }
                self.push_timeline(
                    "workflow_interrupted",
                    None,
                    None,
                    Some(message),
                    None,
                    None,
                );
            }
            _ => {}
        }
    }

    pub fn finish_success(&mut self) {
        self.finish(RunStatus::Success, None);
    }

    pub fn finish_error(&mut self, message: impl Into<String>) {
        self.finish(RunStatus::Failed, Some(message.into()));
    }

    pub fn finish_interrupted(&mut self, message: impl Into<String>) {
        self.finish(RunStatus::Interrupted, Some(message.into()));
    }

    fn ensure_agent(&mut self, agent: &str) -> usize {
        if let Some(index) = self.agent_index.get(agent) {
            return *index;
        }

        let index = self.report.agents.len();
        self.report.agents.push(AgentRunRecord {
            agent: agent.to_string(),
            model: model_for_agent_name(agent, &self.model_by_role),
            status: AgentRunStatus::Pending,
            started_at_unix_ms: None,
            finished_at_unix_ms: None,
            duration_ms: None,
            token_chunks: 0,
            output_chars: 0,
            last_progress: None,
            errors: Vec::new(),
        });
        self.agent_index.insert(agent.to_string(), index);
        self.refresh_counts();
        index
    }

    fn push_timeline(
        &mut self,
        event_type: &str,
        agent: Option<&str>,
        phase: Option<&str>,
        message: Option<&str>,
        path: Option<&str>,
        tool: Option<&str>,
    ) {
        self.report.timeline.push(RunTimelineEvent {
            timestamp_unix_ms: now_unix_ms(),
            event_type: event_type.to_string(),
            agent: agent.map(ToString::to_string),
            phase: phase.map(ToString::to_string),
            message: message.map(ToString::to_string),
            path: path.map(ToString::to_string),
            tool: tool.map(ToString::to_string),
        });
    }

    fn finish(&mut self, status: RunStatus, message: Option<String>) {
        let finished_at = now_unix_ms();
        self.report.status = status;
        self.report.finished_at_unix_ms = Some(finished_at);
        self.report.metrics.duration_ms =
            duration_between(Some(self.report.started_at_unix_ms), Some(finished_at));

        if status == RunStatus::Success {
            for agent in &mut self.report.agents {
                if agent.status == AgentRunStatus::Running {
                    agent.status = AgentRunStatus::Done;
                    agent.finished_at_unix_ms = Some(finished_at);
                    agent.duration_ms =
                        duration_between(agent.started_at_unix_ms, Some(finished_at));
                }
            }
            self.report.failure = None;
        } else if let Some(message) = message {
            self.report.failure = Some(RunFailure {
                failure_type: self.infer_failure_type(status, &message),
                message,
                agent: self.last_error_agent(),
                phase: self.last_phase(),
                probable_cause: "See the timeline and agent errors for details.".to_string(),
            });
        }

        self.push_timeline(
            match status {
                RunStatus::Running => "workflow_running",
                RunStatus::Success => "workflow_success",
                RunStatus::Failed => "workflow_failed",
                RunStatus::Interrupted => "workflow_interrupted",
            },
            None,
            None,
            None,
            None,
            None,
        );
        self.refresh_counts();
    }

    fn refresh_counts(&mut self) {
        self.report.metrics.token_chunks_total = self
            .report
            .agents
            .iter()
            .map(|agent| agent.token_chunks)
            .sum();
        self.report.metrics.output_chars_total = self
            .report
            .agents
            .iter()
            .map(|agent| agent.output_chars)
            .sum();
        self.report.metrics.agent_count = self.report.agents.len();
        self.report.metrics.file_count = self.report.files.len();
        self.report.metrics.tool_call_count = self.report.tools.len();
    }

    fn infer_failure_type(&self, status: RunStatus, message: &str) -> String {
        if status == RunStatus::Interrupted {
            return "interrupted".to_string();
        }
        if self.last_error_agent().is_some() {
            return "agent_error".to_string();
        }
        let lower = message.to_ascii_lowercase();
        if lower.contains("interrupt") || lower.contains("abort") {
            "interrupted".to_string()
        } else {
            "workflow_error".to_string()
        }
    }

    fn last_error_agent(&self) -> Option<String> {
        self.report
            .agents
            .iter()
            .rev()
            .find(|agent| !agent.errors.is_empty() || agent.status == AgentRunStatus::Error)
            .map(|agent| agent.agent.clone())
    }

    fn last_phase(&self) -> Option<String> {
        self.report
            .timeline
            .iter()
            .rev()
            .find_map(|event| event.phase.clone())
    }

    fn record_file_written(&mut self, agent: &str, path: &str, created: bool, new_content: &str) {
        let operation = if created { "created" } else { "modified" };
        self.report.files.push(FileRunRecord {
            agent: agent.to_string(),
            path: path.to_string(),
            operation: operation.to_string(),
            bytes: new_content.len(),
            sha256: String::new(),
            timestamp_unix_ms: now_unix_ms(),
        });
        self.push_timeline(
            "file_written",
            Some(agent),
            None,
            Some(operation),
            Some(path),
            None,
        );
        self.refresh_counts();
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

fn duration_between(
    started_at_unix_ms: Option<u64>,
    finished_at_unix_ms: Option<u64>,
) -> Option<u64> {
    finished_at_unix_ms
        .zip(started_at_unix_ms)
        .map(|(finished, started)| finished.saturating_sub(started))
}

fn model_for_agent_name(agent: &str, model_by_role: &BTreeMap<String, String>) -> Option<String> {
    let normalized = agent.trim().to_ascii_lowercase().replace([' ', '-'], "_");

    model_by_role
        .get(&normalized)
        .or_else(|| {
            normalized
                .split_once(':')
                .and_then(|(role, _)| model_by_role.get(role))
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::events::TuiEvent;

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

    #[test]
    fn collector_records_agent_lifecycle_and_metrics() {
        let config = Config::default();
        let mut collector = RunReportCollector::new("dev", "build", &config);

        collector.record_event(&TuiEvent::WorkflowStarted {
            workflow: "dev".to_string(),
            agents: vec!["ceo".to_string(), "developer".to_string()],
        });
        collector.record_event(&TuiEvent::AgentStarted {
            agent: "developer".to_string(),
        });
        collector.record_event(&TuiEvent::AgentProgress {
            agent: "developer".to_string(),
            message: "Working ... (5s)".to_string(),
        });
        collector.record_event(&TuiEvent::TokenChunk {
            agent: "developer".to_string(),
            chunk: "hello ".to_string(),
        });
        collector.record_event(&TuiEvent::TokenChunk {
            agent: "developer".to_string(),
            chunk: "world".to_string(),
        });
        collector.record_event(&TuiEvent::AgentDone {
            agent: "developer".to_string(),
        });
        collector.finish_success();

        let report = collector.report();
        assert_eq!(report.status, RunStatus::Success);
        assert_eq!(report.agents.len(), 2);

        let developer = report
            .agents
            .iter()
            .find(|agent| agent.agent == "developer")
            .unwrap();
        assert_eq!(developer.status, AgentRunStatus::Done);
        assert_eq!(developer.model.as_deref(), Some("qwen2.5-coder:32b"));
        assert_eq!(developer.token_chunks, 2);
        assert_eq!(developer.output_chars, "hello world".len());
        assert_eq!(developer.last_progress.as_deref(), Some("Working ... (5s)"));
        assert!(developer.duration_ms.is_some());
        assert_eq!(report.metrics.token_chunks_total, 2);
        assert_eq!(report.metrics.output_chars_total, "hello world".len());
        assert_eq!(report.metrics.agent_count, 2);
    }

    #[test]
    fn collector_records_phase_error_stats_and_failure() {
        let config = Config::default();
        let mut collector = RunReportCollector::new("dev", "build", &config);

        collector.record_event(&TuiEvent::AgentStarted {
            agent: "qa".to_string(),
        });
        collector.record_event(&TuiEvent::PhaseComplete {
            phase: "qa".to_string(),
        });
        collector.record_event(&TuiEvent::WorkflowStats { tokens_total: 1234 });
        collector.record_event(&TuiEvent::Error {
            agent: "qa".to_string(),
            message: "tests failed".to_string(),
        });
        collector.finish_error("workflow failed: tests failed");

        let report = collector.report();
        assert_eq!(report.status, RunStatus::Failed);
        assert_eq!(report.metrics.tokens_total, Some(1234));
        assert_eq!(report.failure.as_ref().unwrap().failure_type, "agent_error");
        assert_eq!(
            report.failure.as_ref().unwrap().agent.as_deref(),
            Some("qa")
        );
        assert!(
            report
                .timeline
                .iter()
                .any(|event| event.event_type == "phase_complete")
        );
    }

    #[test]
    fn collector_records_interruption() {
        let config = Config::default();
        let mut collector = RunReportCollector::new("dev", "build", &config);

        collector.record_event(&TuiEvent::WorkflowInterrupted {
            message: "Interrupted by user".to_string(),
        });
        collector.finish_interrupted("Workflow aborted.");

        let report = collector.report();
        assert_eq!(report.status, RunStatus::Interrupted);
        assert_eq!(report.failure.as_ref().unwrap().failure_type, "interrupted");
        assert!(report.finished_at_unix_ms.is_some());
    }

    #[test]
    fn collector_does_not_store_raw_token_chunks_in_timeline() {
        let config = Config::default();
        let mut collector = RunReportCollector::new("dev", "build", &config);

        for i in 0..100 {
            collector.record_event(&TuiEvent::TokenChunk {
                agent: "developer".to_string(),
                chunk: format!("chunk-{i} "),
            });
        }

        assert_eq!(collector.report().metrics.token_chunks_total, 100);
        assert!(
            collector
                .report()
                .timeline
                .iter()
                .all(|event| event.event_type != "token_chunk")
        );
    }
}
