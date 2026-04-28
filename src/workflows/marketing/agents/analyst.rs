#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/analyst.md");

pub async fn run(strategy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "analyst".into(),
    });
    send_agent_progress(options, "analyst", "Definition des KPIs et tests");

    let model = crate::providers::model_for_role("analyst", &options.config)?;
    let prompt = format!(
        "Define KPIs and A/B tests for this marketing strategy:\n\n{}",
        strategy
    );
    let metrics = crate::providers::complete(model, PREAMBLE, &prompt, options, "analyst")
        .await
        .map_err(|e| anyhow::anyhow!("Analyst agent error: {e}"))?;

    send_agent_summary(options, "analyst", &metrics);
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "analyst".into(),
        chunk: format!("metrics.md generated ({} chars)", metrics.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "analyst".into(),
    });

    Ok(metrics)
}
