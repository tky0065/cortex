#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/outreach_manager.md");

pub async fn run(profiles_and_emails: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "outreach_manager".into() });

    let model = crate::providers::model_for_role("outreach_manager", &options.config)?;
    let prompt = format!(
        "Organize this outreach campaign and produce a report:\n\n{}",
        profiles_and_emails
    );
    let report = crate::providers::complete(model, PREAMBLE, &prompt).await
        .map_err(|e| anyhow::anyhow!("Outreach Manager agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "outreach_manager".into(),
        chunk: format!("outreach_report.md ready ({} chars)", report.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "outreach_manager".into() });

    Ok(report)
}
