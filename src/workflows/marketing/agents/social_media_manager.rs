#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{
    RunOptions, bus_agent_done, bus_agent_started, send_agent_progress, send_agent_summary,
};

const PREAMBLE_RAW: &str = include_str!("../prompts/social_media_manager.md");

pub async fn run(strategy: &str, copy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "social_media_manager".into(),
    });
    send_agent_progress(
        options,
        "social_media_manager",
        "Planification du calendrier social media",
    );
    bus_agent_started(options, "social_media_manager").await;

    let model = crate::providers::model_for_role("social_media_manager", &options.config)?;
    let prompt = format!(
        "Create a 30-day content calendar.\n\nStrategy:\n{}\n\nCopy:\n{}",
        strategy, copy
    );
    let calendar =
        crate::providers::complete(model, crate::custom_defs::prompt_body(PREAMBLE_RAW), &prompt, options, "social_media_manager")
            .await
            .map_err(|e| anyhow::anyhow!("Social Media Manager agent error: {e}"))?;

    send_agent_summary(options, "social_media_manager", &calendar);
    bus_agent_done(options, "social_media_manager", &calendar).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "social_media_manager".into(),
        chunk: format!("calendar.md generated ({} chars)", calendar.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "social_media_manager".into(),
    });

    Ok(calendar)
}
