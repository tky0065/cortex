#![allow(dead_code)]

use anyhow::{bail, Result};
use rig::providers::groq as rig_groq;

pub fn client() -> Result<rig_groq::Client> {
    let api_key = std::env::var("GROQ_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        bail!("GROQ_API_KEY env var is not set");
    }
    rig_groq::Client::new(&api_key)
        .map_err(|e| anyhow::anyhow!("Groq client init failed: {e}"))
}
