#![allow(dead_code)]

pub mod groq;
pub mod ollama;
pub mod openrouter;
pub mod together;

use anyhow::{bail, Result};
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::config::Config;

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
        other => bail!("Unknown agent role: '{}'", other),
    }
}

/// Parses a model string like `"ollama/qwen2.5-coder:32b"` into `("ollama", "qwen2.5-coder:32b")`.
/// If no prefix is found, defaults to `"ollama"`.
fn parse_model(model_str: &str) -> (&str, &str) {
    if let Some((provider, model)) = model_str.split_once('/') {
        (provider, model)
    } else {
        ("ollama", model_str)
    }
}

/// Single entry point for LLM completions. Parses the provider prefix from the
/// model string in config and routes to the correct rig client.
///
/// Supported prefixes: `ollama/`, `openrouter/`, `groq/`, `together/`.
/// Falls back to Ollama when no prefix is present.
pub async fn complete(model_str: &str, preamble: &str, prompt: &str) -> Result<String> {
    let (provider, model) = parse_model(model_str);
    match provider {
        "ollama" => {
            let client = rig_ollama::Client::new(Nothing)
                .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.prompt(prompt).await.map_err(|e| anyhow::anyhow!("Ollama completion error: {e}"))
        }
        "openrouter" => {
            let client = openrouter::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.prompt(prompt).await.map_err(|e| anyhow::anyhow!("OpenRouter completion error: {e}"))
        }
        "groq" => {
            let client = groq::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.prompt(prompt).await.map_err(|e| anyhow::anyhow!("Groq completion error: {e}"))
        }
        "together" => {
            let client = together::client()?;
            let agent = client.agent(model).preamble(preamble).build();
            agent.prompt(prompt).await.map_err(|e| anyhow::anyhow!("Together AI completion error: {e}"))
        }
        other => bail!("Unknown provider prefix '{}' in model string '{}'", other, model_str),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn default_config() -> Config {
        Config::default()
    }

    #[test]
    fn parse_model_with_prefix() {
        assert_eq!(parse_model("ollama/qwen2.5-coder:32b"), ("ollama", "qwen2.5-coder:32b"));
        assert_eq!(parse_model("openrouter/gpt-4o"), ("openrouter", "gpt-4o"));
        assert_eq!(parse_model("groq/llama3-70b-8192"), ("groq", "llama3-70b-8192"));
        assert_eq!(parse_model("together/mistralai/Mixtral"), ("together", "mistralai/Mixtral"));
    }

    #[test]
    fn parse_model_no_prefix_defaults_to_ollama() {
        assert_eq!(parse_model("qwen2.5-coder:32b"), ("ollama", "qwen2.5-coder:32b"));
    }

    #[test]
    fn model_for_role_dev_workflow() {
        let cfg = default_config();
        assert!(model_for_role("ceo", &cfg).is_ok());
        assert!(model_for_role("pm", &cfg).is_ok());
        assert!(model_for_role("tech_lead", &cfg).is_ok());
        assert!(model_for_role("developer", &cfg).is_ok());
        assert!(model_for_role("qa", &cfg).is_ok());
        assert!(model_for_role("devops", &cfg).is_ok());
    }

    #[test]
    fn model_for_role_code_review_workflow() {
        let cfg = default_config();
        assert!(model_for_role("reviewer", &cfg).is_ok());
        assert!(model_for_role("security", &cfg).is_ok());
        assert!(model_for_role("performance", &cfg).is_ok());
        assert!(model_for_role("reporter", &cfg).is_ok());
    }

    #[test]
    fn model_for_role_marketing_workflow() {
        let cfg = default_config();
        assert!(model_for_role("strategist", &cfg).is_ok());
        assert!(model_for_role("copywriter", &cfg).is_ok());
        assert!(model_for_role("analyst", &cfg).is_ok());
        assert!(model_for_role("social_media_manager", &cfg).is_ok());
    }

    #[test]
    fn model_for_role_prospecting_workflow() {
        let cfg = default_config();
        assert!(model_for_role("researcher", &cfg).is_ok());
        assert!(model_for_role("profiler", &cfg).is_ok());
        assert!(model_for_role("outreach_manager", &cfg).is_ok());
    }

    #[test]
    fn model_for_role_unknown_returns_error() {
        let cfg = default_config();
        assert!(model_for_role("unknown_role", &cfg).is_err());
    }
}
