#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/pm.md");

pub async fn run(brief: &str, options: &RunOptions) -> Result<String> {
    let _ = options
        .tx
        .send(TuiEvent::AgentStarted { agent: "pm".into() });
    send_agent_progress(options, "pm", "Redaction de specs.md");

    let model = crate::providers::model_for_role("pm", &options.config)?;
    let prompt = format!(
        "Generate a complete specs.md for this project brief:\n\n{}",
        brief
    );
    let specs = crate::providers::complete(model, PREAMBLE, &prompt, options, "pm")
        .await
        .map_err(|e| anyhow::anyhow!("PM agent error: {e}"))?;

    send_agent_summary(options, "pm", &specs);
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "pm".into(),
        chunk: format!("specs.md generated ({} chars)", specs.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "pm".into() });

    Ok(specs)
}
