use anyhow::{Context, Result, bail};
use chrono::Utc;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT};
use rig::message::UserContent;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
};

use crate::auth::{AuthMethod, AuthRecord, AuthStore};

const OPENAI_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_ISSUER: &str = "https://auth.openai.com";
// Official Responses API — works with ChatGPT OAuth tokens obtained via
// codex_cli_simplified_flow=true (no paid API key required for ChatGPT subscribers)
// /v1/chat/completions works with the OAuth token from codex_cli_simplified_flow=true.
// /v1/responses requires api.responses.write scope that the OAuth flow does NOT grant.
const OPENAI_CODEX_ENDPOINT: &str = "https://api.openai.com/v1/chat/completions";
const OPENAI_OAUTH_PORT: u16 = 1455;
const OPENAI_OAUTH_CALLBACK_PATH: &str = "/auth/callback";

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: &'static str,
    pub content: String,
}

pub async fn chatgpt_browser_auth() -> Result<AuthRecord> {
    chatgpt_browser_auth_with_url(|auth_url| {
        eprintln!("Open this URL to connect ChatGPT Plus/Pro:\n{auth_url}");
        Ok(())
    })
    .await
}

pub async fn chatgpt_browser_auth_with_url<F>(on_url: F) -> Result<AuthRecord>
where
    F: FnOnce(&str) -> Result<()>,
{
    let listener = TcpListener::bind(("::1", OPENAI_OAUTH_PORT))
        .await
        .context(
            "failed to bind OpenAI OAuth callback server on localhost:1455; close any running opencode/codex auth flow and retry",
        )?;
    let redirect_uri = format!("http://localhost:{OPENAI_OAUTH_PORT}{OPENAI_OAUTH_CALLBACK_PATH}");
    let verifier = generate_verifier();
    let challenge = pkce_challenge(&verifier);
    let state = generate_verifier();
    let auth_url = build_openai_authorize_url(&redirect_uri, &challenge, &state);

    on_url(&auth_url)?;

    let (mut socket, _) = listener
        .accept()
        .await
        .context("failed to accept OpenAI OAuth callback")?;
    let mut buf = vec![0_u8; 8192];
    let n = socket
        .read(&mut buf)
        .await
        .context("failed to read OpenAI OAuth callback")?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or_default();
    let target = first_line.split_whitespace().nth(1).unwrap_or_default();
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let params = parse_query(query);

    let response = b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 54\r\n\r\nCortex authorization complete. You can close this tab.";
    let _ = socket.write_all(response).await;

    if params
        .get("state")
        .is_some_and(|returned| returned.as_str() != state.as_str())
    {
        bail!("OpenAI OAuth callback state did not match");
    }
    let code = params
        .get("code")
        .context("OpenAI OAuth callback did not include a code")?;
    exchange_openai_code(code, &redirect_uri, &verifier).await
}

pub async fn refresh_chatgpt_auth(record: &AuthRecord) -> Result<AuthRecord> {
    let refresh = record
        .refresh_token
        .as_deref()
        .context("OpenAI ChatGPT auth record has no refresh token")?;
    let client = reqwest::Client::new();
    let tokens: OpenAiTokenResponse = client
        .post(format!("{OPENAI_ISSUER}/oauth/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", OPENAI_CLIENT_ID),
            ("refresh_token", refresh),
        ])
        .send()
        .await
        .context("OpenAI OAuth refresh request failed")?
        .error_for_status()
        .context("OpenAI OAuth refresh request was rejected")?
        .json()
        .await
        .context("OpenAI OAuth refresh response was invalid")?;
    Ok(openai_auth_record(tokens, record.account_id.clone()))
}

pub async fn chatgpt_codex_complete(
    model: &str,
    preamble: &str,
    turns: &[ChatTurn],
) -> Result<String> {
    let record = usable_auth_record("openai").await?;
    let token = record
        .access_token
        .as_deref()
        .context("OpenAI ChatGPT auth is missing an access token")?;
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("cortex"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(account_id) = record.account_id.as_deref() {
        headers.insert("ChatGPT-Account-Id", HeaderValue::from_str(account_id)?);
    }

    // /v1/chat/completions — standard format, no special scope needed
    let body = json!({
        "model": model,
        "messages": openai_messages(preamble, turns),
        "stream": false
    });
    post_json_for_text(OPENAI_CODEX_ENDPOINT, headers, body).await
}

pub async fn github_copilot_complete(
    model: &str,
    preamble: &str,
    turns: &[ChatTurn],
) -> Result<String> {
    let record = usable_auth_record("github_copilot").await?;
    let token = record
        .access_token
        .as_deref()
        .context("GitHub Copilot auth is missing an access token")?;
    let messages = openai_messages(preamble, turns);
    let body = json!({
        "model": model,
        "messages": messages,
        "stream": false
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("cortex"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "Openai-Intent",
        HeaderValue::from_static("conversation-edits"),
    );
    headers.insert("x-initiator", HeaderValue::from_static("cortex"));
    post_json_for_text(
        "https://api.githubcopilot.com/chat/completions",
        headers,
        body,
    )
    .await
}

pub async fn vertex_complete(model: &str, preamble: &str, turns: &[ChatTurn]) -> Result<String> {
    let token = google_access_token().await?;
    let project = std::env::var("GOOGLE_CLOUD_PROJECT")
        .or_else(|_| std::env::var("GCLOUD_PROJECT"))
        .context("GOOGLE_CLOUD_PROJECT is required for google_vertex")?;
    let location = std::env::var("GOOGLE_CLOUD_LOCATION")
        .or_else(|_| std::env::var("GOOGLE_VERTEX_LOCATION"))
        .unwrap_or_else(|_| "us-central1".to_string());
    let model_path = if model.contains('/') {
        model.to_string()
    } else {
        format!("publishers/google/models/{model}")
    };
    let url = format!(
        "https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/{model_path}:generateContent"
    );
    let mut contents = Vec::new();
    for turn in turns {
        contents.push(json!({
            "role": if turn.role == "assistant" { "model" } else { "user" },
            "parts": [{"text": turn.content}]
        }));
    }
    let body = json!({
        "systemInstruction": {"parts": [{"text": preamble}]},
        "contents": contents
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let value = post_json(&url, headers, body).await?;
    extract_text(&value).context("Vertex response did not contain text")
}

pub async fn openai_compatible_complete(
    base_url: &str,
    token: &str,
    model: &str,
    preamble: &str,
    turns: &[ChatTurn],
) -> Result<String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "messages": openai_messages(preamble, turns),
        "stream": false
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    post_json_for_text(&url, headers, body).await
}

pub fn message_turns_from_prompt(prompt: &str) -> Vec<ChatTurn> {
    vec![ChatTurn {
        role: "user",
        content: prompt.to_string(),
    }]
}

pub fn message_turns_from_history(
    history: &[rig::completion::Message],
    prompt: &str,
) -> Vec<ChatTurn> {
    let mut turns = Vec::new();
    for message in history {
        match message {
            rig::completion::Message::User { content } => {
                let text = content
                    .iter()
                    .filter_map(|item| match item {
                        UserContent::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    turns.push(ChatTurn {
                        role: "user",
                        content: text,
                    });
                }
            }
            rig::completion::Message::Assistant { content, .. } => {
                let text = content
                    .iter()
                    .filter_map(|item| match item {
                        rig::completion::AssistantContent::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    turns.push(ChatTurn {
                        role: "assistant",
                        content: text,
                    });
                }
            }
            rig::completion::Message::System { .. } => {}
        }
    }
    turns.push(ChatTurn {
        role: "user",
        content: prompt.to_string(),
    });
    turns
}

async fn usable_auth_record(provider: &str) -> Result<AuthRecord> {
    let mut store = AuthStore::load()?;
    let mut record = store
        .record(provider)
        .cloned()
        .with_context(|| format!("{provider} is not connected. Use /connect {provider}"))?;
    if provider == "openai" && record.is_expired() {
        record = refresh_chatgpt_auth(&record).await?;
        store.set(record.clone());
        store.save()?;
    } else if provider == "github_copilot" && record.is_expired() {
        record = crate::auth::refresh_github_copilot(&record).await?;
        store.set(record.clone());
        store.save()?;
    }
    Ok(record)
}

async fn google_access_token() -> Result<String> {
    if let Ok(token) = std::env::var("GOOGLE_OAUTH_ACCESS_TOKEN")
        && !token.is_empty()
    {
        return Ok(token);
    }
    let output = Command::new("gcloud")
        .args(["auth", "application-default", "print-access-token"])
        .output()
        .await
        .context("failed to run gcloud for Google ADC token")?;
    if !output.status.success() {
        bail!("gcloud could not produce an access token for google_vertex");
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        bail!("gcloud returned an empty access token for google_vertex");
    }
    Ok(token)
}

fn openai_messages(preamble: &str, turns: &[ChatTurn]) -> Vec<Value> {
    let mut messages = vec![json!({"role": "system", "content": preamble})];
    messages.extend(
        turns
            .iter()
            .map(|turn| json!({"role": turn.role, "content": turn.content})),
    );
    messages
}

async fn exchange_openai_code(
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<AuthRecord> {
    let client = reqwest::Client::new();
    let tokens: OpenAiTokenResponse = client
        .post(format!("{OPENAI_ISSUER}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", OPENAI_CLIENT_ID),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .context("OpenAI OAuth token exchange failed")?
        .error_for_status()
        .context("OpenAI OAuth token exchange was rejected")?
        .json()
        .await
        .context("OpenAI OAuth token response was invalid")?;
    Ok(openai_auth_record(tokens, None))
}

#[derive(Debug, Deserialize)]
struct OpenAiTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    id_token: Option<String>,
}

fn openai_auth_record(
    tokens: OpenAiTokenResponse,
    previous_account_id: Option<String>,
) -> AuthRecord {
    let account_id = previous_account_id.or_else(|| {
        tokens
            .id_token
            .as_deref()
            .and_then(extract_unverified_account_id)
    });
    AuthRecord {
        provider: "openai".to_string(),
        method: AuthMethod::OAuth,
        access_token: Some(tokens.access_token),
        refresh_token: tokens.refresh_token,
        expires_at: Some(Utc::now() + chrono::Duration::seconds(tokens.expires_in.unwrap_or(3600))),
        account_id,
        base_url: Some(OPENAI_CODEX_ENDPOINT.to_string()),
        extra: Default::default(),
    }
}

fn extract_unverified_account_id(jwt: &str) -> Option<String> {
    let payload = jwt.split('.').nth(1)?;
    let decoded = base64_url_decode(payload)?;
    let value: Value = serde_json::from_slice(&decoded).ok()?;
    value
        .get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .or_else(|| value.get("chatgpt_account_id"))
        .or_else(|| value.pointer("/organizations/0/id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

async fn post_json_for_text(url: &str, headers: HeaderMap, body: Value) -> Result<String> {
    let value = post_json(url, headers, body).await?;
    extract_text(&value).context("provider response did not contain text")
}

async fn post_json(url: &str, headers: HeaderMap, body: Value) -> Result<Value> {
    let resp = reqwest::Client::new()
        .post(url)
        .headers(headers)
        .json(&body)
        .send()
        .await
        .with_context(|| format!("request failed: {url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        // Try to extract a clean error message from JSON error bodies
        let detail = serde_json::from_str::<Value>(&body_text)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .or_else(|| v.get("message"))
                    .or_else(|| v.get("error"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or(body_text);
        bail!("{status} from {url}: {detail}");
    }

    resp.json()
        .await
        .with_context(|| format!("provider returned invalid JSON: {url}"))
}

fn extract_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(text) = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
    {
        return Some(text.to_string());
    }
    if let Some(text) = value
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str)
    {
        return Some(text.to_string());
    }
    if let Some(text) = value.get("content").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(output) = value.get("output").and_then(Value::as_array) {
        let mut parts = Vec::new();
        for item in output {
            if let Some(content) = item.get("content").and_then(Value::as_array) {
                for part in content {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
            }
        }
        if !parts.is_empty() {
            return Some(parts.join(""));
        }
    }
    None
}

fn build_openai_authorize_url(redirect_uri: &str, challenge: &str, state: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", OPENAI_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", "openid profile email offline_access model.request"),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "opencode"),
    ];
    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{OPENAI_ISSUER}/oauth/authorize?{query}")
}

fn generate_verifier() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64_url_encode(&digest)
}

fn base64_url_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = *bytes.get(i + 1).unwrap_or(&0);
        let b2 = *bytes.get(i + 2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        }
        i += 3;
    }
    out
}

fn base64_url_decode(input: &str) -> Option<Vec<u8>> {
    let mut values = Vec::new();
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        };
        values.push(value);
    }
    let mut out = Vec::new();
    for chunk in values.chunks(4) {
        if chunk.len() >= 2 {
            out.push((chunk[0] << 2) | (chunk[1] >> 4));
        }
        if chunk.len() >= 3 {
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        if chunk.len() >= 4 {
            out.push((chunk[2] << 6) | chunk[3]);
        }
    }
    Some(out)
}

fn percent_encode(input: &str) -> String {
    input
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = u8::from_str_radix(&input[i + 1..i + 3], 16)
        {
            out.push(hex);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn parse_query(query: &str) -> std::collections::BTreeMap<String, String> {
    query
        .split('&')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((percent_decode(key), percent_decode(value)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_compatible_text() {
        let value = json!({"choices":[{"message":{"content":"ok"}}]});
        assert_eq!(extract_text(&value).as_deref(), Some("ok"));
    }

    #[test]
    fn pkce_challenge_is_base64_url() {
        let challenge = pkce_challenge("abc");
        assert!(!challenge.contains('='));
        assert!(
            challenge
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }
}
