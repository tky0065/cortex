use anyhow::Result;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Fetch the list of model IDs available for a given provider.
/// Falls back to a curated static list on network error or missing API key.
pub async fn fetch_models(provider: &str) -> Result<Vec<String>> {
    let provider = super::registry::normalize_provider(provider);
    match provider {
        "openai" => fetch_openai().await,
        "openai_chatgpt" => fetch_openai_chatgpt().await,
        "anthropic" => fetch_anthropic().await,
        "gemini" => fetch_gemini().await,
        "mistral" => fetch_mistral().await,
        "xai" => fetch_openai_compatible("xai", "XAI_API_KEY", "https://api.x.ai/v1/models").await,
        "cohere" => {
            fetch_openai_compatible(
                "cohere",
                "COHERE_API_KEY",
                "https://api.cohere.ai/v1/models",
            )
            .await
        }
        "deepseek" => {
            fetch_openai_compatible(
                "deepseek",
                "DEEPSEEK_API_KEY",
                "https://api.deepseek.com/models",
            )
            .await
        }
        "perplexity" => {
            fetch_openai_compatible(
                "perplexity",
                "PERPLEXITY_API_KEY",
                "https://api.perplexity.ai/models",
            )
            .await
        }
        "huggingface" => Ok(static_fallback("huggingface")),
        "azure_openai" => Ok(static_fallback("azure_openai")),
        "github_copilot" => {
            fetch_openai_compatible(
                "github_copilot",
                "GITHUB_COPILOT_TOKEN",
                "https://api.githubcopilot.com/models",
            )
            .await
        }
        "fireworks" => {
            fetch_openai_compatible(
                "fireworks",
                "FIREWORKS_API_KEY",
                "https://api.fireworks.ai/inference/v1/models",
            )
            .await
        }
        "deepinfra" => {
            fetch_openai_compatible(
                "deepinfra",
                "DEEPINFRA_API_KEY",
                "https://api.deepinfra.com/v1/openai/models",
            )
            .await
        }
        "cerebras" => {
            fetch_openai_compatible(
                "cerebras",
                "CEREBRAS_API_KEY",
                "https://api.cerebras.ai/v1/models",
            )
            .await
        }
        "moonshot" => {
            fetch_openai_compatible(
                "moonshot",
                "MOONSHOT_API_KEY",
                "https://api.moonshot.ai/v1/models",
            )
            .await
        }
        "zai" => {
            fetch_openai_compatible("zai", "ZAI_API_KEY", "https://api.z.ai/api/paas/v4/models")
                .await
        }
        "alibaba" => {
            fetch_openai_compatible(
                "alibaba",
                "ALIBABA_API_KEY",
                "https://dashscope.aliyuncs.com/compatible-mode/v1/models",
            )
            .await
        }
        "minimax" => {
            fetch_openai_compatible(
                "minimax",
                "MINIMAX_API_KEY",
                "https://api.minimax.io/v1/models",
            )
            .await
        }
        "nebius" => {
            fetch_openai_compatible(
                "nebius",
                "NEBIUS_API_KEY",
                "https://api.studio.nebius.com/v1/models",
            )
            .await
        }
        "scaleway" => {
            fetch_openai_compatible(
                "scaleway",
                "SCALEWAY_API_KEY",
                "https://api.scaleway.ai/v1/models",
            )
            .await
        }
        "vercel_ai_gateway" => {
            fetch_openai_compatible(
                "vercel_ai_gateway",
                "VERCEL_AI_GATEWAY_API_KEY",
                "https://ai-gateway.vercel.sh/v1/models",
            )
            .await
        }
        "302ai" | "cloudflare" | "gitlab_duo" | "google_vertex" | "amazon_bedrock" => {
            Ok(static_fallback(provider))
        }
        "lmstudio" => fetch_lmstudio().await,
        "openrouter" => fetch_openrouter().await,
        "ollama" => fetch_ollama().await,
        "groq" => fetch_groq().await,
        "together" => fetch_together().await,
        other => Ok(static_fallback(other)),
    }
}

pub async fn fetch_models_for_config(
    provider: &str,
    config: &crate::config::Config,
) -> Result<Vec<String>> {
    let provider = super::registry::normalize_provider(provider);
    if let Some(custom) = config.custom_providers.get(provider) {
        if !custom.models.is_empty() {
            let mut models = custom.models.clone();
            models.sort();
            return Ok(models);
        }
    }
    fetch_models(provider).await
}

pub fn default_model_for_config(provider: &str, config: &crate::config::Config) -> Option<String> {
    let provider = super::registry::normalize_provider(provider);
    if let Some(custom) = config.custom_providers.get(provider)
        && let Some(model) = custom.models.first()
    {
        return Some(model.clone());
    }
    static_fallback(provider).into_iter().next()
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

async fn fetch_openai() -> Result<Vec<String>> {
    fetch_openai_compatible(
        "openai",
        "OPENAI_API_KEY",
        "https://api.openai.com/v1/models",
    )
    .await
}

async fn fetch_mistral() -> Result<Vec<String>> {
    fetch_openai_compatible(
        "mistral",
        "MISTRAL_API_KEY",
        "https://api.mistral.ai/v1/models",
    )
    .await
}

async fn fetch_anthropic() -> Result<Vec<String>> {
    let key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
    if key.is_empty() {
        return Ok(static_fallback("anthropic"));
    }
    let resp = reqwest::Client::new()
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?
        .error_for_status()?
        .json::<OpenAiModelsResponse>()
        .await?;
    let mut ids: Vec<String> = resp.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}

#[derive(Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModel>,
}

#[derive(Deserialize)]
struct GeminiModel {
    name: String,
}

async fn fetch_gemini() -> Result<Vec<String>> {
    let key = std::env::var("GEMINI_API_KEY")
        .or_else(|_| std::env::var("GOOGLE_API_KEY"))
        .unwrap_or_default();
    if key.is_empty() {
        return Ok(static_fallback("gemini"));
    }
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models?key={key}");
    let resp = reqwest::get(url)
        .await?
        .error_for_status()?
        .json::<GeminiModelsResponse>()
        .await?;
    let mut ids: Vec<String> = resp
        .models
        .into_iter()
        .map(|m| {
            m.name
                .strip_prefix("models/")
                .unwrap_or(&m.name)
                .to_string()
        })
        .collect();
    ids.sort();
    Ok(ids)
}

async fn fetch_openai_compatible(provider: &str, env_var: &str, url: &str) -> Result<Vec<String>> {
    let key = crate::auth::AuthStore::load()
        .ok()
        .and_then(|store| store.bearer_token(provider).map(ToOwned::to_owned))
        .or_else(|| std::env::var(env_var).ok())
        .unwrap_or_default();
    if key.is_empty() {
        return Ok(static_fallback(provider));
    }
    let result: Result<Vec<String>, _> = async {
        let resp = reqwest::Client::new()
            .get(url)
            .bearer_auth(&key)
            .send()
            .await?
            .error_for_status()?
            .json::<OpenAiModelsResponse>()
            .await?;
        let mut ids: Vec<String> = resp.data.into_iter().map(|m| m.id).collect();
        ids.sort();
        anyhow::Ok(ids)
    }
    .await;
    // Fall back to static list on any HTTP/network error (e.g., restricted API keys)
    Ok(result.unwrap_or_else(|_| static_fallback(provider)))
}

/// Fetch ChatGPT-subscription-compatible models.
/// The OAuth token is stored under "openai" in AuthStore (not "openai_chatgpt"),
/// so we check both keys before falling back to the env var / static list.
async fn fetch_openai_chatgpt() -> Result<Vec<String>> {
    let store = crate::auth::AuthStore::load().ok();
    let key = store
        .as_ref()
        .and_then(|s| s.bearer_token("openai_chatgpt").map(ToOwned::to_owned))
        .or_else(|| {
            store
                .as_ref()
                .and_then(|s| s.bearer_token("openai").map(ToOwned::to_owned))
        })
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .unwrap_or_default();

    if key.is_empty() {
        return Ok(static_fallback("openai_chatgpt"));
    }

    let resp = reqwest::Client::new()
        .get("https://api.openai.com/v1/models")
        .bearer_auth(&key)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(_) => return Ok(static_fallback("openai_chatgpt")),
    };

    let resp = match resp.error_for_status() {
        Ok(r) => r,
        Err(_) => return Ok(static_fallback("openai_chatgpt")),
    };

    let models: OpenAiModelsResponse = match resp.json().await {
        Ok(m) => m,
        Err(_) => return Ok(static_fallback("openai_chatgpt")),
    };

    // Filter to ChatGPT-subscription-compatible models (mirrors opencode's codex plugin).
    // gpt-5.1-codex*, including gpt-5.1-codex-mini, are rejected by the backend with
    // "not supported when using Codex with a ChatGPT account".
    let allowed: std::collections::HashSet<&str> = [
        "gpt-5.2",
        "gpt-5.2-codex",
        "gpt-5.3-codex",
        "gpt-5.4",
        "gpt-5.4-mini",
    ]
    .into();

    let mut ids: Vec<String> = models
        .data
        .into_iter()
        .map(|m| m.id)
        .filter(|id| {
            if allowed.contains(id.as_str()) {
                return true;
            }
            // Include any future gpt-5.x where x > 5.4
            if let Some(cap) = id.strip_prefix("gpt-") {
                let version = cap.split('-').next().unwrap_or("");
                if let Ok(v) = version.parse::<f32>() {
                    return v > 5.4;
                }
            }
            false
        })
        .collect();

    if ids.is_empty() {
        return Ok(static_fallback("openai_chatgpt"));
    }

    ids.sort();
    Ok(ids)
}

async fn fetch_lmstudio() -> Result<Vec<String>> {
    let base_url = std::env::var("LMSTUDIO_BASE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    match reqwest::get(url).await {
        Ok(resp) => {
            let resp = resp.error_for_status()?;
            let mut ids: Vec<String> = resp
                .json::<OpenAiModelsResponse>()
                .await?
                .data
                .into_iter()
                .map(|m| m.id)
                .collect();
            ids.sort();
            Ok(ids)
        }
        Err(_) => Ok(Vec::new()),
    }
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
        "openai" => vec![
            "gpt-5.3".to_string(),
            "gpt-5.3-codex".to_string(),
            "gpt-5.2".to_string(),
            "gpt-5-mini".to_string(),
        ],
        "openai_chatgpt" => vec![
            "gpt-5.2".to_string(),
            "gpt-5.2-codex".to_string(),
            "gpt-5.3-codex".to_string(),
            "gpt-5.4".to_string(),
            "gpt-5.4-mini".to_string(),
            "gpt-5.5".to_string(),
            "gpt-5.5-fast".to_string(),
            "gpt-5.5-pro".to_string(),
        ],
        "anthropic" => vec![
            "claude-sonnet-4-6".to_string(),
            "claude-sonnet-4-5".to_string(),
            "claude-haiku-4-5".to_string(),
            "claude-opus-4-1".to_string(),
        ],
        "gemini" => vec![
            "gemini-2.5-pro".to_string(),
            "gemini-2.5-flash".to_string(),
            "gemini-2.0-flash".to_string(),
        ],
        "mistral" => vec![
            "codestral-latest".to_string(),
            "mistral-large-latest".to_string(),
            "mistral-small-latest".to_string(),
            "open-mixtral-8x22b".to_string(),
        ],
        "deepseek" => vec!["deepseek-chat".to_string(), "deepseek-reasoner".to_string()],
        "xai" => vec![
            "grok-4".to_string(),
            "grok-3".to_string(),
            "grok-3-mini".to_string(),
        ],
        "cohere" => vec![
            "command-a-03-2025".to_string(),
            "command-r-plus".to_string(),
            "command-r".to_string(),
        ],
        "perplexity" => vec![
            "sonar-pro".to_string(),
            "sonar".to_string(),
            "sonar-reasoning-pro".to_string(),
        ],
        "huggingface" => vec![
            "moonshotai/Kimi-K2-Instruct".to_string(),
            "zai-org/GLM-4.6".to_string(),
            "Qwen/Qwen3-Coder-480B-A35B-Instruct".to_string(),
        ],
        "azure_openai" => vec![
            "gpt-5.3".to_string(),
            "gpt-5.2".to_string(),
            "gpt-4.1".to_string(),
        ],
        "github_copilot" => vec![
            "gpt-5.1".to_string(),
            "gpt-4.1".to_string(),
            "claude-sonnet-4.5".to_string(),
        ],
        "gitlab_duo" => vec!["claude-sonnet-4".to_string(), "gpt-4.1".to_string()],
        "google_vertex" => vec!["gemini-2.5-pro".to_string(), "gemini-2.5-flash".to_string()],
        "amazon_bedrock" => vec![
            "anthropic.claude-sonnet-4-5".to_string(),
            "amazon.nova-pro".to_string(),
            "meta.llama3-3-70b-instruct".to_string(),
        ],
        "fireworks" => vec![
            "accounts/fireworks/models/qwen3-coder-480b-a35b-instruct".to_string(),
            "accounts/fireworks/models/llama-v3p1-405b-instruct".to_string(),
        ],
        "deepinfra" => vec![
            "Qwen/Qwen3-Coder-480B-A35B-Instruct".to_string(),
            "meta-llama/Llama-3.3-70B-Instruct".to_string(),
        ],
        "cerebras" => vec!["qwen-3-coder-480b".to_string(), "llama3.1-8b".to_string()],
        "moonshot" => vec![
            "kimi-k2-0711-preview".to_string(),
            "moonshot-v1-128k".to_string(),
        ],
        "zai" => vec!["glm-4.6".to_string(), "glm-4.5".to_string()],
        "302ai" => vec!["gpt-4o".to_string(), "claude-3-5-sonnet".to_string()],
        "alibaba" => vec!["qwen-plus".to_string(), "qwen-max".to_string()],
        "cloudflare" => vec![
            "@cf/meta/llama-3.1-8b-instruct".to_string(),
            "@cf/qwen/qwen1.5-14b-chat-awq".to_string(),
        ],
        "minimax" => vec!["MiniMax-M1".to_string(), "abab6.5s-chat".to_string()],
        "nebius" => vec![
            "Qwen/Qwen3-Coder-480B-A35B-Instruct".to_string(),
            "meta-llama/Meta-Llama-3.1-70B-Instruct".to_string(),
        ],
        "scaleway" => vec!["qwen3-coder-30b-a3b-instruct".to_string()],
        "vercel_ai_gateway" => vec![
            "openai/gpt-4.1".to_string(),
            "anthropic/claude-sonnet-4".to_string(),
        ],
        "lmstudio" => vec![],
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
        for provider in &[
            "openai",
            "openai_chatgpt",
            "anthropic",
            "gemini",
            "mistral",
            "deepseek",
            "xai",
            "cohere",
            "perplexity",
            "huggingface",
            "azure_openai",
            "github_copilot",
            "gitlab_duo",
            "google_vertex",
            "amazon_bedrock",
            "fireworks",
            "deepinfra",
            "cerebras",
            "moonshot",
            "zai",
            "302ai",
            "alibaba",
            "cloudflare",
            "minimax",
            "nebius",
            "scaleway",
            "vercel_ai_gateway",
            "groq",
            "together",
            "ollama",
            "openrouter",
        ] {
            let list = static_fallback(provider);
            assert!(
                !list.is_empty(),
                "fallback for {provider} should not be empty"
            );
        }
    }

    #[test]
    fn lmstudio_fallback_is_empty() {
        // LM Studio models are user-specific; the fallback is intentionally empty
        // so the picker shows no fake models when the local server is unreachable.
        assert!(static_fallback("lmstudio").is_empty());
    }

    #[test]
    fn unknown_provider_returns_empty_fallback() {
        assert!(static_fallback("unknown_xyz").is_empty());
    }
}
