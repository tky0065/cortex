use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub provider: ProviderConfig,
    pub models: ModelConfig,
    pub limits: LimitsConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub default: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelConfig {
    pub ceo: String,
    pub pm: String,
    pub tech_lead: String,
    pub developer: String,
    pub qa: String,
    pub devops: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_qa_iterations: u32,
    pub max_tokens_per_call: u32,
    pub max_parallel_workers: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: ProviderConfig { default: "ollama".to_string() },
            models: ModelConfig {
                ceo:       "qwen2.5-coder:32b".to_string(),
                pm:        "qwen2.5-coder:32b".to_string(),
                tech_lead: "qwen2.5-coder:32b".to_string(),
                developer: "qwen2.5-coder:32b".to_string(),
                qa:        "qwen2.5-coder:14b".to_string(),
                devops:    "qwen2.5-coder:14b".to_string(),
            },
            limits: LimitsConfig {
                max_qa_iterations:    5,
                max_tokens_per_call:  8192,
                max_parallel_workers: 4,
            },
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_dir = Self::config_dir()?;
        let config_path = config_dir.join("config.toml");

        if !config_dir.exists() {
            fs::create_dir_all(&config_dir)
                .with_context(|| format!("Failed to create config dir: {}", config_dir.display()))?;
        }

        if !config_path.exists() {
            let defaults = Config::default();
            let toml_str = toml::to_string_pretty(&defaults)
                .context("Failed to serialize default config")?;
            fs::write(&config_path, &toml_str)
                .with_context(|| format!("Failed to write config: {}", config_path.display()))?;
            return Ok(defaults);
        }

        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config: {}", config_path.display()))?;
        let config: Config = toml::from_str(&raw)
            .with_context(|| format!("Failed to parse config: {}", config_path.display()))?;
        Ok(config)
    }

    fn config_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".cortex"))
    }
}
