#![allow(dead_code)]

use anyhow::{Result, bail};
use rig::providers::together as rig_together;

pub fn client() -> Result<rig_together::Client> {
    let api_key = std::env::var("TOGETHER_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        bail!("TOGETHER_API_KEY env var is not set");
    }
    rig_together::Client::new(&api_key)
        .map_err(|e| anyhow::anyhow!("Together AI client init failed: {e}"))
}
