#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, bus_agent_started, bus_agent_done, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/reporter.md");

pub async fn run(combined: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "reporter".into(),
    });
    send_agent_progress(options, "reporter", "Synthese du rapport final");
    bus_agent_started(options, "reporter").await;

    let model = crate::providers::model_for_role("reporter", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, combined, options, "reporter")
        .await
        .map_err(|e| anyhow::anyhow!("Reporter agent error: {e}"))?;

    send_agent_summary(options, "reporter", &response);
    bus_agent_done(options, "reporter", &response).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "reporter".into(),
        chunk: format!("final report generated ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "reporter".into(),
    });

    Ok(response)
}
