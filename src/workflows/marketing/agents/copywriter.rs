#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/copywriter.md");

pub async fn run(strategy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "copywriter".into(),
    });
    send_agent_progress(options, "copywriter", "Redaction des messages marketing");

    let model = crate::providers::model_for_role("copywriter", &options.config)?;
    let prompt = format!(
        "Write marketing copy based on this strategy:\n\n{}",
        strategy
    );
    let copy = crate::providers::complete(model, PREAMBLE, &prompt)
        .await
        .map_err(|e| anyhow::anyhow!("Copywriter agent error: {e}"))?;

    send_agent_summary(options, "copywriter", &copy);
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "copywriter".into(),
        chunk: format!("copy generated ({} chars)", copy.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "copywriter".into(),
    });

    Ok(copy)
}
