#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/pm.md");

pub async fn run(brief: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "pm".into() });

    let model = crate::providers::model_for_role("pm", &options.config)?;
    let prompt = format!(
        "Generate a complete specs.md for this project brief:\n\n{}",
        brief
    );
    let specs = crate::providers::complete(model, PREAMBLE, &prompt).await
        .map_err(|e| anyhow::anyhow!("PM agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "pm".into(),
        chunk: format!("specs.md ready ({} chars)", specs.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "pm".into() });

    Ok(specs)
}
