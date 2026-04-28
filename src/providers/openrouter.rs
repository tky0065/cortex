use anyhow::{bail, Result};
use rig::providers::openrouter as rig_openrouter;

pub fn client() -> Result<rig_openrouter::Client> {
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        bail!("OPENROUTER_API_KEY env var is not set");
    }
    rig_openrouter::Client::new(&api_key)
        .map_err(|e| anyhow::anyhow!("OpenRouter client init failed: {e}"))
}
