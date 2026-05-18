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
        let api_keys = &config.api_keys;
        let configured_values = [
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
        ];

        let mut redactor = Self::from_values(configured_values.into_iter().flatten());
        redactor.add_values(
            config
                .custom_providers
                .values()
                .filter_map(|provider| provider.api_key.as_deref()),
        );

        redactor.add_values(
            ENV_SECRET_VARS
                .iter()
                .filter_map(|name| std::env::var(name).ok()),
        );
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

        let mut output = input.to_string();
        for secret in &self.secrets {
            output = output.replace(secret, REDACTED);
        }

        output = redact_private_key_blocks(&output);
        output = redact_bearer_tokens(&output);
        redact_assignments(&output)
    }

    fn add_values<I, S>(&mut self, values: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for value in values {
            let value = value.into();
            let value = value.trim();
            if value.len() < MIN_SECRET_LEN || self.secrets.iter().any(|secret| secret == value) {
                continue;
            }
            self.secrets.push(value.to_string());
        }
        self.secrets
            .sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    }
}

fn redact_private_key_blocks(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;

    while let Some(begin_idx) = rest.find("-----BEGIN ") {
        output.push_str(&rest[..begin_idx]);
        let block = &rest[begin_idx..];

        let Some(header_end_idx) = block.find('\n') else {
            output.push_str(block);
            return output;
        };
        let header = &block[..header_end_idx];
        if !header.contains("PRIVATE KEY") {
            output.push_str(&block[..header.len()]);
            rest = &block[header.len()..];
            continue;
        }

        let Some(end_idx) = block.find("-----END ") else {
            output.push_str(block);
            return output;
        };
        output.push_str(REDACTED);
        let end_block = &block[end_idx..];
        let after_marker_idx = if let Some(newline_idx) = end_block.find('\n') {
            end_idx + newline_idx
        } else {
            block.len()
        };
        rest = &block[after_marker_idx..];
    }

    output.push_str(rest);
    output
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;

    while let Some(idx) = find_ascii_case_insensitive(rest, "Bearer ") {
        output.push_str(&rest[..idx + "Bearer ".len()]);
        let after_prefix = &rest[idx + "Bearer ".len()..];
        let (token, after_token) = take_unquoted_token(after_prefix);

        if token.len() >= MIN_SECRET_LEN {
            output.push_str(REDACTED);
        } else {
            output.push_str(token);
        }
        rest = after_token;
    }

    output.push_str(rest);
    output
}

fn redact_assignments(input: &str) -> String {
    let mut output = input.to_string();
    for key in ["api_key", "apikey", "token", "password", "secret"] {
        output = redact_assignment_key(&output, key);
    }
    output
}

fn redact_assignment_key(input: &str, key: &str) -> String {
    let mut output = String::new();
    let mut rest = input;

    while let Some(idx) = find_ascii_case_insensitive(rest, key) {
        output.push_str(&rest[..idx]);
        let matched_key = &rest[idx..idx + key.len()];
        let after_key = &rest[idx + key.len()..];

        let Some((separator, after_separator)) = parse_assignment_separator(after_key) else {
            output.push_str(matched_key);
            rest = after_key;
            continue;
        };

        let (value, after_value) = take_assignment_value(after_separator);
        output.push_str(matched_key);
        output.push_str(separator);
        if value.len() >= MIN_SECRET_LEN {
            output.push_str(REDACTED);
        } else {
            output.push_str(value);
        }
        rest = after_value;
    }

    output.push_str(rest);
    output
}

fn parse_assignment_separator(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    let leading_ws_len = input.len() - trimmed.len();
    let separator = trimmed.chars().next()?;
    if separator != '=' && separator != ':' {
        return None;
    }

    let after_separator = &trimmed[separator.len_utf8()..];
    let after_value_ws = after_separator.trim_start();
    let consumed_len =
        leading_ws_len + separator.len_utf8() + after_separator.len() - after_value_ws.len();

    Some((&input[..consumed_len], after_value_ws))
}

fn take_assignment_value(input: &str) -> (&str, &str) {
    if let Some(stripped) = input.strip_prefix('"') {
        if let Some(end_idx) = stripped.find('"') {
            return (&stripped[..end_idx], &stripped[end_idx + 1..]);
        }
    }
    if let Some(stripped) = input.strip_prefix('\'') {
        if let Some(end_idx) = stripped.find('\'') {
            return (&stripped[..end_idx], &stripped[end_idx + 1..]);
        }
    }

    take_unquoted_token(input)
}

fn take_unquoted_token(input: &str) -> (&str, &str) {
    let end_idx = input
        .char_indices()
        .find_map(|(idx, ch)| {
            if ch.is_whitespace() || matches!(ch, ',' | ';') {
                Some(idx)
            } else {
                None
            }
        })
        .unwrap_or(input.len());

    (&input[..end_idx], &input[end_idx..])
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
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
        let output =
            redactor.redact_text("prefix secret-value-123 middle another-secret-456 suffix");

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
        let input =
            "before\n-----BEGIN PRIVATE KEY-----\nabcdef123456\n-----END PRIVATE KEY-----\nafter";
        let output = redactor.redact_text(input);

        assert_eq!(output, "before\n[REDACTED]\nafter");
    }

    #[test]
    fn from_config_and_env_reads_config_and_env_keys() {
        let mut config = Config::default();
        config.api_keys.openai = Some("sk-config-1234567890".to_string());
        let previous = std::env::var("GROQ_API_KEY").ok();

        // SAFETY: this unit test updates a process env var before constructing the redactor.
        unsafe {
            std::env::set_var("GROQ_API_KEY", "gsk-env-1234567890");
        }

        let redactor = SecretRedactor::from_config_and_env(&config);
        let output =
            redactor.redact_text("config sk-config-1234567890 env gsk-env-1234567890 visible");

        assert_eq!(output, "config [REDACTED] env [REDACTED] visible");

        // SAFETY: cleanup for the env var set by this test.
        unsafe {
            if let Some(previous) = previous {
                std::env::set_var("GROQ_API_KEY", previous);
            } else {
                std::env::remove_var("GROQ_API_KEY");
            }
        }
    }
}
