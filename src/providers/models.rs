use anyhow::Result;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch the list of model IDs available for a given provider.
/// Falls back to a curated static list on network error or missing API key.
pub async fn fetch_models(provider: &str) -> Result<Vec<String>> {
    match provider {
        "openrouter" => fetch_openrouter().await,
        "ollama"     => fetch_ollama().await,
        "groq"       => fetch_groq().await,
        "together"   => fetch_together().await,
        other        => Ok(static_fallback(other)),
    }
}

// ---------------------------------------------------------------------------
// OpenRouter — public endpoint, no auth required
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OpenRouterModels {
    data: Vec<OpenRouterModel>,
}

#[derive(Deserialize)]
struct OpenRouterModel {
    id: String,
}

async fn fetch_openrouter() -> Result<Vec<String>> {
    let resp = reqwest::get("https://openrouter.ai/api/v1/models")
        .await?
        .error_for_status()?
        .json::<OpenRouterModels>()
        .await?;
    let mut ids: Vec<String> = resp.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Ollama — local REST API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

async fn fetch_ollama() -> Result<Vec<String>> {
    let resp = reqwest::get("http://localhost:11434/api/tags")
        .await?
        .error_for_status()?
        .json::<OllamaTagsResponse>()
        .await?;
    let mut names: Vec<String> = resp.models.into_iter().map(|m| m.name).collect();
    names.sort();
    Ok(names)
}

// ---------------------------------------------------------------------------
// Groq — requires GROQ_API_KEY env var
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[derive(Deserialize)]
struct OpenAiModel {
    id: String,
}

async fn fetch_groq() -> Result<Vec<String>> {
    let key = std::env::var("GROQ_API_KEY").unwrap_or_default();
    if key.is_empty() {
        return Ok(static_fallback("groq"));
    }
    let resp = reqwest::Client::new()
        .get("https://api.groq.com/openai/v1/models")
        .bearer_auth(&key)
        .send()
        .await?
        .error_for_status()?
        .json::<OpenAiModelsResponse>()
        .await?;
    let mut ids: Vec<String> = resp.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Together AI — requires TOGETHER_API_KEY env var
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TogetherModel {
    id: String,
}

async fn fetch_together() -> Result<Vec<String>> {
    let key = std::env::var("TOGETHER_API_KEY").unwrap_or_default();
    if key.is_empty() {
        return Ok(static_fallback("together"));
    }
    let resp = reqwest::Client::new()
        .get("https://api.together.xyz/v1/models")
        .bearer_auth(&key)
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<TogetherModel>>()
        .await?;
    let mut ids: Vec<String> = resp.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Static fallback lists
// ---------------------------------------------------------------------------

fn static_fallback(provider: &str) -> Vec<String> {
    match provider {
        "groq" => vec![
            "llama-3.3-70b-versatile".to_string(),
            "llama-3.1-8b-instant".to_string(),
            "llama3-70b-8192".to_string(),
            "llama3-8b-8192".to_string(),
            "mixtral-8x7b-32768".to_string(),
            "gemma2-9b-it".to_string(),
            "gemma-7b-it".to_string(),
        ],
        "together" => vec![
            "meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo".to_string(),
            "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo".to_string(),
            "meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo".to_string(),
            "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string(),
            "mistralai/Mistral-7B-Instruct-v0.1".to_string(),
            "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(),
        ],
        "ollama" => vec![
            "qwen2.5-coder:32b".to_string(),
            "qwen2.5-coder:14b".to_string(),
            "qwen2.5-coder:7b".to_string(),
            "llama3.1:8b".to_string(),
            "llama3.2:3b".to_string(),
            "mistral:7b".to_string(),
            "codellama:13b".to_string(),
            "deepseek-coder:6.7b".to_string(),
        ],
        "openrouter" => vec![
            "openai/gpt-4o".to_string(),
            "openai/gpt-4o-mini".to_string(),
            "anthropic/claude-3.5-sonnet".to_string(),
            "anthropic/claude-3-haiku".to_string(),
            "google/gemini-2.0-flash-001".to_string(),
            "meta-llama/llama-3.3-70b-instruct".to_string(),
            "mistralai/mixtral-8x7b-instruct".to_string(),
            "qwen/qwen-2.5-coder-32b-instruct".to_string(),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_fallbacks_are_non_empty() {
        for provider in &["groq", "together", "ollama", "openrouter"] {
            let list = static_fallback(provider);
            assert!(!list.is_empty(), "fallback for {provider} should not be empty");
        }
    }

    #[test]
    fn unknown_provider_returns_empty_fallback() {
        assert!(static_fallback("unknown_xyz").is_empty());
    }
}
