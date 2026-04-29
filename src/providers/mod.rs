#![allow(dead_code)]

pub mod groq;
pub mod models;
pub mod ollama;
pub mod openrouter;
pub mod together;

use anyhow::{Result, bail};
use futures_util::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingResult};
use rig::client::CompletionClient;
use rig::completion::{Chat, Message};
use rig::streaming::{StreamedAssistantContent, StreamingChat, StreamingPrompt};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::tui::events::TuiEvent;

/// Returns the full model string (e.g. `"ollama/qwen2.5-coder:32b"`) for a role.
pub fn model_for_role<'a>(role: &str, config: &'a Config) -> Result<&'a str> {
    match role {
        "ceo" => Ok(&config.models.ceo),
        "pm" => Ok(&config.models.pm),
        "tech_lead" => Ok(&config.models.tech_lead),
        "developer" => Ok(&config.models.developer),
        "qa" => Ok(&config.models.qa),
        "devops" => Ok(&config.models.devops),
        // code-review workflow roles — fall back to qa model
        "reviewer" | "security" | "performance" | "reporter" => Ok(&config.models.qa),
        // marketing workflow roles — fall back to developer model
        "strategist" | "copywriter" | "analyst" | "social_media_manager" => {
            Ok(&config.models.developer)
        }
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

struct ToolCallStreamFilter {
    buffer: String,
    inside_tool_call: bool,
}

impl ToolCallStreamFilter {
    fn new() -> Self {
        Self {
            buffer: String::new(),
            inside_tool_call: false,
        }
    }

    fn push(&mut self, chunk: &str) -> Vec<String> {
        const OPEN: &str = "<tool_call>";
        const CLOSE: &str = "</tool_call>";

        self.buffer.push_str(chunk);
        let mut visible = Vec::new();

        loop {
            if self.inside_tool_call {
                if let Some(close_idx) = self.buffer.find(CLOSE) {
                    let after = close_idx + CLOSE.len();
                    self.buffer.drain(..after);
                    self.inside_tool_call = false;
                    continue;
                }

                let keep = longest_suffix_prefix(&self.buffer, CLOSE);
                if self.buffer.len() > keep {
                    self.buffer.drain(..self.buffer.len() - keep);
                }
                break;
            }

            if let Some(open_idx) = self.buffer.find(OPEN) {
                if open_idx > 0 {
                    visible.push(self.buffer[..open_idx].to_string());
                }
                let after = open_idx + OPEN.len();
                self.buffer.drain(..after);
                self.inside_tool_call = true;
                continue;
            }

            let keep = longest_suffix_prefix(&self.buffer, OPEN);
            if self.buffer.len() > keep {
                visible.push(self.buffer[..self.buffer.len() - keep].to_string());
                self.buffer.drain(..self.buffer.len() - keep);
            }
            break;
        }

        visible
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect()
    }

    fn flush(&mut self) -> Option<String> {
        if self.inside_tool_call || self.buffer.is_empty() {
            self.buffer.clear();
            self.inside_tool_call = false;
            return None;
        }
        Some(std::mem::take(&mut self.buffer))
    }
}

fn longest_suffix_prefix(value: &str, pattern: &str) -> usize {
    let max = value.len().min(pattern.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if value.is_char_boundary(value.len() - len)
            && pattern.is_char_boundary(len)
            && value[value.len() - len..] == pattern[..len]
        {
            return len;
        }
    }
    0
}

fn display_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if ch.is_whitespace() {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn send_display_text(tx: &crate::tui::events::TuiSender, agent_name: &str, text: &str) {
    for chunk in display_chunks(text) {
        let _ = tx.send(TuiEvent::TokenChunk {
            agent: agent_name.to_string(),
            chunk,
        });
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
        if let MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)) =
            raw_choice
        {
            cancel.cancel(); // stop heartbeat on first text token
            full_response.push_str(&text.text);
            send_display_text(&options.tx, agent_name, &text.text);
        } else if let MultiTurnStreamItem::FinalResponse(response) = raw_choice
            && full_response.is_empty()
        {
            full_response = response.response().to_string();
        }
    }
    cancel.cancel(); // ensure heartbeat stops even if stream was empty or had no text

    Ok(full_response)
}

async fn consume_chat_stream<R: Clone + Send + 'static>(
    mut stream: StreamingResult<R>,
    tx: &crate::tui::events::TuiSender,
    agent_name: &str,
) -> Result<String> {
    let cancel = CancellationToken::new();
    let cancel_hb = cancel.clone();
    let tx_hb = tx.clone();
    let agent_hb = agent_name.to_string();

    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
        interval.tick().await;
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
    let mut final_response = None;
    let mut filter = ToolCallStreamFilter::new();
    let mut sent_visible = false;

    while let Some(chunk) = stream.next().await {
        let raw_choice = chunk.map_err(|e| anyhow::anyhow!("Stream chunk error: {e}"))?;
        match raw_choice {
            MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(text)) => {
                cancel.cancel();
                full_response.push_str(&text.text);
                for visible in filter.push(&text.text) {
                    sent_visible = true;
                    send_display_text(tx, agent_name, &visible);
                }
            }
            MultiTurnStreamItem::FinalResponse(response) => {
                final_response = Some(response.response().to_string());
            }
            _ => {}
        }
    }
    cancel.cancel();

    if let Some(visible) = filter.flush()
        && !visible.is_empty()
    {
        sent_visible = true;
        send_display_text(tx, agent_name, &visible);
    }

    if full_response.is_empty() {
        if let Some(response) = final_response {
            full_response = response;
            if !sent_visible {
                let visible = crate::assistant::strip_tool_calls_for_display(&full_response);
                if !visible.is_empty() {
                    send_display_text(tx, agent_name, &visible);
                }
            }
        }
    }

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
    let skill_context = if options.config.tools.skills_enabled {
        crate::skills::context_for_prompt(
            agent_name,
            prompt,
            options.config.tools.max_skill_context_chars,
        )
        .unwrap_or_default()
    } else {
        String::new()
    };
    let enriched_preamble = if skill_context.is_empty() {
        preamble.to_string()
    } else {
        format!("{preamble}{skill_context}")
    };
    let project_context = if should_inject_project_context(agent_name) {
        crate::project_context::load_agents_context(16_000)
    } else {
        String::new()
    };
    let enriched_preamble = if project_context.is_empty() {
        enriched_preamble
    } else {
        format!("{enriched_preamble}{project_context}")
    };

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
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        _ => {
            let client = ollama::client()?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
    }
}

fn should_inject_project_context(agent_name: &str) -> bool {
    agent_name != "init"
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
            agent
                .chat(user_msg, history)
                .await
                .map_err(|e| anyhow::anyhow!("OpenRouter chat error: {e}"))
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .chat(user_msg, history)
                .await
                .map_err(|e| anyhow::anyhow!("Groq chat error: {e}"))
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .chat(user_msg, history)
                .await
                .map_err(|e| anyhow::anyhow!("Together chat error: {e}"))
        }
        _ => {
            let client = ollama::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .chat(user_msg, history)
                .await
                .map_err(|e| anyhow::anyhow!("Ollama chat error: {e}"))
        }
    }
}

/// Multi-turn chat completion with streamed visible text events.
pub async fn complete_chat_stream(
    model_str: &str,
    preamble: &str,
    history: Vec<Message>,
    prompt: &str,
    tx: &crate::tui::events::TuiSender,
    agent_name: &str,
) -> Result<String> {
    let (provider, model) = parse_model(model_str);

    match provider {
        "openrouter" => {
            let client = openrouter::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(prompt, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("OpenRouter chat stream error: {e}"))
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(prompt, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Groq chat stream error: {e}"))
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(prompt, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Together chat stream error: {e}"))
        }
        _ => {
            let client = ollama::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(prompt, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Ollama chat stream error: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_chunks_preserve_words_and_spaces() {
        assert_eq!(
            display_chunks("hello world\nok"),
            vec!["hello ", "world\n", "ok"]
        );
    }

    #[test]
    fn tool_call_filter_hides_complete_xml() {
        let mut filter = ToolCallStreamFilter::new();
        let mut out = Vec::new();
        out.extend(filter.push("visible <tool_call><name>x</name></tool_call> done"));
        if let Some(rest) = filter.flush() {
            out.push(rest);
        }
        assert_eq!(out.join(""), "visible  done");
    }

    #[test]
    fn tool_call_filter_hides_split_xml() {
        let mut filter = ToolCallStreamFilter::new();
        let mut out = Vec::new();
        out.extend(filter.push("hello <tool"));
        out.extend(filter.push("_call><name>x</name></tool"));
        out.extend(filter.push("_call> world"));
        if let Some(rest) = filter.flush() {
            out.push(rest);
        }
        assert_eq!(out.join(""), "hello  world");
    }

    #[test]
    fn init_agent_does_not_inject_project_context() {
        assert!(!should_inject_project_context("init"));
        assert!(should_inject_project_context("developer"));
    }
}
