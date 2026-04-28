#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/copywriter.md");

pub async fn run(profile: &str, freelancer_context: &str, options: &RunOptions) -> Result<String> {
    let agent_name = "copywriter".to_string();
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: agent_name.clone() });

    let model = crate::providers::model_for_role("copywriter", &options.config)?;
    let prompt = format!(
        "Write a personalized outreach email.\n\nFreelancer context:\n{}\n\nProspect profile:\n{}",
        freelancer_context, profile
    );
    let email = crate::providers::complete(model, PREAMBLE, &prompt).await
        .map_err(|e| anyhow::anyhow!("Copywriter agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("email ready ({} chars)", email.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });

    Ok(email)
}
