#![allow(dead_code)]

use anyhow::Result;
use rig::providers::together as rig_together;

pub fn client(api_key: &str) -> Result<rig_together::Client> {
    rig_together::Client::new(api_key)
        .map_err(|e| anyhow::anyhow!("Together AI client init failed: {e}"))
}
