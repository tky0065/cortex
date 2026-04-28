#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/tech_lead.md");

pub async fn run(specs: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "tech_lead".into(),
    });
    send_agent_progress(options, "tech_lead", "Generation de architecture.md");

    let model = crate::providers::model_for_role("tech_lead", &options.config)?;
    let prompt = format!(
        "Generate a complete architecture.md for these specifications:\n\n{}",
        specs
    );
    let arch = crate::providers::complete(model, PREAMBLE, &prompt)
        .await
        .map_err(|e| anyhow::anyhow!("Tech Lead agent error: {e}"))?;

    send_agent_summary(options, "tech_lead", &arch);
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "tech_lead".into(),
        chunk: format!("architecture.md generated ({} chars)", arch.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "tech_lead".into(),
    });

    Ok(arch)
}
