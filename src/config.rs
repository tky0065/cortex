use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub default: String,
}

/// Optional API keys stored in config.toml (never required for Ollama).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiKeysConfig {
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsConfig {
    /// Enable web search context injection for all agents. Requires `api_keys.web_search` to be set.
    #[serde(default)]
    pub web_search_enabled: bool,
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
            "openrouter" => self.api_keys.openrouter = Some(key),
            "groq" => self.api_keys.groq = Some(key),
            "together" => self.api_keys.together = Some(key),
            "web_search" => self.api_keys.web_search = Some(key),
            other => anyhow::bail!(
                "Unknown provider '{}'. Valid providers: openrouter, groq, together, web_search",
                other
            ),
        }
        Ok(())
    }

    /// Retrieve the stored API key for a provider (None if not set or not needed).
    #[allow(dead_code)]
    pub fn get_api_key(&self, provider: &str) -> Option<&str> {
        match provider {
            "openrouter" => self.api_keys.openrouter.as_deref(),
            "groq" => self.api_keys.groq.as_deref(),
            "together" => self.api_keys.together.as_deref(),
            "web_search" => self.api_keys.web_search.as_deref(),
            _ => None,
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
