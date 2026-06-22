#![allow(dead_code)]

use anyhow::Result;
use rig::providers::groq as rig_groq;

pub fn client(api_key: &str) -> Result<rig_groq::Client> {
    rig_groq::Client::new(api_key).map_err(|e| anyhow::anyhow!("Groq client init failed: {e}"))
}
