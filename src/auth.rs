use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::providers::registry;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    ApiKey,
    OAuth,
    DeviceCode,
    Pat,
    CredentialChain,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRecord {
    pub provider: String,
    pub method: AuthMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl AuthRecord {
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|expires| expires <= Utc::now())
    }

    pub fn token(&self) -> Option<&str> {
        if self.is_expired() {
            return None;
        }
        self.access_token
            .as_deref()
            .filter(|token| !token.is_empty())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthStore {
    #[serde(default)]
    pub records: BTreeMap<String, AuthRecord>,
}

impl AuthStore {
    pub fn load() -> Result<Self> {
        let path = auth_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read auth store: {}", path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse auth store: {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = auth_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir: {}", parent.display()))?;
        }
        let raw = serde_json::to_string_pretty(self).context("Failed to serialize auth store")?;
        fs::write(&path, raw)
            .with_context(|| format!("Failed to write auth store: {}", path.display()))?;
        restrict_permissions(&path)?;
        Ok(())
    }

    pub fn set(&mut self, record: AuthRecord) {
        self.records.insert(record.provider.clone(), record);
    }

    pub fn record(&self, provider: &str) -> Option<&AuthRecord> {
        let provider = registry::normalize_provider(provider);
        self.records.get(provider)
    }

    pub fn bearer_token(&self, provider: &str) -> Option<&str> {
        self.record(provider).and_then(AuthRecord::token)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthMethodSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub method: AuthMethod,
    pub requires_secret: bool,
}

pub fn auth_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".cortex").join("auth.json"))
}

pub fn methods_for_provider(provider: &str) -> Vec<AuthMethodSpec> {
    let provider = registry::normalize_provider(provider);
    match provider {
        "openai" | "openai_chatgpt" => vec![
            AuthMethodSpec {
                id: "chatgpt_browser",
                label: "ChatGPT Plus/Pro (browser)",
                description: "OAuth PKCE browser flow for ChatGPT Codex backend",
                method: AuthMethod::OAuth,
                requires_secret: false,
            },
            AuthMethodSpec {
                id: "chatgpt_token",
                label: "ChatGPT access token",
                description: "Paste an existing ChatGPT OAuth access token",
                method: AuthMethod::OAuth,
                requires_secret: true,
            },
            api_key_spec("OpenAI Platform API key"),
        ],
        "github_copilot" => vec![
            AuthMethodSpec {
                id: "github_device",
                label: "GitHub Copilot device login",
                description: "Sign in with GitHub and use your Copilot subscription",
                method: AuthMethod::DeviceCode,
                requires_secret: false,
            },
            AuthMethodSpec {
                id: "copilot_token",
                label: "GitHub Copilot token",
                description: "Paste a Copilot API token obtained by another trusted client",
                method: AuthMethod::DeviceCode,
                requires_secret: true,
            },
        ],
        "gitlab_duo" => vec![
            AuthMethodSpec {
                id: "gitlab_oauth",
                label: "GitLab OAuth",
                description: "OAuth/PAT connection for GitLab Duo compatible accounts",
                method: AuthMethod::OAuth,
                requires_secret: false,
            },
            AuthMethodSpec {
                id: "gitlab_pat",
                label: "GitLab personal access token",
                description: "Paste a GitLab PAT with the required AI scopes",
                method: AuthMethod::Pat,
                requires_secret: true,
            },
        ],
        "google_vertex" => vec![
            AuthMethodSpec {
                id: "google_adc",
                label: "Google ADC / gcloud",
                description: "Use Application Default Credentials from gcloud",
                method: AuthMethod::CredentialChain,
                requires_secret: false,
            },
            api_key_spec("Google Gemini API key"),
        ],
        "amazon_bedrock" => vec![AuthMethodSpec {
            id: "aws_profile",
            label: "AWS credential chain",
            description: "Use AWS_PROFILE or the default AWS SDK credential chain",
            method: AuthMethod::CredentialChain,
            requires_secret: false,
        }],
        "ollama" | "lmstudio" => vec![AuthMethodSpec {
            id: "local",
            label: "Local server",
            description: "No cloud account required",
            method: AuthMethod::Local,
            requires_secret: false,
        }],
        _ => vec![api_key_spec("API key")],
    }
}

pub fn method_by_id(provider: &str, method_id: &str) -> Option<AuthMethodSpec> {
    methods_for_provider(provider)
        .into_iter()
        .find(|method| method.id == method_id)
}

pub fn connect_blocker(provider: &str, method_id: &str) -> Option<&'static str> {
    let _ = (provider, method_id);
    None
}

pub fn record_from_secret(provider: &str, method_id: &str, secret: String) -> Result<AuthRecord> {
    let provider = registry::normalize_provider(provider).to_string();
    if let Some(message) = connect_blocker(&provider, method_id) {
        bail!("{message}");
    }
    let spec = method_by_id(&provider, method_id)
        .with_context(|| format!("Unknown auth method '{method_id}' for provider '{provider}'"))?;
    if spec.requires_secret && secret.trim().is_empty() {
        bail!("auth method '{}' requires a token or key", spec.id);
    }
    let record_provider = if provider == "openai" && method_id.starts_with("chatgpt") {
        "openai"
    } else {
        &provider
    };
    Ok(AuthRecord {
        provider: record_provider.to_string(),
        method: spec.method,
        access_token: if secret.trim().is_empty() {
            None
        } else {
            Some(secret.trim().to_string())
        },
        refresh_token: None,
        expires_at: None,
        account_id: None,
        base_url: if method_id.starts_with("chatgpt") {
            Some("https://chatgpt.com/backend-api/codex/responses".to_string())
        } else {
            None
        },
        extra: BTreeMap::new(),
    })
}

pub async fn connect_github_copilot_device() -> Result<AuthRecord> {
    #[derive(Deserialize)]
    struct DeviceCodeResponse {
        device_code: String,
        user_code: String,
        verification_uri: String,
        #[serde(default = "default_interval")]
        interval: u64,
        #[serde(default)]
        expires_in: u64,
    }

    #[derive(Deserialize)]
    struct AccessTokenResponse {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }

    #[derive(Deserialize)]
    struct CopilotTokenResponse {
        token: String,
        #[serde(default)]
        expires_at: Option<i64>,
    }

    let client = reqwest::Client::new();
    let device: DeviceCodeResponse = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[
            ("client_id", "Ov23li8tweQw6odWQebz"),
            ("scope", "read:user"),
        ])
        .send()
        .await
        .context("GitHub device-code request failed")?
        .error_for_status()
        .context("GitHub rejected the device-code request")?
        .json()
        .await
        .context("GitHub device-code response was invalid")?;

    eprintln!(
        "Open {} and enter code {}",
        device.verification_uri, device.user_code
    );

    let poll_deadline = std::time::Instant::now() + Duration::from_secs(device.expires_in.max(60));
    let mut interval = Duration::from_secs(device.interval.max(5));
    let github_access_token = loop {
        if std::time::Instant::now() >= poll_deadline {
            bail!("GitHub device login expired before authorization completed");
        }

        tokio::time::sleep(interval).await;
        let response: AccessTokenResponse = client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&[
                ("client_id", "Ov23li8tweQw6odWQebz"),
                ("device_code", device.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .context("GitHub token polling failed")?
            .error_for_status()
            .context("GitHub rejected the token polling request")?
            .json()
            .await
            .context("GitHub token polling response was invalid")?;

        if let Some(token) = response.access_token {
            break token;
        }

        match response.error.as_deref() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += Duration::from_secs(5),
            Some(other) => bail!(
                "GitHub device login failed: {}",
                response
                    .error_description
                    .unwrap_or_else(|| other.to_string())
            ),
            None => bail!("GitHub token polling returned no access token"),
        }
    };

    let copilot: CopilotTokenResponse = client
        .get("https://api.github.com/copilot_internal/v2/token")
        .header("Authorization", format!("Bearer {github_access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "cortex")
        .send()
        .await
        .context("GitHub Copilot token request failed")?
        .error_for_status()
        .context("GitHub rejected the Copilot token request")?
        .json()
        .await
        .context("GitHub Copilot token response was invalid")?;

    let mut extra = BTreeMap::new();
    extra.insert("github_access_token".to_string(), github_access_token);

    Ok(AuthRecord {
        provider: "github_copilot".to_string(),
        method: AuthMethod::DeviceCode,
        access_token: Some(copilot.token),
        refresh_token: None,
        expires_at: copilot
            .expires_at
            .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0)),
        account_id: None,
        base_url: Some("https://api.githubcopilot.com".to_string()),
        extra,
    })
}

pub async fn refresh_github_copilot(record: &AuthRecord) -> Result<AuthRecord> {
    #[derive(Deserialize)]
    struct CopilotTokenResponse {
        token: String,
        #[serde(default)]
        expires_at: Option<i64>,
    }

    let github_access_token = record
        .extra
        .get("github_access_token")
        .context("GitHub Copilot auth record cannot refresh because the GitHub token is missing")?;
    let copilot: CopilotTokenResponse = reqwest::Client::new()
        .get("https://api.github.com/copilot_internal/v2/token")
        .header("Authorization", format!("Bearer {github_access_token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "cortex")
        .send()
        .await
        .context("GitHub Copilot token refresh failed")?
        .error_for_status()
        .context("GitHub rejected the Copilot token refresh")?
        .json()
        .await
        .context("GitHub Copilot token refresh response was invalid")?;

    let mut refreshed = record.clone();
    refreshed.access_token = Some(copilot.token);
    refreshed.expires_at = copilot
        .expires_at
        .and_then(|ts| DateTime::<Utc>::from_timestamp(ts, 0));
    Ok(refreshed)
}

fn api_key_spec(label: &'static str) -> AuthMethodSpec {
    AuthMethodSpec {
        id: "api_key",
        label,
        description: "Paste a provider API key",
        method: AuthMethod::ApiKey,
        requires_secret: true,
    }
}

fn default_interval() -> u64 {
    5
}

fn restrict_permissions(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to restrict permissions: {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn methods_include_direct_auth_where_supported() {
        assert!(
            methods_for_provider("openai")
                .iter()
                .any(|method| method.id == "chatgpt_browser")
        );
        assert!(
            methods_for_provider("github-copilot")
                .iter()
                .any(|method| method.id == "github_device")
        );
        assert!(
            methods_for_provider("anthropic")
                .iter()
                .all(|method| method.id == "api_key")
        );
    }

    #[test]
    fn chatgpt_subscription_auth_can_be_recorded() {
        assert!(connect_blocker("openai", "chatgpt_browser").is_none());
        assert!(record_from_secret("openai", "chatgpt_token", "token".to_string()).is_ok());
    }

    #[test]
    fn expired_records_do_not_return_tokens() {
        let record = AuthRecord {
            provider: "openai".to_string(),
            method: AuthMethod::OAuth,
            access_token: Some("token".to_string()),
            refresh_token: None,
            expires_at: Some(Utc::now() - chrono::Duration::seconds(1)),
            account_id: None,
            base_url: None,
            extra: BTreeMap::new(),
        };
        assert_eq!(record.token(), None);
    }
}
