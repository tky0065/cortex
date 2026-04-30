use anyhow::{Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_bedrockruntime::{Client, primitives::Blob};
use serde_json::{Value, json};

use super::custom_http::ChatTurn;

pub async fn complete(model: &str, preamble: &str, turns: &[ChatTurn]) -> Result<String> {
    let region = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .unwrap_or_else(|_| "us-east-1".to_string());
    let config = aws_config::defaults(BehaviorVersion::latest())
        .region(aws_config::Region::new(region))
        .load()
        .await;
    let client = Client::new(&config);
    let body = if model.starts_with("anthropic.") {
        anthropic_body(preamble, turns)
    } else {
        generic_body(preamble, turns)
    };
    let response = client
        .invoke_model()
        .model_id(model)
        .content_type("application/json")
        .accept("application/json")
        .body(Blob::new(serde_json::to_vec(&body)?))
        .send()
        .await
        .with_context(|| format!("Bedrock invoke_model failed for {model}"))?;
    let bytes = response.body().as_ref();
    let value: Value = serde_json::from_slice(bytes).context("Bedrock returned invalid JSON")?;
    extract_bedrock_text(&value).context("Bedrock response did not contain text")
}

fn anthropic_body(preamble: &str, turns: &[ChatTurn]) -> Value {
    let messages = turns
        .iter()
        .map(|turn| {
            json!({
                "role": if turn.role == "assistant" { "assistant" } else { "user" },
                "content": [{"type": "text", "text": turn.content}]
            })
        })
        .collect::<Vec<_>>();
    json!({
        "anthropic_version": "bedrock-2023-05-31",
        "system": preamble,
        "messages": messages,
        "max_tokens": 4096
    })
}

fn generic_body(preamble: &str, turns: &[ChatTurn]) -> Value {
    let prompt = turns
        .iter()
        .map(|turn| format!("{}: {}", turn.role, turn.content))
        .collect::<Vec<_>>()
        .join("\n");
    json!({
        "system": [{"text": preamble}],
        "messages": [{"role": "user", "content": [{"text": prompt}]}],
        "inferenceConfig": {"maxTokens": 4096}
    })
}

fn extract_bedrock_text(value: &Value) -> Option<String> {
    if let Some(content) = value.get("content").and_then(Value::as_array) {
        let text = content
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        if !text.is_empty() {
            return Some(text);
        }
    }
    if let Some(text) = value
        .pointer("/output/message/content/0/text")
        .and_then(Value::as_str)
    {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("outputText").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_anthropic_bedrock_text() {
        let value = json!({"content":[{"type":"text","text":"hello"}]});
        assert_eq!(extract_bedrock_text(&value).as_deref(), Some("hello"));
    }
}
