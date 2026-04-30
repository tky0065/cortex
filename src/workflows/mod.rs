use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::agent_bus::{AgentBus, AgentStatus};
use crate::config::Config;
use crate::tui::events::{TuiEvent, TuiSender};

pub mod code_review;
pub mod dev;
pub mod marketing;
pub mod prospecting;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkflowInfo {
    pub name: &'static str,
    pub description: &'static str,
}

pub const AVAILABLE_WORKFLOWS: &[WorkflowInfo] = &[
    WorkflowInfo {
        name: "dev",
        description: "Software development workflow",
    },
    WorkflowInfo {
        name: "marketing",
        description: "Marketing and content workflow",
    },
    WorkflowInfo {
        name: "prospecting",
        description: "Freelance outreach workflow",
    },
    WorkflowInfo {
        name: "code-review",
        description: "Review an existing project",
    },
];

pub fn available_workflows() -> &'static [WorkflowInfo] {
    AVAILABLE_WORKFLOWS
}

pub fn available_workflow_names() -> String {
    available_workflows()
        .iter()
        .map(|workflow| workflow.name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Clone)]
pub struct RunOptions {
    pub auto: bool,
    pub config: Arc<Config>,
    pub tx: TuiSender,
    pub project_dir: std::path::PathBuf,
    /// Token used to cancel an in-flight workflow (Task 32).
    pub cancel: CancellationToken,
    /// Sender used by the REPL `/continue` to resume an interactive pause (Task 30).
    #[allow(dead_code)]
    pub resume_tx: Arc<tokio::sync::mpsc::Sender<()>>,
    /// Receiver side of the resume channel, wrapped in Mutex for shared access across clones.
    #[allow(dead_code)]
    pub resume_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>>,
    /// Sender used by the TUI to deliver a text answer back to a waiting agent.
    #[allow(dead_code)]
    pub answer_tx: Arc<tokio::sync::mpsc::Sender<String>>,
    /// Receiver side of the answer channel — agents await here after emitting UserQuestion.
    pub answer_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<String>>>,
    /// When true, log all agent prompts/responses to `cortex.log`.
    #[allow(dead_code)]
    pub verbose: bool,
    /// Shared inter-agent communication bus (optional — absent in legacy tests).
    pub agent_bus: Option<Arc<AgentBus>>,
}

#[async_trait]
pub trait Workflow: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &str;
    #[allow(dead_code)]
    fn description(&self) -> &str;
    async fn run(&self, prompt: String, options: RunOptions) -> Result<()>;
}

pub fn get_workflow(name: &str) -> Result<Box<dyn Workflow>> {
    match name {
        "dev" => Ok(Box::new(dev::DevWorkflow)),
        "marketing" => Ok(Box::new(marketing::MarketingWorkflow)),
        "prospecting" => Ok(Box::new(prospecting::ProspectingWorkflow)),
        "code-review" => Ok(Box::new(code_review::CodeReviewWorkflow)),
        other => anyhow::bail!(
            "Unknown workflow '{}'. Available: {}",
            other,
            available_workflow_names()
        ),
    }
}

pub fn send_agent_progress(
    options: &RunOptions,
    agent: impl Into<String>,
    message: impl Into<String>,
) {
    let _ = options.tx.send(TuiEvent::AgentProgress {
        agent: agent.into(),
        message: message.into(),
    });
}

pub fn send_agent_summary(options: &RunOptions, agent: impl Into<String>, output: &str) {
    let _ = options.tx.send(TuiEvent::AgentSummary {
        agent: agent.into(),
        summary: summarize_output(output),
    });
}

pub fn summarize_output(output: &str) -> String {
    let mut lines = Vec::new();

    for raw in output.lines() {
        let line = raw
            .trim()
            .trim_start_matches(['#', '-', '*', ' ', '\t'])
            .trim();
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        lines.push(truncate_line(line, 110));
        if lines.len() == 3 {
            break;
        }
    }

    if lines.is_empty() {
        "Completed; no text summary was produced.".to_string()
    } else {
        lines.join("\n")
    }
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let mut chars = line.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated.trim_end())
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_workflow_names_match_registry() {
        assert_eq!(
            available_workflow_names(),
            "dev, marketing, prospecting, code-review"
        );
    }

    #[test]
    fn unknown_workflow_error_lists_available_workflows() {
        let err = match get_workflow("unknown") {
            Ok(_) => panic!("unknown workflow should fail"),
            Err(err) => err.to_string(),
        };
        assert!(err.contains("Unknown workflow 'unknown'"));
        assert!(err.contains("Available: dev, marketing, prospecting, code-review"));
    }
}

// ---------------------------------------------------------------------------
// AgentBus helpers (called from individual agents)
// ---------------------------------------------------------------------------

/// Report an agent as running to the bus (if one is present in options).
pub async fn bus_agent_started(options: &RunOptions, name: &str) {
    if let Some(ref bus) = options.agent_bus {
        bus.update_agent(name, AgentStatus::Running, None).await;
    }
}

/// Report an agent as done (with its output) to the bus.
pub async fn bus_agent_done(options: &RunOptions, name: &str, output: &str) {
    if let Some(ref bus) = options.agent_bus {
        bus.update_agent(name, AgentStatus::Done, Some(output.to_string()))
            .await;
    }
}

/// Report an agent as failed to the bus.
#[allow(dead_code)]
pub async fn bus_agent_error(options: &RunOptions, name: &str, error: &str) {
    if let Some(ref bus) = options.agent_bus {
        bus.update_agent(name, AgentStatus::Error(error.to_string()), None)
            .await;
    }
}

/// Drain any pending directives from the bus and log them to the TUI.
/// Returns the drained directives so the caller can act on them.
pub async fn drain_and_log_directives(
    options: &RunOptions,
    phase: &str,
) -> Vec<crate::agent_bus::AgentDirective> {
    if let Some(ref bus) = options.agent_bus {
        let directives = bus.drain_directives().await;
        for d in &directives {
            let _ = options.tx.send(TuiEvent::TokenChunk {
                agent: "orchestrator".into(),
                chunk: format!(
                    "[{}] Directive for '{}': {}",
                    phase, d.target_agent, d.instruction
                ),
            });
        }
        return directives;
    }
    Vec::new()
}
