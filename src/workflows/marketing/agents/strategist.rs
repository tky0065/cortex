#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/strategist.md");

pub async fn run(brief: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "strategist".into(),
    });
    send_agent_progress(
        options,
        "strategist",
        "Construction de la strategie marketing",
    );

    let model = crate::providers::model_for_role("strategist", &options.config)?;
    let prompt = format!("Create a complete marketing strategy for:\n\n{}", brief);
    let strategy = crate::providers::complete(model, PREAMBLE, &prompt, options, "strategist")
        .await
        .map_err(|e| anyhow::anyhow!("Strategist agent error: {e}"))?;

    send_agent_summary(options, "strategist", &strategy);
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "strategist".into(),
        chunk: format!("strategy.md generated ({} chars)", strategy.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "strategist".into(),
    });

    Ok(strategy)
}
