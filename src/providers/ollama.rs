use anyhow::Result;
use rig::client::Nothing;
use rig::providers::ollama as rig_ollama;

pub fn client() -> Result<rig_ollama::Client> {
    rig_ollama::Client::new(Nothing).map_err(|e| anyhow::anyhow!("Ollama client init failed: {e}"))
}
