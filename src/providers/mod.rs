#![allow(dead_code)]

pub mod bedrock;
pub mod custom_http;
pub mod groq;
pub mod models;
pub mod ollama;
pub mod openrouter;
pub mod registry;
pub mod together;

use anyhow::{Result, bail};
use futures_util::StreamExt;
use rig::agent::{MultiTurnStreamItem, StreamingResult};
use rig::client::CompletionClient;
use rig::completion::{Chat, Message};
use rig::streaming::{StreamedAssistantContent, StreamingChat, StreamingPrompt};
use tokio_util::sync::CancellationToken;

use crate::auth::AuthStore;
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
        (registry::normalize_provider(provider), model)
    } else {
        ("", model_str)
    }
}

fn configured_or_env_key(config: &Config, provider: &str) -> Result<String> {
    let provider = registry::normalize_provider(provider);
    if let Ok(store) = AuthStore::load()
        && let Some(record) = store.record(provider)
        && let Some(token) = record.token()
        && (provider == "github_copilot"
            || matches!(
                record.method,
                crate::auth::AuthMethod::ApiKey | crate::auth::AuthMethod::Pat
            ))
    {
        return Ok(token.to_string());
    }
    if let Some(key) = config.get_api_key(provider)
        && !key.is_empty()
    {
        return Ok(key.to_string());
    }
    if let Some(info) = registry::builtin(provider)
        && let Some(env_var) = info.env_var
    {
        let key = std::env::var(env_var).unwrap_or_default();
        if !key.is_empty() {
            return Ok(key);
        }
        bail!("{env_var} env var is not set. Set it with /apikey {provider} <key>");
    }
    bail!("No API key configured for provider '{provider}'");
}

fn custom_key(config: &Config, provider: &str) -> String {
    config
        .custom_providers
        .get(provider)
        .and_then(|custom| {
            custom.api_key.clone().or_else(|| {
                custom
                    .api_key_env
                    .as_ref()
                    .and_then(|env| std::env::var(env).ok())
            })
        })
        .unwrap_or_else(|| "unused".to_string())
}

fn openai_compatible_base_url(provider: &str) -> Option<(&'static str, &'static str)> {
    match registry::normalize_provider(provider) {
        "github_copilot" => Some(("GitHub Copilot", "https://api.githubcopilot.com")),
        "fireworks" => Some(("Fireworks AI", "https://api.fireworks.ai/inference/v1")),
        "deepinfra" => Some(("DeepInfra", "https://api.deepinfra.com/v1/openai")),
        "cerebras" => Some(("Cerebras", "https://api.cerebras.ai/v1")),
        "moonshot" => Some(("Moonshot AI", "https://api.moonshot.ai/v1")),
        "zai" => Some(("Z.ai", "https://api.z.ai/api/paas/v4")),
        "302ai" => Some(("302.AI", "https://api.302.ai/v1")),
        "alibaba" => Some((
            "Alibaba Cloud",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
        )),
        "minimax" => Some(("MiniMax", "https://api.minimax.io/v1")),
        "nebius" => Some(("Nebius AI Studio", "https://api.studio.nebius.com/v1")),
        "scaleway" => Some(("Scaleway", "https://api.scaleway.ai/v1")),
        "vercel_ai_gateway" => Some(("Vercel AI Gateway", "https://ai-gateway.vercel.sh/v1")),
        _ => None,
    }
}

fn unsupported_direct_provider(provider: &str) -> Option<&'static str> {
    match registry::normalize_provider(provider) {
        "gitlab_duo" => Some(
            "GitLab Duo auth can be stored with /connect, but the Duo chat backend adapter is not implemented yet.",
        ),
        _ => None,
    }
}

/// Wraps an LM Studio error and appends actionable guidance when it's a connection failure.
fn lmstudio_connection_hint(e: anyhow::Error) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("error sending request") || msg.contains("Connection refused") {
        anyhow::anyhow!(
            "{msg}\n\nHint: LM Studio's Local Server is not running. \
             Open LM Studio → Developer tab → click \"Start Server\"."
        )
    } else {
        e
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

fn gemini_supports_system_instruction(model: &str) -> bool {
    !model.starts_with("gemma-")
}

fn inline_preamble_prompt(preamble: &str, prompt: &str) -> String {
    if preamble.trim().is_empty() {
        return prompt.to_string();
    }
    format!("Instructions:\n{preamble}\n\nUser request:\n{prompt}")
}

fn send_display_text(tx: &crate::tui::events::TuiSender, agent_name: &str, text: &str) {
    for chunk in display_chunks(text) {
        let _ = tx.send(TuiEvent::TokenChunk {
            agent: agent_name.to_string(),
            chunk,
        });
    }
}

fn emit_text(tx: &crate::tui::events::TuiSender, agent_name: &str, text: &str) {
    send_display_text(tx, agent_name, text);
}

/// Drains a streaming response into a `String`, forwarding each text token as `TuiEvent::TokenChunk`.
///
/// While waiting for the **first token**, a background heartbeat fires every 5 seconds and sends
/// `TuiEvent::AgentProgress { "Working ... Xs" }` so the TUI stays visually
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
                        message: format!("Working ... ({}s)", elapsed),
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
                        message: format!("Working ... ({}s)", elapsed),
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

async fn consume_chatgpt_codex_stream(
    model: &str,
    preamble: &str,
    turns: &[custom_http::ChatTurn],
    tx: &crate::tui::events::TuiSender,
    agent_name: &str,
) -> Result<String> {
    let cancel = chatgpt_heartbeat(tx, agent_name);
    let cancel_delta = cancel.clone();
    let response = custom_http::chatgpt_codex_complete_streaming(model, preamble, turns, |delta| {
        cancel_delta.cancel();
        send_display_text(tx, agent_name, delta);
    })
    .await;
    cancel.cancel();
    response
}

async fn consume_chatgpt_codex_chat_stream(
    model: &str,
    preamble: &str,
    turns: &[custom_http::ChatTurn],
    tx: &crate::tui::events::TuiSender,
    agent_name: &str,
) -> Result<String> {
    let cancel = chatgpt_heartbeat(tx, agent_name);
    let cancel_delta = cancel.clone();
    let mut filter = ToolCallStreamFilter::new();
    let mut sent_visible = false;

    let response = custom_http::chatgpt_codex_complete_streaming(model, preamble, turns, |delta| {
        cancel_delta.cancel();
        for visible in filter.push(delta) {
            sent_visible = true;
            send_display_text(tx, agent_name, &visible);
        }
    })
    .await;
    cancel.cancel();

    let full_response = response?;
    if let Some(visible) = filter.flush()
        && !visible.is_empty()
    {
        sent_visible = true;
        send_display_text(tx, agent_name, &visible);
    }
    if !sent_visible {
        let visible = crate::assistant::strip_tool_calls_for_display(&full_response);
        if !visible.is_empty() {
            send_display_text(tx, agent_name, &visible);
        }
    }
    Ok(full_response)
}

fn chatgpt_heartbeat(tx: &crate::tui::events::TuiSender, agent_name: &str) -> CancellationToken {
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
                        message: format!("Working ... ({}s)", elapsed),
                    });
                }
                _ = cancel_hb.cancelled() => break,
            }
        }
    });

    cancel
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
    let mention_context = crate::mentions::resolve_prompt_mentions(prompt);
    let prompt_with_mentions = if mention_context.is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}{mention_context}")
    };
    let skill_context = if options.config.tools.skills_enabled {
        crate::skills::context_for_prompt(
            agent_name,
            &prompt_with_mentions,
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
    let search_query: String = prompt_with_mentions.chars().take(200).collect();
    let web_context = crate::tools::web_search::fetch_context(&search_query, &options.config).await;
    let enriched_prompt = if web_context.is_empty() {
        prompt_with_mentions
    } else {
        format!("{}{}", prompt_with_mentions, web_context)
    };

    let (provider, model) = parse_model(model_str);
    let provider = if provider.is_empty() {
        registry::normalize_provider(&options.config.provider.default)
    } else {
        provider
    };

    match provider {
        "openai" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::openai::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("OpenAI client init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "anthropic" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::anthropic::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Anthropic client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "gemini" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::gemini::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Gemini client init failed: {e}"))?;
            let stream = if gemini_supports_system_instruction(model) {
                let agent = client.agent(model).preamble(&enriched_preamble).build();
                agent.stream_prompt(&enriched_prompt).await
            } else {
                let agent = client.agent(model).build();
                let prompt = inline_preamble_prompt(&enriched_preamble, &enriched_prompt);
                agent.stream_prompt(&prompt).await
            };
            consume_stream(stream, options, agent_name).await
        }
        "mistral" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::mistral::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Mistral client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "deepseek" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::deepseek::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("DeepSeek client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "xai" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::xai::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("xAI client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "cohere" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::cohere::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Cohere client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "perplexity" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::perplexity::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Perplexity client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "huggingface" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let client = rig::providers::huggingface::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Hugging Face client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        "azure_openai" => {
            let key = configured_or_env_key(&options.config, provider)?;
            let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT").unwrap_or_default();
            if endpoint.is_empty() {
                bail!("AZURE_OPENAI_ENDPOINT env var is not set for azure_openai");
            }
            let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
                .unwrap_or_else(|_| "2024-10-21".to_string());
            let client = rig::providers::azure::Client::builder()
                .api_key(rig::providers::azure::AzureOpenAIAuth::ApiKey(key))
                .azure_endpoint(endpoint)
                .api_version(&api_version)
                .build()
                .map_err(|e| anyhow::anyhow!("Azure OpenAI client init failed: {e}"))?;
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
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
        "lmstudio" => {
            let base_url = std::env::var("LMSTUDIO_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());
            let client = rig::providers::openai::Client::builder()
                .api_key("lm-studio")
                .base_url(base_url)
                .build()
                .map_err(|e| anyhow::anyhow!("LM Studio client init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name)
                .await
                .map_err(lmstudio_connection_hint)
        }
        "google_vertex" => {
            let turns = custom_http::message_turns_from_prompt(&enriched_prompt);
            let response = custom_http::vertex_complete(model, &enriched_preamble, &turns).await?;
            emit_text(&options.tx, agent_name, &response);
            Ok(response)
        }
        "amazon_bedrock" => {
            let turns = custom_http::message_turns_from_prompt(&enriched_prompt);
            let response = bedrock::complete(model, &enriched_preamble, &turns).await?;
            emit_text(&options.tx, agent_name, &response);
            Ok(response)
        }
        "github_copilot" => {
            let turns = custom_http::message_turns_from_prompt(&enriched_prompt);
            let response =
                custom_http::github_copilot_complete(model, &enriched_preamble, &turns).await?;
            emit_text(&options.tx, agent_name, &response);
            Ok(response)
        }
        "openai_chatgpt" => {
            let turns = custom_http::message_turns_from_prompt(&enriched_prompt);
            consume_chatgpt_codex_stream(model, &enriched_preamble, &turns, &options.tx, agent_name)
                .await
        }
        openai_compatible if openai_compatible_base_url(openai_compatible).is_some() => {
            let (name, base_url) = openai_compatible_base_url(openai_compatible)
                .expect("checked openai-compatible provider");
            let key = configured_or_env_key(&options.config, openai_compatible)?;
            let client = rig::providers::openai::Client::builder()
                .api_key(key)
                .base_url(base_url)
                .build()
                .map_err(|e| anyhow::anyhow!("{name} client init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(&enriched_preamble).build();
            let stream = agent.stream_prompt(&enriched_prompt).await;
            consume_stream(stream, options, agent_name).await
        }
        unsupported if unsupported_direct_provider(unsupported).is_some() => {
            bail!(
                "{}",
                unsupported_direct_provider(unsupported).expect("checked unsupported provider")
            );
        }
        custom if options.config.custom_providers.contains_key(custom) => {
            let custom_provider = options
                .config
                .custom_providers
                .get(custom)
                .expect("checked contains_key");
            let client = rig::providers::openai::Client::builder()
                .api_key(custom_key(&options.config, custom))
                .base_url(&custom_provider.base_url)
                .build()
                .map_err(|e| anyhow::anyhow!("Custom provider '{custom}' init failed: {e}"))?
                .completions_api();
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
    let provider = if provider.is_empty() {
        "ollama"
    } else {
        provider
    };
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
        "gemini" => {
            let key = configured_or_env_key(&Config::load()?, provider)?;
            let client = rig::providers::gemini::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Gemini client init failed: {e}"))?;
            if gemini_supports_system_instruction(model) {
                let agent = client.agent(model).preamble(preamble).build();
                agent
                    .chat(user_msg, history)
                    .await
                    .map_err(|e| anyhow::anyhow!("Gemini chat error: {e}"))
            } else {
                let agent = client.agent(model).build();
                let prompt = inline_preamble_prompt(preamble, prompt);
                agent
                    .chat(Message::user(&prompt), history)
                    .await
                    .map_err(|e| anyhow::anyhow!("Gemini chat error: {e}"))
            }
        }
        "openai_chatgpt" => {
            let turns = custom_http::message_turns_from_history(&history, prompt);
            custom_http::chatgpt_codex_complete(model, preamble, &turns).await
        }
        "github_copilot" => {
            let turns = custom_http::message_turns_from_history(&history, prompt);
            custom_http::github_copilot_complete(model, preamble, &turns).await
        }
        "google_vertex" => {
            let turns = custom_http::message_turns_from_history(&history, prompt);
            custom_http::vertex_complete(model, preamble, &turns).await
        }
        "amazon_bedrock" => {
            let turns = custom_http::message_turns_from_history(&history, prompt);
            bedrock::complete(model, preamble, &turns).await
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
    config: &Config,
    tx: &crate::tui::events::TuiSender,
    agent_name: &str,
) -> Result<String> {
    let mention_context = crate::mentions::resolve_prompt_mentions(prompt);
    let prompt_with_mentions = if mention_context.is_empty() {
        prompt.to_string()
    } else {
        format!("{prompt}{mention_context}")
    };
    let (provider, model) = parse_model(model_str);
    let provider = if provider.is_empty() {
        registry::normalize_provider(&config.provider.default)
    } else {
        provider
    };

    match provider {
        "openai" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::openai::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("OpenAI client init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("OpenAI chat stream error: {e}"))
        }
        "anthropic" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::anthropic::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Anthropic client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Anthropic chat stream error: {e}"))
        }
        "gemini" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::gemini::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Gemini client init failed: {e}"))?;
            let stream = if gemini_supports_system_instruction(model) {
                let agent = client.agent(model).preamble(preamble).build();
                agent.stream_chat(&prompt_with_mentions, history).await
            } else {
                let agent = client.agent(model).build();
                let prompt = inline_preamble_prompt(preamble, &prompt_with_mentions);
                agent.stream_chat(&prompt, history).await
            };
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Gemini chat stream error: {e}"))
        }
        "mistral" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::mistral::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Mistral client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Mistral chat stream error: {e}"))
        }
        "deepseek" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::deepseek::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("DeepSeek client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("DeepSeek chat stream error: {e}"))
        }
        "xai" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::xai::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("xAI client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("xAI chat stream error: {e}"))
        }
        "cohere" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::cohere::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Cohere client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Cohere chat stream error: {e}"))
        }
        "perplexity" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::perplexity::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Perplexity client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Perplexity chat stream error: {e}"))
        }
        "huggingface" => {
            let key = configured_or_env_key(config, provider)?;
            let client = rig::providers::huggingface::Client::new(&key)
                .map_err(|e| anyhow::anyhow!("Hugging Face client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Hugging Face chat stream error: {e}"))
        }
        "azure_openai" => {
            let key = configured_or_env_key(config, provider)?;
            let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT").unwrap_or_default();
            if endpoint.is_empty() {
                bail!("AZURE_OPENAI_ENDPOINT env var is not set for azure_openai");
            }
            let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
                .unwrap_or_else(|_| "2024-10-21".to_string());
            let client = rig::providers::azure::Client::builder()
                .api_key(rig::providers::azure::AzureOpenAIAuth::ApiKey(key))
                .azure_endpoint(endpoint)
                .api_version(&api_version)
                .build()
                .map_err(|e| anyhow::anyhow!("Azure OpenAI client init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Azure OpenAI chat stream error: {e}"))
        }
        "openrouter" => {
            let client = openrouter::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("OpenRouter chat stream error: {e}"))
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Groq chat stream error: {e}"))
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Together chat stream error: {e}"))
        }
        "lmstudio" => {
            let base_url = std::env::var("LMSTUDIO_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());
            let client = rig::providers::openai::Client::builder()
                .api_key("lm-studio")
                .base_url(base_url)
                .build()
                .map_err(|e| anyhow::anyhow!("LM Studio client init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(lmstudio_connection_hint)
        }
        "google_vertex" => {
            let turns = custom_http::message_turns_from_history(&history, &prompt_with_mentions);
            let response = custom_http::vertex_complete(model, preamble, &turns).await?;
            emit_text(tx, agent_name, &response);
            Ok(response)
        }
        "amazon_bedrock" => {
            let turns = custom_http::message_turns_from_history(&history, &prompt_with_mentions);
            let response = bedrock::complete(model, preamble, &turns).await?;
            emit_text(tx, agent_name, &response);
            Ok(response)
        }
        "github_copilot" => {
            let turns = custom_http::message_turns_from_history(&history, &prompt_with_mentions);
            let response = custom_http::github_copilot_complete(model, preamble, &turns).await?;
            emit_text(tx, agent_name, &response);
            Ok(response)
        }
        "openai_chatgpt" => {
            let turns = custom_http::message_turns_from_history(&history, &prompt_with_mentions);
            consume_chatgpt_codex_chat_stream(model, preamble, &turns, tx, agent_name).await
        }
        openai_compatible if openai_compatible_base_url(openai_compatible).is_some() => {
            let (name, base_url) = openai_compatible_base_url(openai_compatible)
                .expect("checked openai-compatible provider");
            let key = configured_or_env_key(config, openai_compatible)?;
            let client = rig::providers::openai::Client::builder()
                .api_key(key)
                .base_url(base_url)
                .build()
                .map_err(|e| anyhow::anyhow!("{name} client init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("{name} chat stream error: {e}"))
        }
        unsupported if unsupported_direct_provider(unsupported).is_some() => {
            bail!(
                "{}",
                unsupported_direct_provider(unsupported).expect("checked unsupported provider")
            );
        }
        custom if config.custom_providers.contains_key(custom) => {
            let custom_provider = config
                .custom_providers
                .get(custom)
                .expect("checked contains_key");
            let client = rig::providers::openai::Client::builder()
                .api_key(custom_key(config, custom))
                .base_url(&custom_provider.base_url)
                .build()
                .map_err(|e| anyhow::anyhow!("Custom provider '{custom}' init failed: {e}"))?
                .completions_api();
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
            consume_chat_stream(stream, tx, agent_name)
                .await
                .map_err(|e| anyhow::anyhow!("Custom provider '{custom}' chat stream error: {e}"))
        }
        _ => {
            let client = ollama::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            let stream = agent.stream_chat(&prompt_with_mentions, history).await;
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
    fn gemma_models_do_not_use_system_instruction() {
        assert!(!gemini_supports_system_instruction("gemma-3-12b-it"));
        assert!(!gemini_supports_system_instruction("gemma-3n-e4b-it"));
        assert!(gemini_supports_system_instruction("gemini-2.5-flash"));
        assert!(gemini_supports_system_instruction("gemini-2.5-pro"));
    }

    #[test]
    fn inline_preamble_prompt_preserves_instructions_and_request() {
        assert_eq!(
            inline_preamble_prompt("Be concise.", "hello"),
            "Instructions:\nBe concise.\n\nUser request:\nhello"
        );
        assert_eq!(inline_preamble_prompt("   ", "hello"), "hello");
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

    #[test]
    fn parse_model_normalizes_provider_aliases() {
        assert_eq!(
            parse_model("google/gemini-2.5-pro"),
            ("gemini", "gemini-2.5-pro")
        );
        assert_eq!(
            parse_model("hf/Qwen/Qwen3-Coder"),
            ("huggingface", "Qwen/Qwen3-Coder")
        );
        assert_eq!(
            parse_model("azure/my-deployment"),
            ("azure_openai", "my-deployment")
        );
    }

    #[test]
    fn parse_model_keeps_bare_model_for_default_provider_resolution() {
        assert_eq!(parse_model("qwen2.5-coder:32b"), ("", "qwen2.5-coder:32b"));
    }
}
