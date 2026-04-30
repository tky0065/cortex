use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: ProviderConfig,
    pub models: ModelConfig,
    pub limits: LimitsConfig,
    #[serde(default)]
    pub api_keys: ApiKeysConfig,
    #[serde(default)]
    pub tools: ToolsConfig,
    #[serde(default)]
    pub custom_providers: BTreeMap<String, CustomProviderConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomProviderConfig {
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub default: String,
}

/// Optional API keys stored in config.toml (never required for Ollama).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeysConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mistral: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deepseek: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xai: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cohere: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perplexity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub huggingface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure_openai: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openrouter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groq: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub together: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<String>,
}

/// Optional tools configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    /// Enable web search context injection for all agents. Requires `api_keys.web_search` to be set.
    #[serde(default)]
    pub web_search_enabled: bool,
    /// Enable active Cortex skills context injection for all agents.
    #[serde(default = "default_skills_enabled")]
    pub skills_enabled: bool,
    /// Maximum number of characters of skill context injected into each model call.
    #[serde(default = "default_max_skill_context_chars")]
    pub max_skill_context_chars: usize,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            web_search_enabled: false,
            skills_enabled: default_skills_enabled(),
            max_skill_context_chars: default_max_skill_context_chars(),
        }
    }
}

fn default_skills_enabled() -> bool {
    true
}

fn default_max_skill_context_chars() -> usize {
    12_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub ceo: String,
    pub pm: String,
    pub tech_lead: String,
    pub developer: String,
    pub qa: String,
    pub devops: String,
    /// Conversational assistant model used for free-form chat in the REPL.
    /// Falls back to the CEO model if not set in config.toml.
    #[serde(default = "ModelConfig::default_assistant")]
    pub assistant: String,
}

impl ModelConfig {
    fn default_assistant() -> String {
        "qwen2.5-coder:32b".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_qa_iterations: u32,
    pub max_tokens_per_call: u32,
    pub max_parallel_workers: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderConfig {
                default: "ollama".to_string(),
            },
            models: ModelConfig {
                ceo: "qwen2.5-coder:32b".to_string(),
                pm: "qwen2.5-coder:32b".to_string(),
                tech_lead: "qwen2.5-coder:32b".to_string(),
                developer: "qwen2.5-coder:32b".to_string(),
                qa: "qwen2.5-coder:14b".to_string(),
                devops: "qwen2.5-coder:14b".to_string(),
                assistant: "qwen2.5-coder:32b".to_string(),
            },
            limits: LimitsConfig {
                max_qa_iterations: 5,
                max_tokens_per_call: 8192,
                max_parallel_workers: 4,
            },
            api_keys: ApiKeysConfig::default(),
            tools: ToolsConfig::default(),
            custom_providers: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_path = config_dir.join("config.toml");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).with_context(|| {
                format!("Failed to create config dir: {}", config_dir.display())
            })?;
        }

        if !config_path.exists() {
            let defaults = Config::default();
            let toml_str =
                toml::to_string_pretty(&defaults).context("Failed to serialize default config")?;
            fs::write(&config_path, &toml_str)
                .with_context(|| format!("Failed to write config: {}", config_path.display()))?;
            return Ok(defaults);
        }

        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse config: {}", config_path.display()))?;
        config.apply_api_keys_to_env();
        Ok(config)
    }

    /// Persist the current config to `~/.cortex/config.toml`.
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir()?;
        if !config_dir.exists() {
            fs::create_dir_all(&config_dir).with_context(|| {
                format!("Failed to create config dir: {}", config_dir.display())
            })?;
        }
        let config_path = config_dir.join("config.toml");
        let toml_str = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&config_path, toml_str)
            .with_context(|| format!("Failed to write config: {}", config_path.display()))?;
        Ok(())
    }

    /// Update the model for a named role. Returns an error if the role is unknown.
    pub fn set_model(&mut self, role: &str, model: String) -> Result<()> {
        match role {
            "ceo" => self.models.ceo = model,
            "pm" => self.models.pm = model,
            "tech_lead" => self.models.tech_lead = model,
            "developer" => self.models.developer = model,
            "qa" => self.models.qa = model,
            "devops" => self.models.devops = model,
            "assistant" => self.models.assistant = model,
            "all" => {
                self.models.ceo = model.clone();
                self.models.pm = model.clone();
                self.models.tech_lead = model.clone();
                self.models.developer = model.clone();
                self.models.qa = model.clone();
                self.models.devops = model.clone();
                self.models.assistant = model;
            }
            other => anyhow::bail!(
                "Unknown role '{}'. Valid roles: ceo, pm, tech_lead, developer, qa, devops, assistant, all",
                other
            ),
        }
        Ok(())
    }

    /// Update the default provider name (e.g. `"ollama"`, `"openrouter"`, `"groq"`, `"together"`).
    pub fn set_provider(&mut self, name: String) {
        self.provider.default = name;
    }

    /// Store an API key for a provider. Returns error for unknown providers.
    pub fn set_api_key(&mut self, provider: &str, key: String) -> Result<()> {
        match provider {
            "openai" => self.api_keys.openai = Some(key),
            "anthropic" => self.api_keys.anthropic = Some(key),
            "gemini" | "google" => self.api_keys.gemini = Some(key),
            "mistral" => self.api_keys.mistral = Some(key),
            "deepseek" => self.api_keys.deepseek = Some(key),
            "xai" => self.api_keys.xai = Some(key),
            "cohere" => self.api_keys.cohere = Some(key),
            "perplexity" => self.api_keys.perplexity = Some(key),
            "huggingface" | "hf" => self.api_keys.huggingface = Some(key),
            "azure_openai" | "azure" => self.api_keys.azure_openai = Some(key),
            "openrouter" => self.api_keys.openrouter = Some(key),
            "groq" => self.api_keys.groq = Some(key),
            "together" => self.api_keys.together = Some(key),
            "web_search" => self.api_keys.web_search = Some(key),
            custom if self.custom_providers.contains_key(custom) => {
                if let Some(provider) = self.custom_providers.get_mut(custom) {
                    provider.api_key = Some(key);
                }
            }
            other => anyhow::bail!(
                "Unknown provider '{}'. Valid providers: openai, anthropic, gemini, mistral, deepseek, xai, cohere, perplexity, huggingface, azure_openai, openrouter, groq, together, web_search, or a configured custom provider",
                other
            ),
        }
        Ok(())
    }

    /// Retrieve the stored API key for a provider (None if not set or not needed).
    #[allow(dead_code)]
    pub fn get_api_key(&self, provider: &str) -> Option<&str> {
        match provider {
            "openai" => self.api_keys.openai.as_deref(),
            "anthropic" => self.api_keys.anthropic.as_deref(),
            "gemini" | "google" => self.api_keys.gemini.as_deref(),
            "mistral" => self.api_keys.mistral.as_deref(),
            "deepseek" => self.api_keys.deepseek.as_deref(),
            "xai" => self.api_keys.xai.as_deref(),
            "cohere" => self.api_keys.cohere.as_deref(),
            "perplexity" => self.api_keys.perplexity.as_deref(),
            "huggingface" | "hf" => self.api_keys.huggingface.as_deref(),
            "azure_openai" | "azure" => self.api_keys.azure_openai.as_deref(),
            "openrouter" => self.api_keys.openrouter.as_deref(),
            "groq" => self.api_keys.groq.as_deref(),
            "together" => self.api_keys.together.as_deref(),
            "web_search" => self.api_keys.web_search.as_deref(),
            custom => self
                .custom_providers
                .get(custom)
                .and_then(|provider| provider.api_key.as_deref()),
        }
    }

    /// Toggle web search on or off. Persists to config.toml.
    pub fn set_web_search_enabled(&mut self, enabled: bool) {
        self.tools.web_search_enabled = enabled;
    }

    /// Export stored API keys as environment variables so provider clients can read them.
    pub fn apply_api_keys_to_env(&self) {
        // SAFETY: single-threaded at startup / before any threads are spawned;
        // in TUI context called while holding the config write-lock.
        unsafe {
            if let Some(k) = &self.api_keys.openai {
                std::env::set_var("OPENAI_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.anthropic {
                std::env::set_var("ANTHROPIC_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.gemini {
                std::env::set_var("GEMINI_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.mistral {
                std::env::set_var("MISTRAL_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.deepseek {
                std::env::set_var("DEEPSEEK_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.xai {
                std::env::set_var("XAI_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.cohere {
                std::env::set_var("COHERE_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.perplexity {
                std::env::set_var("PERPLEXITY_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.huggingface {
                std::env::set_var("HUGGINGFACE_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.azure_openai {
                std::env::set_var("AZURE_OPENAI_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.openrouter {
                std::env::set_var("OPENROUTER_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.groq {
                std::env::set_var("GROQ_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.together {
                std::env::set_var("TOGETHER_API_KEY", k);
            }
            if let Some(k) = &self.api_keys.web_search {
                std::env::set_var("WEB_SEARCH_API_KEY", k);
            }
        }
    }

    fn config_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".cortex"))
    }
}
