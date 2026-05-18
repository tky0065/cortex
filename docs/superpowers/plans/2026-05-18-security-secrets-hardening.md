# Security Secrets Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add central secret redaction and apply it to Cortex logs, manifests, email previews/errors, web-search context, and the security backlog.

**Architecture:** Add a focused `src/secrets.rs` module that collects configured and environment secrets, then redacts exact secret values and conservative secret-like output patterns. Integrate it only at display/persistence boundaries so agent/provider behavior remains unchanged. Document the threat model and update `LACUNES.md` after verification.

**Tech Stack:** Rust, Tokio tests, `anyhow`, `serde_json`, Markdown docs, existing `cargo fmt`, `cargo test`, and `cargo check` workflow.

---

## File Structure

- Create `src/secrets.rs`: central redaction module and unit tests.
- Modify `src/main.rs`: expose `mod secrets;`.
- Modify `src/orchestrator.rs`: redact verbose log lines and manifest prompt output; add helper tests.
- Modify `src/tools/email.rs`: redact dry-run preview and sanitize returned SMTP errors.
- Modify `src/tools/web_search.rs`: redact search context formatting and add deterministic formatting tests.
- Modify `src/tools/filesystem.rs`: add symlink escape test if current implementation permits escape through symlinks.
- Modify `src/tools/terminal.rs`: add adversarial command-name rejection coverage.
- Create `docs/SECURITY_THREAT_MODEL.md`: threat model and remaining gaps.
- Modify `LACUNES.md`: mark lacunes 2, 20, and 22 accurately and add lot tracking.

## Task 1: Add Central Secret Redactor

**Files:**
- Create: `src/secrets.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Expose the module**

Add this line to [src/main.rs](/Users/yacoubakone/Documents/dev/cortex/src/main.rs:1) with the other top-level modules:

```rust
mod secrets;
```

- [ ] **Step 2: Create failing redactor tests**

Create [src/secrets.rs](/Users/yacoubakone/Documents/dev/cortex/src/secrets.rs) with the tests first:

```rust
use crate::config::Config;

const REDACTED: &str = "[REDACTED]";
const MIN_SECRET_LEN: usize = 8;

#[derive(Debug, Clone, Default)]
pub struct SecretRedactor {
    secrets: Vec<String>,
}

impl SecretRedactor {
    pub fn from_config_and_env(_config: &Config) -> Self {
        Self::default()
    }

    pub fn from_values<I, S>(_values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::default()
    }

    pub fn redact_text(&self, input: &str) -> String {
        input.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_exact_configured_values() {
        let redactor = SecretRedactor::from_values(["sk-test-1234567890"]);
        let output = redactor.redact_text("token sk-test-1234567890 used");

        assert_eq!(output, "token [REDACTED] used");
        assert!(!output.contains("sk-test-1234567890"));
    }

    #[test]
    fn ignores_short_values_to_avoid_false_positives() {
        let redactor = SecretRedactor::from_values(["dev"]);
        assert_eq!(redactor.redact_text("dev mode"), "dev mode");
    }

    #[test]
    fn deduplicates_values_and_keeps_unrelated_text() {
        let redactor = SecretRedactor::from_values([
            "secret-value-123",
            "secret-value-123",
            "another-secret-456",
        ]);
        let output = redactor.redact_text("prefix secret-value-123 middle another-secret-456 suffix");

        assert_eq!(output, "prefix [REDACTED] middle [REDACTED] suffix");
    }

    #[test]
    fn redacts_bearer_tokens() {
        let redactor = SecretRedactor::default();
        let output = redactor.redact_text("Authorization: Bearer abcdefghijklmnopqrstuvwxyz123456");

        assert_eq!(output, "Authorization: Bearer [REDACTED]");
    }

    #[test]
    fn redacts_assignment_patterns() {
        let redactor = SecretRedactor::default();
        let output = redactor.redact_text(
            "api_key=sk-abcdef123456 password=\"super-secret-value\" token: ghp_abcdef1234567890",
        );

        assert!(!output.contains("sk-abcdef123456"));
        assert!(!output.contains("super-secret-value"));
        assert!(!output.contains("ghp_abcdef1234567890"));
        assert!(output.contains("api_key=[REDACTED]"));
        assert!(output.contains("password=[REDACTED]"));
        assert!(output.contains("token: [REDACTED]"));
    }

    #[test]
    fn redacts_private_key_blocks() {
        let redactor = SecretRedactor::default();
        let input = "before\n-----BEGIN PRIVATE KEY-----\nabcdef123456\n-----END PRIVATE KEY-----\nafter";
        let output = redactor.redact_text(input);

        assert_eq!(output, "before\n[REDACTED]\nafter");
    }
}
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
cargo test secrets::tests
```

Expected: several tests fail because the initial implementation returns input unchanged.

- [ ] **Step 4: Implement the redactor**

Replace [src/secrets.rs](/Users/yacoubakone/Documents/dev/cortex/src/secrets.rs) with:

```rust
use crate::config::Config;

const REDACTED: &str = "[REDACTED]";
const MIN_SECRET_LEN: usize = 8;

const ENV_SECRET_VARS: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "MISTRAL_API_KEY",
    "DEEPSEEK_API_KEY",
    "XAI_API_KEY",
    "COHERE_API_KEY",
    "PERPLEXITY_API_KEY",
    "HUGGINGFACE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "GROQ_API_KEY",
    "TOGETHER_API_KEY",
    "WEB_SEARCH_API_KEY",
    "SMTP_PASS",
];

#[derive(Debug, Clone, Default)]
pub struct SecretRedactor {
    secrets: Vec<String>,
}

impl SecretRedactor {
    pub fn from_config_and_env(config: &Config) -> Self {
        let mut values = Vec::new();

        let api_keys = &config.api_keys;
        values.extend([
            api_keys.openai.as_deref(),
            api_keys.anthropic.as_deref(),
            api_keys.gemini.as_deref(),
            api_keys.mistral.as_deref(),
            api_keys.deepseek.as_deref(),
            api_keys.xai.as_deref(),
            api_keys.cohere.as_deref(),
            api_keys.perplexity.as_deref(),
            api_keys.huggingface.as_deref(),
            api_keys.azure_openai.as_deref(),
            api_keys.openrouter.as_deref(),
            api_keys.groq.as_deref(),
            api_keys.together.as_deref(),
            api_keys.web_search.as_deref(),
        ]);

        for custom in config.custom_providers.values() {
            values.push(custom.api_key.as_deref());
        }

        let mut env_values = Vec::new();
        for name in ENV_SECRET_VARS {
            if let Ok(value) = std::env::var(name) {
                env_values.push(value);
            }
        }

        let mut redactor = Self::from_values(values.into_iter().flatten());
        redactor.add_values(env_values);
        redactor
    }

    pub fn from_values<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut redactor = Self::default();
        redactor.add_values(values);
        redactor
    }

    pub fn redact_text(&self, input: &str) -> String {
        if input.is_empty() {
            return String::new();
        }

        let mut out = input.to_string();
        for secret in &self.secrets {
            out = out.replace(secret, REDACTED);
        }
        out = redact_private_key_blocks(&out);
        out = redact_bearer_tokens(&out);
        redact_assignments(&out)
    }

    fn add_values<I, S>(&mut self, values: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for value in values {
            let value = value.into();
            let trimmed = value.trim();
            if trimmed.len() < MIN_SECRET_LEN {
                continue;
            }
            if !self.secrets.iter().any(|existing| existing == trimmed) {
                self.secrets.push(trimmed.to_string());
            }
        }
    }
}

fn redact_private_key_blocks(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    while let Some(begin) = rest.find("-----BEGIN ") {
        out.push_str(&rest[..begin]);
        let after_begin = &rest[begin..];
        if let Some(end_rel) = after_begin.find("-----END ") {
            let after_end_marker = &after_begin[end_rel..];
            let after = if let Some(newline_rel) = after_end_marker.find('\n') {
                end_rel + newline_rel
            } else {
                after_begin.len()
            };
            out.push_str(REDACTED);
            rest = &after_begin[after..];
            continue;
        }
        out.push_str(&rest[begin..]);
        return out;
    }

    out.push_str(rest);
    out
}

fn redact_bearer_tokens(input: &str) -> String {
    redact_after_prefix(input, "Bearer ")
}

fn redact_assignments(input: &str) -> String {
    let mut out = input.to_string();
    for key in ["api_key", "apikey", "token", "password", "secret"] {
        out = redact_assignment_key(&out, key);
    }
    out
}

fn redact_assignment_key(input: &str, key: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    while let Some(idx) = rest.to_ascii_lowercase().find(key) {
        out.push_str(&rest[..idx]);
        let matched = &rest[idx..idx + key.len()];
        let after_key = &rest[idx + key.len()..];
        let Some((separator, after_separator)) = parse_secret_separator(after_key) else {
            out.push_str(matched);
            rest = after_key;
            continue;
        };

        let (token, after_token) = take_secret_token(after_separator);
        out.push_str(matched);
        out.push_str(separator);
        if token.len() >= MIN_SECRET_LEN {
            out.push_str(REDACTED);
        } else {
            out.push_str(token);
        }
        rest = after_token;
    }

    out.push_str(rest);
    out
}

fn parse_secret_separator(input: &str) -> Option<(&str, &str)> {
    for sep in [" = ", "=", ": ", ":"] {
        if let Some(rest) = input.strip_prefix(sep) {
            return Some((sep, rest));
        }
    }
    None
}

fn take_secret_token(input: &str) -> (&str, &str) {
    let input = input.trim_start();
    if let Some(stripped) = input.strip_prefix('"')
        && let Some(end) = stripped.find('"')
    {
        return (&stripped[..end], &stripped[end + 1..]);
    }
    if let Some(stripped) = input.strip_prefix('\'')
        && let Some(end) = stripped.find('\'')
    {
        return (&stripped[..end], &stripped[end + 1..]);
    }

    let end = input
        .char_indices()
        .find_map(|(idx, ch)| {
            if ch.is_whitespace() || matches!(ch, ',' | ';') {
                Some(idx)
            } else {
                None
            }
        })
        .unwrap_or(input.len());
    (&input[..end], &input[end..])
}

fn redact_after_prefix(input: &str, prefix: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    while let Some(idx) = rest.find(prefix) {
        out.push_str(&rest[..idx + prefix.len()]);
        let after_prefix = &rest[idx + prefix.len()..];
        let (token, after_token) = take_secret_token(after_prefix);
        if token.len() >= MIN_SECRET_LEN {
            out.push_str(REDACTED);
        } else {
            out.push_str(token);
        }
        rest = after_token;
    }

    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_exact_configured_values() {
        let redactor = SecretRedactor::from_values(["sk-test-1234567890"]);
        let output = redactor.redact_text("token sk-test-1234567890 used");

        assert_eq!(output, "token [REDACTED] used");
        assert!(!output.contains("sk-test-1234567890"));
    }

    #[test]
    fn ignores_short_values_to_avoid_false_positives() {
        let redactor = SecretRedactor::from_values(["dev"]);
        assert_eq!(redactor.redact_text("dev mode"), "dev mode");
    }

    #[test]
    fn deduplicates_values_and_keeps_unrelated_text() {
        let redactor = SecretRedactor::from_values([
            "secret-value-123",
            "secret-value-123",
            "another-secret-456",
        ]);
        let output = redactor.redact_text("prefix secret-value-123 middle another-secret-456 suffix");

        assert_eq!(output, "prefix [REDACTED] middle [REDACTED] suffix");
    }

    #[test]
    fn redacts_bearer_tokens() {
        let redactor = SecretRedactor::default();
        let output = redactor.redact_text("Authorization: Bearer abcdefghijklmnopqrstuvwxyz123456");

        assert_eq!(output, "Authorization: Bearer [REDACTED]");
    }

    #[test]
    fn redacts_assignment_patterns() {
        let redactor = SecretRedactor::default();
        let output = redactor.redact_text(
            "api_key=sk-abcdef123456 password=\"super-secret-value\" token: ghp_abcdef1234567890",
        );

        assert!(!output.contains("sk-abcdef123456"));
        assert!(!output.contains("super-secret-value"));
        assert!(!output.contains("ghp_abcdef1234567890"));
        assert!(output.contains("api_key=[REDACTED]"));
        assert!(output.contains("password=[REDACTED]"));
        assert!(output.contains("token: [REDACTED]"));
    }

    #[test]
    fn redacts_private_key_blocks() {
        let redactor = SecretRedactor::default();
        let input = "before\n-----BEGIN PRIVATE KEY-----\nabcdef123456\n-----END PRIVATE KEY-----\nafter";
        let output = redactor.redact_text(input);

        assert_eq!(output, "before\n[REDACTED]\nafter");
    }
}
```

- [ ] **Step 5: Run redactor tests**

Run:

```bash
cargo test secrets::tests
```

Expected: all `secrets::tests` pass.

- [ ] **Step 6: Commit the redactor**

Run:

```bash
git add src/main.rs src/secrets.rs
git commit -m "feat: add secret redactor"
```

Expected: one commit containing only module exposure and `src/secrets.rs`.

## Task 2: Redact Verbose Logs And Run Manifest

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write failing manifest redaction test**

In [src/orchestrator.rs](/Users/yacoubakone/Documents/dev/cortex/src/orchestrator.rs:396), update the test module imports:

```rust
use super::{default_project_dir, write_manifest};
use crate::config::Config;
use crate::tui::events::{TuiEvent, channel};
```

Add this test inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn manifest_redacts_prompt_secrets() {
    let dir = std::env::temp_dir().join(format!(
        "cortex_manifest_redact_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let mut config = Config::default();
    config.api_keys.openai = Some("sk-test-manifest-secret".to_string());

    write_manifest(
        &dir,
        "dev",
        "build a tool with key sk-test-manifest-secret",
        &config,
    );

    let content = std::fs::read_to_string(dir.join("cortex.manifest.json")).unwrap();
    assert!(content.contains("[REDACTED]"));
    assert!(!content.contains("sk-test-manifest-secret"));

    let _ = std::fs::remove_dir_all(dir);
}
```

- [ ] **Step 2: Run test to verify failure**

Run:

```bash
cargo test orchestrator::tests::manifest_redacts_prompt_secrets
```

Expected: FAIL because `write_manifest()` currently stores the prompt unchanged.

- [ ] **Step 3: Redact manifest prompt**

In `write_manifest()` in [src/orchestrator.rs](/Users/yacoubakone/Documents/dev/cortex/src/orchestrator.rs:275), add:

```rust
let redactor = crate::secrets::SecretRedactor::from_config_and_env(config);
let redacted_prompt = redactor.redact_text(prompt);
```

Then change the manifest field:

```rust
"prompt": redacted_prompt,
```

- [ ] **Step 4: Run manifest test**

Run:

```bash
cargo test orchestrator::tests::manifest_redacts_prompt_secrets
```

Expected: PASS.

- [ ] **Step 5: Write failing verbose log helper test**

Add this helper near `write_manifest()`:

```rust
fn format_verbose_log_line(
    agent: &str,
    chunk: &str,
    redactor: &crate::secrets::SecretRedactor,
) -> String {
    format!("[{}] {}", agent, redactor.redact_text(chunk))
}
```

Add this test inside the existing orchestrator test module:

```rust
#[test]
fn verbose_log_line_redacts_secrets() {
    let redactor = crate::secrets::SecretRedactor::from_values(["log-secret-123456"]);
    let line = super::format_verbose_log_line(
        "developer",
        "received log-secret-123456",
        &redactor,
    );

    assert_eq!(line, "[developer] received [REDACTED]");
    assert!(!line.contains("log-secret-123456"));
}
```

- [ ] **Step 6: Wire helper into verbose logger**

In the verbose logging task in [src/orchestrator.rs](/Users/yacoubakone/Documents/dev/cortex/src/orchestrator.rs:150), create a redactor before `tokio::spawn`:

```rust
let log_redactor = crate::secrets::SecretRedactor::from_config_and_env(&self.config);
```

Move `log_redactor` into the spawned task, and replace:

```rust
let _ = writeln!(f, "[{}] {}", agent, chunk);
```

with:

```rust
let _ = writeln!(f, "{}", format_verbose_log_line(agent, chunk, &log_redactor));
```

- [ ] **Step 7: Run orchestrator redaction tests**

Run:

```bash
cargo test orchestrator::tests::
```

Expected: both tests pass.

- [ ] **Step 8: Commit log and manifest integration**

Run:

```bash
git add src/orchestrator.rs
git commit -m "feat: redact secrets in run artifacts"
```

Expected: one commit modifying `src/orchestrator.rs`.

## Task 3: Redact Email Tool Output

**Files:**
- Modify: `src/tools/email.rs`

- [ ] **Step 1: Write failing dry-run test**

Add this test in [src/tools/email.rs](/Users/yacoubakone/Documents/dev/cortex/src/tools/email.rs:77):

```rust
#[tokio::test]
async fn dry_run_redacts_secret_like_body() {
    let msg = EmailMessage {
        to: "test@example.com".into(),
        subject: "Hello".into(),
        body: "password=super-secret-value".into(),
    };

    let result = send(&msg, SendMode::DryRun).await.unwrap();

    assert!(result.contains("password=[REDACTED]"));
    assert!(!result.contains("super-secret-value"));
}
```

- [ ] **Step 2: Run test to verify failure**

Run:

```bash
cargo test tools::email::tests::dry_run_redacts_secret_like_body
```

Expected: FAIL because dry-run currently returns the body unchanged.

- [ ] **Step 3: Redact dry-run preview**

Change the `SendMode::DryRun` branch in [src/tools/email.rs](/Users/yacoubakone/Documents/dev/cortex/src/tools/email.rs:23) to:

```rust
SendMode::DryRun => {
    let redactor = crate::secrets::SecretRedactor::from_config_and_env(&crate::config::Config::default());
    let preview = format!(
        "[DRY-RUN] Would send email:\n  To:      {}\n  Subject: {}\n  Body:\n{}",
        msg.to, msg.subject, msg.body
    );
    Ok(redactor.redact_text(&preview))
}
```

- [ ] **Step 4: Run dry-run test**

Run:

```bash
cargo test tools::email::tests::dry_run_redacts_secret_like_body
```

Expected: PASS.

- [ ] **Step 5: Add SMTP env-secret error test**

Add this test:

```rust
#[tokio::test]
async fn live_send_error_does_not_expose_smtp_pass() {
    unsafe {
        std::env::set_var("SMTP_HOST", "invalid.localhost");
        std::env::set_var("SMTP_USER", "sender@example.com");
        std::env::set_var("SMTP_PASS", "smtp-secret-123456");
    }

    let msg = EmailMessage {
        to: "test@example.com".into(),
        subject: "Hello".into(),
        body: "World".into(),
    };

    let err = send(&msg, SendMode::Send).await.unwrap_err().to_string();
    assert!(!err.contains("smtp-secret-123456"));

    unsafe {
        std::env::remove_var("SMTP_HOST");
        std::env::remove_var("SMTP_USER");
        std::env::remove_var("SMTP_PASS");
    }
}
```

- [ ] **Step 6: Run email tests**

Run:

```bash
cargo test tools::email::tests
```

Expected: all email tests pass.

- [ ] **Step 7: Commit email redaction**

Run:

```bash
git add src/tools/email.rs
git commit -m "feat: redact email tool output"
```

Expected: one commit modifying `src/tools/email.rs`.

## Task 4: Redact Web Search Context

**Files:**
- Modify: `src/tools/web_search.rs`

- [ ] **Step 1: Add formatting helper tests**

Add these helper tests in [src/tools/web_search.rs](/Users/yacoubakone/Documents/dev/cortex/src/tools/web_search.rs:242):

```rust
#[test]
fn formats_context_with_redacted_query_and_results() {
    let redactor = crate::secrets::SecretRedactor::from_values(["web-secret-123456"]);
    let results = vec![SearchResult {
        title: "title web-secret-123456".into(),
        url: "https://example.com/?token=web-secret-123456".into(),
        snippet: "snippet web-secret-123456".into(),
    }];

    let block = format_results_block(
        "Web Search Results",
        "query web-secret-123456",
        &results,
        &redactor,
    );

    assert!(block.contains("[REDACTED]"));
    assert!(!block.contains("web-secret-123456"));
}

#[test]
fn offline_stub_redacts_query() {
    let redactor = crate::secrets::SecretRedactor::from_values(["offline-secret-123456"]);
    let result = offline_stub_result("find offline-secret-123456", &redactor);

    assert!(result.snippet.contains("[REDACTED]"));
    assert!(!result.snippet.contains("offline-secret-123456"));
}
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test tools::web_search::tests::
```

Expected: FAIL because `format_results_block()` and `offline_stub_result()` do not exist.

- [ ] **Step 3: Add redacted formatting helpers**

Add these helpers near the `SearchResult` struct:

```rust
fn offline_stub_result(
    query: &str,
    redactor: &crate::secrets::SecretRedactor,
) -> SearchResult {
    let redacted_query = redactor.redact_text(query);
    SearchResult {
        title: format!("Search results for: {}", redacted_query),
        url: "https://example.com".into(),
        snippet: format!(
            "[offline mode] No WEB_SEARCH_API_KEY set. Query was: {}",
            redacted_query
        ),
    }
}

fn format_results_block(
    title: &str,
    query: &str,
    results: &[SearchResult],
    redactor: &crate::secrets::SecretRedactor,
) -> String {
    let mut block = format!(
        "\n\n## {}\nQuery: {}\n\n",
        title,
        redactor.redact_text(query)
    );
    for (i, result) in results.iter().enumerate() {
        block.push_str(&format!(
            "{}. **{}** ({})\n   {}\n",
            i + 1,
            redactor.redact_text(&result.title),
            redactor.redact_text(&result.url),
            redactor.redact_text(&result.snippet)
        ));
    }
    block
}
```

- [ ] **Step 4: Wire helpers into search context**

In `search()` replace the offline stub construction with:

```rust
let redactor = crate::secrets::SecretRedactor::from_values([api_key.clone()]);
return Ok(vec![offline_stub_result(query, &redactor)]);
```

In `search_without_key()`, create:

```rust
let redactor = crate::secrets::SecretRedactor::default();
```

Then replace manual block formatting with:

```rust
format_results_block("Web Search Results (DuckDuckGo Lite)", query, &results[..results.len().min(5)], &redactor)
```

In `fetch_context()`, create:

```rust
let redactor = crate::secrets::SecretRedactor::from_config_and_env(config);
```

Use it when formatting API-backed results:

```rust
format_results_block("Web Search Results", trimmed, &results, &redactor)
```

- [ ] **Step 5: Run web-search tests**

Run:

```bash
cargo test tools::web_search::tests
```

Expected: all web-search tests pass without network access.

- [ ] **Step 6: Commit web-search redaction**

Run:

```bash
git add src/tools/web_search.rs
git commit -m "feat: redact web search context"
```

Expected: one commit modifying `src/tools/web_search.rs`.

## Task 5: Add Adversarial Tool Tests

**Files:**
- Modify: `src/tools/filesystem.rs`
- Modify: `src/tools/terminal.rs`

- [ ] **Step 1: Add filesystem symlink escape test**

Add this Unix-only test in [src/tools/filesystem.rs](/Users/yacoubakone/Documents/dev/cortex/src/tools/filesystem.rs:62):

```rust
#[cfg(unix)]
#[test]
fn rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!("cortex_fs_symlink_root_{}", std::process::id()));
    let outside = std::env::temp_dir().join(format!("cortex_fs_symlink_outside_{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    symlink(&outside, root.join("escape")).unwrap();

    let sandbox = FileSystem::new(&root);
    assert!(sandbox.read("escape/secret.txt").is_err());

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside);
}
```

- [ ] **Step 2: Run test to verify current behavior**

Run:

```bash
cargo test tools::filesystem::tests::rejects_symlink_escape
```

Expected: FAIL if symlink escape is currently possible. If it passes because implementation already rejects symlinks after canonicalization, keep the test and continue.

- [ ] **Step 3: Harden filesystem resolution if needed**

If the symlink test failed, update `resolve()` in [src/tools/filesystem.rs](/Users/yacoubakone/Documents/dev/cortex/src/tools/filesystem.rs:35) after computing `abs`:

```rust
if abs.exists() {
    let canonical_root = self
        .root
        .canonicalize()
        .with_context(|| format!("canonicalize sandbox root failed: {}", self.root.display()))?;
    let canonical_abs = abs
        .canonicalize()
        .with_context(|| format!("canonicalize path failed: {}", abs.display()))?;
    if !canonical_abs.starts_with(&canonical_root) {
        bail!("path escapes sandbox: {}", canonical_abs.display());
    }
}
```

Keep the existing normalized containment check for new files that do not exist yet.

- [ ] **Step 4: Add terminal disguised shell rejection test**

Add this test in [src/tools/terminal.rs](/Users/yacoubakone/Documents/dev/cortex/src/tools/terminal.rs:55):

```rust
#[tokio::test]
async fn rejects_shell_like_command_names() {
    assert!(run("cargo;sh", &["--version"], None, None).await.is_err());
    assert!(run("git&&sh", &["--version"], None, None).await.is_err());
    assert!(run("/bin/sh", &["-c", "echo hi"], None, None).await.is_err());
}
```

- [ ] **Step 5: Run tool tests**

Run:

```bash
cargo test tools::
```

Expected: all filesystem and terminal tests pass.

- [ ] **Step 6: Commit adversarial tool tests**

Run:

```bash
git add src/tools/filesystem.rs src/tools/terminal.rs
git commit -m "test: add adversarial tool coverage"
```

Expected: one commit containing only tool hardening tests and any required filesystem containment fix.

## Task 6: Add Threat Model Documentation And Update Lacunes

**Files:**
- Create: `docs/SECURITY_THREAT_MODEL.md`
- Modify: `LACUNES.md`

- [ ] **Step 1: Create threat model document**

Create [docs/SECURITY_THREAT_MODEL.md](/Users/yacoubakone/Documents/dev/cortex/docs/SECURITY_THREAT_MODEL.md):

```markdown
# Cortex Security Threat Model

This document tracks the beta security model for Cortex. It focuses on the surfaces where untrusted text, model output, local files, tools, providers, and credentials meet.

## Protected Assets

- User source trees and generated project files.
- `~/.cortex/config.toml` provider configuration.
- API keys, OAuth tokens, PATs, SMTP credentials, and provider tokens.
- `cortex.log` verbose logs.
- `cortex.manifest.json` run metadata.
- Email previews and live-send errors.
- Web-search results injected into prompts.

## Trust Boundaries

| Boundary | Risk | Current Control |
|----------|------|-----------------|
| User prompt to model provider | User may include private content intentionally or accidentally | Privacy docs explain provider exposure; this lot does not alter outbound prompts |
| Model output to terminal tool | Model may request unsafe commands | Hardcoded command allowlist in `src/tools/terminal.rs` |
| Model output to filesystem tool | Model may request path traversal or sandbox escape | Relative path validation and containment checks in `src/tools/filesystem.rs` |
| Web search result to agent prompt | Search result may contain prompt injection or reflected secrets | Web-search context is redacted before injection; full prompt-injection defense remains open |
| Email tool output to user | Email body or SMTP errors may contain secrets | Dry-run previews and SMTP errors are redacted |
| Run artifacts to disk | Logs and manifests may persist tokens from prompts or agent output | `cortex.log` and manifest prompt fields are redacted |
| Custom agents and workflows | Custom definitions may request unsafe tools or malformed execution | Full validation remains tracked by lacune 8 |
| Updater | Release/update path may be compromised | Release process exists; stronger updater verification remains future work |

## Adversaries And Abuse Cases

- Malicious web content that instructs an agent to reveal local secrets.
- Malicious or careless prompt content containing API keys or SMTP credentials.
- Model output that tries to execute shell commands outside the allowlist.
- Model output that tries to read files outside the filesystem sandbox.
- Custom workflow definitions that request unsafe behavior.
- Provider or SMTP errors that include request metadata.

## Controls Added In This Lot

- Central `SecretRedactor` for configured API keys, selected environment secrets, bearer tokens, private key blocks, and common assignment patterns.
- Redaction for verbose logs written to `cortex.log`.
- Redaction for the prompt stored in `cortex.manifest.json`.
- Redaction for email dry-run previews and returned SMTP errors.
- Redaction for web-search context blocks before prompt injection.
- Adversarial tests for redaction and selected tool boundaries.

## Remaining Gaps

- Lacune 2 remains in progress until tool permissions, updater integrity, custom workflow boundaries, and web-search prompt injection have broader coverage.
- Lacune 8 remains open for strict custom workflow and custom agent validation.
- Lacune 20 remains in progress until adversarial tests cover composed attacks across web search, custom agents, terminal, filesystem, email, and resume.
- Redaction is best-effort. It reduces accidental leakage in Cortex-owned output surfaces, but it does not prevent users from sending secrets to configured model providers.
```

- [ ] **Step 2: Update lacune 2**

In [LACUNES.md](/Users/yacoubakone/Documents/dev/cortex/LACUNES.md:31), change lacune 2 status/proof to:

```markdown
**Statut:** En cours
**Preuve:** Modèle de menace ajouté dans `docs/SECURITY_THREAT_MODEL.md`; premières protections runtime prévues/couvertes par le lot sécurité/secrets (redaction logs, manifests, email, web search). Reste à couvrir updater, validation custom workflows et prompt injection web avancée.
```

- [ ] **Step 3: Update lacune 20**

In [LACUNES.md](/Users/yacoubakone/Documents/dev/cortex/LACUNES.md:173), change lacune 20 status/proof to:

```markdown
**Statut:** En cours
**Preuve:** Premiers tests adversariaux ajoutés pour redaction de secrets et frontières tools (`src/secrets.rs`, `src/tools/filesystem.rs`, `src/tools/terminal.rs`, `src/tools/email.rs`, `src/tools/web_search.rs`). Les attaques composées restent à couvrir.
```

- [ ] **Step 4: Update lacune 22**

In [LACUNES.md](/Users/yacoubakone/Documents/dev/cortex/LACUNES.md:189), change lacune 22 status/proof to:

```markdown
**Statut:** Terminé
**Preuve:** Redaction centrale dans `src/secrets.rs`, appliquée aux artefacts de run (`cortex.log`, `cortex.manifest.json`), aux previews email et au contexte web search, avec tests de non-régression.
```

- [ ] **Step 5: Add lot tracking entry**

Append this line under `## Suivi des lots`:

```markdown
- 2026-05-18 — Lot sécurité/secrets terminé: modèle de menace, redaction centrale, logs/manifests/email/web search redacted, premiers tests adversariaux. Lacunes terminées: 22. Lacunes partiellement traitées: 2, 20.
```

- [ ] **Step 6: Check documentation**

Run:

```bash
sed -n '1,220p' docs/SECURITY_THREAT_MODEL.md
rg "Statut: En cours|Statut: Terminé|Lot sécurité/secrets" LACUNES.md
```

Expected: threat model renders cleanly, and lacunes 2/20/22 plus lot tracking are visible.

- [ ] **Step 7: Commit docs and lacunes**

Run:

```bash
git add docs/SECURITY_THREAT_MODEL.md LACUNES.md
git commit -m "docs: add security threat model"
```

Expected: one commit containing only threat model and lacune tracking changes.

## Task 7: Final Verification

**Files:**
- All files changed by previous tasks.

- [ ] **Step 1: Format code**

Run:

```bash
cargo fmt
```

Expected: command exits 0.

- [ ] **Step 2: Run full tests**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Run type check**

Run:

```bash
cargo check
```

Expected: command exits 0 with no errors.

- [ ] **Step 4: Inspect changed files**

Run:

```bash
git status --short
git diff --check
git diff --stat HEAD
```

Expected: no whitespace errors; only expected files are modified if final formatting changed files after their task commits.

- [ ] **Step 5: Commit formatting leftovers if any**

If `cargo fmt` changed files after earlier commits, run:

```bash
git add src/main.rs src/secrets.rs src/orchestrator.rs src/tools/email.rs src/tools/web_search.rs src/tools/filesystem.rs src/tools/terminal.rs
git commit -m "style: format security hardening changes"
```

Expected: either a small formatting commit is created or there is nothing to commit.

- [ ] **Step 6: Final status**

Run:

```bash
git status --short
```

Expected: no tracked files remain modified. Existing unrelated untracked local files may remain.

## Self-Review

- Spec coverage: the plan covers central redaction, logs, manifest, email, web search, adversarial tests, threat model, and `LACUNES.md`.
- Scope check: custom workflow validation, updater integrity, OS sandboxing, and full prompt-injection defense remain outside this lot as specified.
- Type consistency: all new code uses `SecretRedactor::from_config_and_env`, `SecretRedactor::from_values`, and `redact_text` consistently.
- Verification: the final task runs `cargo fmt`, `cargo test`, and `cargo check`.
