use anyhow::Result;
use rig::providers::openrouter as rig_openrouter;

pub fn client(api_key: &str) -> Result<rig_openrouter::Client> {
    rig_openrouter::Client::new(api_key)
        .map_err(|e| anyhow::anyhow!("OpenRouter client init failed: {e}"))
}
