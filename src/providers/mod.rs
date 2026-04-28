#![allow(dead_code)]

pub mod groq;
pub mod ollama;
pub mod openrouter;
pub mod together;

use anyhow::{bail, Result};

use crate::config::Config;

pub fn model_for_role<'a>(role: &str, config: &'a Config) -> Result<&'a str> {
    match role {
        "ceo"       => Ok(&config.models.ceo),
        "pm"        => Ok(&config.models.pm),
        "tech_lead" => Ok(&config.models.tech_lead),
        "developer" => Ok(&config.models.developer),
        "qa"        => Ok(&config.models.qa),
        "devops"    => Ok(&config.models.devops),
        other       => bail!("Unknown agent role: '{}'", other),
    }
}
