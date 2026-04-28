use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::tui::events::{TuiEvent, TuiSender};

pub mod code_review;
pub mod dev;
pub mod marketing;
pub mod prospecting;

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
    pub resume_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>>,
    /// When true, log all agent prompts/responses to `cortex.log`.
    #[allow(dead_code)]
    pub verbose: bool,
}

#[async_trait]
pub trait Workflow: Send + Sync {
    fn name(&self) -> &str;
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
            "Unknown workflow '{}'. Available: dev, marketing, prospecting, code-review",
            other
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
