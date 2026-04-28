#![allow(dead_code)]

pub mod models;
pub mod ollama;
pub mod openrouter;
pub mod groq;
pub mod together;

use anyhow::{bail, Result};
use futures_util::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingResult};
use rig::client::CompletionClient;
use rig::completion::{Chat, Message};
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::tui::events::TuiEvent;

/// Returns the full model string (e.g. `"ollama/qwen2.5-coder:32b"`) for a role.
pub fn model_for_role<'a>(role: &str, config: &'a Config) -> Result<&'a str> {
    match role {
        "ceo"         => Ok(&config.models.ceo),
        "pm"          => Ok(&config.models.pm),
        "tech_lead"   => Ok(&config.models.tech_lead),
        "developer"   => Ok(&config.models.developer),
        "qa"          => Ok(&config.models.qa),
        "devops"      => Ok(&config.models.devops),
        // code-review workflow roles — fall back to qa model
        "reviewer"    | "security" | "performance" | "reporter" => Ok(&config.models.qa),
        // marketing workflow roles — fall back to developer model
        "strategist" | "copywriter" | "analyst" | "social_media_manager" => Ok(&config.models.developer),
        // prospecting workflow roles — fall back to developer model
        "researcher" | "profiler" | "outreach_manager" => Ok(&config.models.developer),
        // conversational assistant
        "assistant" => Ok(&config.models.assistant),
        other => bail!("Unknown agent role: '{}'", other),
    }
}

/// Parses a model string like `"ollama/qwen2.5-coder:32b"` into `("ollama", "qwen2.5-coder:32b")`.
fn parse_model(model_str: &str) -> (&str, &str) {
    if let Some((provider, model)) = model_str.split_once('/') {
        (provider, model)
    } else {
        ("ollama", model_str)
    }
}

/// Drains a streaming response into a `String`, forwarding each text token as `TuiEvent::TokenChunk`.
///
/// While waiting for the **first token**, a background heartbeat fires every 5 seconds and sends
/// `TuiEvent::AgentProgress { "Waiting for model response... Xs" }` so the TUI stays visually
/// alive during slow API queue times (e.g. free models on OpenRouter with long TTFT).
async fn consume_stream<R: Clone + Send + 'static>(
    mut stream: StreamingResult<R>,
    options: &crate::workflows::RunOptions,
    agent_name: &str,
) -> Result<String> {
    let cancel = CancellationToken::new();
    let cancel_hb = cancel.clone();
    let tx_hb = options.tx.clone();
    let agent_hb = agent_name.to_string();

    // Background heartbeat — fires every 5s until the first token cancels it.
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        interval.tick().await; // skip the immediate first tick
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let elapsed = start.elapsed().as_secs();
                    let _ = tx_hb.send(TuiEvent::AgentProgress {
                        agent: agent_hb.clone(),
                        message: format!("Waiting for model response... ({}s)", elapsed),
                    });
                }
                _ = cancel_hb.cancelled() => break,
            }
        }
    });

    let mut full_response = String::new();
    while let Some(chunk) = stream.next().await {
        let raw_choice = chunk.map_err(|e| anyhow::anyhow!("Stream chunk error: {e}"))?;
        if let MultiTurnStreamItem::StreamAssistantItem(
            StreamedAssistantContent::Text(text)
        ) = raw_choice {
            cancel.cancel(); // stop heartbeat on first text token
            full_response.push_str(&text.text);
            let _ = options.tx.send(TuiEvent::TokenChunk {
                agent: agent_name.to_string(),
                chunk: text.text,
            });
        }
    }
    cancel.cancel(); // ensure heartbeat stops even if stream was empty or had no text

    Ok(full_response)
}

/// Single entry point for LLM completions with streaming support.
/// Sends `TuiEvent::TokenChunk` to `options.tx` for each token.
/// Sends `TuiEvent::AgentProgress` heartbeats while waiting for the first token.
/// When `options.config.tools.web_search_enabled` is true and a key is configured,
/// appends live web search results to the prompt before sending to the LLM.
pub async fn complete(
    model_str: &str,
    preamble: &str,
    prompt: &str,
    options: &crate::workflows::RunOptions,
    agent_name: &str,
) -> Result<String> {
    // Extract a concise search query from the first 200 chars of the prompt.
    let search_query: String = prompt.chars().take(200).collect();
    let web_context = crate::tools::web_search::fetch_context(&search_query, &options.config).await;
    let enriched_prompt = if web_context.is_empty() {
        prompt.to_string()
    } else {
        format!("{}{}", prompt, web_context)
    };

    let (provider, model) = parse_model(model_str);

    match provider {
        "openrouter" => {
            let client = openrouter::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "ollama" | _ => {
            let client = ollama::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
    }
}

/// Multi-turn chat completion (used by conversational assistant).
pub async fn complete_chat(
    model_str: &str,
    preamble: &str,
    history: Vec<Message>,
    prompt: &str,
) -> Result<String> {
    let (provider, model) = parse_model(model_str);
    let user_msg = Message::user(prompt);
    
    match provider {
        "openrouter" => {
            let client = openrouter::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.chat(user_msg, history).await.map_err(|e| anyhow::anyhow!("OpenRouter chat error: {e}"))
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.chat(user_msg, history).await.map_err(|e| anyhow::anyhow!("Groq chat error: {e}"))
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.chat(user_msg, history).await.map_err(|e| anyhow::anyhow!("Together chat error: {e}"))
        }
        "ollama" | _ => {
            let client = ollama::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.chat(user_msg, history).await.map_err(|e| anyhow::anyhow!("Ollama chat error: {e}"))
        }
    }
}
