#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{
    RunOptions, bus_agent_done, bus_agent_started, send_agent_progress, send_agent_summary,
};

const PREAMBLE: &str = include_str!("../prompts/copywriter.md");

pub async fn run(profile: &str, freelancer_context: &str, options: &RunOptions) -> Result<String> {
    let agent_name = "copywriter".to_string();
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: agent_name.clone(),
    });
    send_agent_progress(
        options,
        agent_name.clone(),
        "Redaction de l'email personnalise",
    );
    bus_agent_started(options, "copywriter").await;

    let model = crate::providers::model_for_role("copywriter", &options.config)?;
    let prompt = format!(
        "Write a personalized outreach email.\n\nFreelancer context:\n{}\n\nProspect profile:\n{}",
        freelancer_context, profile
    );
    let email = crate::providers::complete(model, PREAMBLE, &prompt, options, "copywriter")
        .await
        .map_err(|e| anyhow::anyhow!("Copywriter agent error: {e}"))?;

    send_agent_summary(options, agent_name.clone(), &email);
    bus_agent_done(options, "copywriter", &email).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("email generated ({} chars)", email.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });

    Ok(email)
}
