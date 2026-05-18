# Security And Secrets Hardening Design

## Context

`LACUNES.md` still lists security and secret-handling risks as open work:

- Lacune 2: tool and external-content security is not covered by a complete threat model.
- Lacune 20: adversarial security tests are incomplete.
- Lacune 22: secrets are not centrally masked before being written to logs, manifests, or tool previews.

Cortex already has useful point protections: command allowlisting in `src/tools/terminal.rs`, path traversal checks in `src/tools/filesystem.rs`, generated-project eval checks for secrets, and project-context instructions that avoid reading obvious secret files. The remaining risk is cross-cutting: several output surfaces can still echo sensitive values if a user prompt, agent output, provider error, web-search query, email body, or environment-derived error contains a token.

This lot adds a focused, testable secret-redaction layer and a short threat model. It does not attempt to solve every security boundary in Cortex.

## Goals

- Add a central secret-redaction module used by sensitive output paths.
- Prevent known secrets from being persisted in `cortex.log` and `cortex.manifest.json`.
- Prevent email dry-run previews and SMTP error messages from exposing obvious secrets.
- Prevent web-search context blocks from reflecting known secrets in injected `Query:` or offline stub text.
- Add adversarial tests for the initial hardening layer.
- Document the threat model for tools, providers, web search, email, updater, custom agents, and custom workflows.
- Update `LACUNES.md` with accurate completion/progress markers.

## Non-Goals

- Do not add an OS sandbox or container runtime sandbox.
- Do not implement a full workflow permission system.
- Do not block users from intentionally sending prompt content to model providers.
- Do not validate all custom workflow schemas in this lot; that remains lacune 8.
- Do not implement updater signature verification in this lot.
- Do not make web search safe against all prompt injection; this lot only prevents obvious secret reflection and documents the remaining risk.

## Recommended Approach

Use targeted, testable hardening:

1. Add central redaction primitives.
2. Apply them to the output surfaces most likely to persist or display sensitive data.
3. Add tests that prove the hardening behavior without live providers or network calls.
4. Document the broader threat model and remaining gaps.

This closes lacune 22 if tests pass, and moves lacunes 2 and 20 to `En cours` with concrete proof.

## Alternatives Considered

### Documentation Only

Write only a threat model and abuse matrix. This is low risk, but it does not reduce runtime leakage risk and should not mark lacune 22 complete.

### Broad Security Refactor

Rework all tool permissions, custom workflows, updater verification, provider boundaries, and web-search injection in one lot. This is more complete, but too large and risky for a focused change.

## Architecture

### `src/secrets.rs`

Add a pure redaction module with no network I/O.

Core responsibilities:

- Build a redactor from `Config` and selected environment variables.
- Collect configured API keys from `Config::api_keys`.
- Collect custom provider `api_key` values.
- Collect selected environment variables used by providers and tools, including:
  - `OPENAI_API_KEY`
  - `ANTHROPIC_API_KEY`
  - `GEMINI_API_KEY`
  - `MISTRAL_API_KEY`
  - `DEEPSEEK_API_KEY`
  - `XAI_API_KEY`
  - `COHERE_API_KEY`
  - `PERPLEXITY_API_KEY`
  - `HUGGINGFACE_API_KEY`
  - `AZURE_OPENAI_API_KEY`
  - `OPENROUTER_API_KEY`
  - `GROQ_API_KEY`
  - `TOGETHER_API_KEY`
  - `WEB_SEARCH_API_KEY`
  - `SMTP_PASS`
- Ignore empty and very short values to avoid destructive false positives.
- Deduplicate secrets.
- Replace known secret values with `[REDACTED]`.
- Redact conservative textual patterns in output strings:
  - `Bearer <long-token>`
  - private key blocks
  - `api_key=<value>`
  - `token=<value>`
  - `password=<value>`
  - `secret=<value>`

The module should expose a small API such as:

```rust
pub struct SecretRedactor { ... }

impl SecretRedactor {
    pub fn from_config_and_env(config: &Config) -> Self;
    pub fn redact_text(&self, input: &str) -> String;
}
```

Exact names can follow local style during implementation.

### `src/orchestrator.rs`

Apply redaction in two places:

- Verbose log writer: every `TuiEvent::TokenChunk` line written to `cortex.log` is redacted.
- Manifest writer: the `prompt` field in `cortex.manifest.json` is redacted before serialization.

This protects persisted local artifacts. It does not alter the prompt sent to agents.

### `src/tools/email.rs`

Apply redaction to returned strings and wrapped errors:

- `SendMode::DryRun` still returns a useful preview, but the preview is redacted before being returned.
- SMTP setup/build/send errors are normalized or redacted so configured SMTP secrets are not included.

The live-send path still reads SMTP credentials from the environment and sends via STARTTLS as before.

### `src/tools/web_search.rs`

Prevent reflected secret leakage in generated context blocks:

- When formatting `Query:` in DuckDuckGo Lite results, use a redacted query string.
- When formatting the offline Brave stub, use a redacted query string.
- When formatting API-backed result blocks, redact title, URL, and snippet text before injection.

If the implementation needs a redactor but only has `Config` in `fetch_context`, derive it there and pass it down to formatting helpers. The lower-level `search()` function may remain provider-focused and unredacted internally as long as its returned user-visible context is redacted before injection.

## Data Flow

1. Config loads API keys and applies them to environment variables as it does today.
2. A run starts and constructs output events, logs, manifests, web-search context, and email previews.
3. Before a sensitive output is persisted or returned for display, the relevant code builds or receives a `SecretRedactor`.
4. The redactor removes known secret values and obvious secret-like patterns.
5. Tests assert that raw secret strings are absent from the output surfaces.

## Error Handling

- Redaction must be best-effort and non-fatal.
- If a redactor cannot read an environment variable, it treats it as absent.
- Redaction must not panic on invalid UTF-8 because all current surfaces operate on Rust `String` values.
- SMTP errors should remain actionable without including host credentials or passwords.
- Web-search failures continue to return empty context as they do today.

## Security Constraints

- Redaction is not a substitute for permission checks or sandboxing.
- Redaction should not mutate generated project files.
- Redaction should not silently remove large unrelated parts of user text.
- Known short values are ignored to avoid masking common words.
- The raw prompt may still be sent to the configured model provider; privacy documentation must remain clear about that.
- Prompt-injection defenses for web results remain a separate hardening area.

## Testing

Add focused tests without live network or provider dependencies:

- `src/secrets.rs`
  - redacts exact configured API keys.
  - redacts selected environment secrets.
  - redacts bearer tokens.
  - redacts private key blocks.
  - redacts assignment patterns such as `password=...` and `api_key=...`.
  - ignores very short values.
  - does not alter unrelated text.
- `src/orchestrator.rs`
  - manifest prompt redaction test.
  - verbose log redaction test, if feasible without running a full workflow.
- `src/tools/email.rs`
  - dry-run preview redacts a secret in the body.
  - live-send configuration errors do not expose SMTP secrets.
- `src/tools/web_search.rs`
  - offline stub or context formatting does not echo a known secret in the query.
- Existing filesystem and terminal adversarial tests remain, with one additional case if the code lacks coverage for symlink escape or shell-like command rejection.

Verification commands:

```bash
cargo fmt
cargo test
cargo check
```

## Documentation

Add `docs/SECURITY_THREAT_MODEL.md` covering:

- protected assets: source files, generated outputs, local config, API keys, SMTP credentials, auth tokens, logs, manifests.
- trust boundaries: user prompt, provider responses, web search results, custom agent definitions, custom workflows, local filesystem, terminal commands, email sending, updater.
- adversaries: malicious web content, malicious prompt content, compromised custom workflow, accidental user secret inclusion, provider error leakage.
- current controls.
- new controls from this lot.
- known remaining gaps mapped to `LACUNES.md`.

## `LACUNES.md` Updates

After implementation and verification:

- Mark lacune 22 as `Terminé` with proof pointing to `src/secrets.rs`, tests, and output-surface integration.
- Mark lacune 2 as `En cours` with proof pointing to `docs/SECURITY_THREAT_MODEL.md` and first runtime protections.
- Mark lacune 20 as `En cours` with proof pointing to adversarial tests added in this lot.
- Add a new entry under `Suivi des lots` for the security/secrets hardening lot.

Do not mark lacune 2 or 20 complete until the broader tool, updater, custom workflow, and web-search prompt-injection risks have dedicated coverage.

## Acceptance Criteria

- `src/secrets.rs` or equivalent central module exists and is covered by unit tests.
- `cortex.log` verbose output redacts known secrets.
- `cortex.manifest.json` redacts known secrets in the prompt field.
- Email dry-run previews redact known or obvious secrets.
- Web-search injected context does not reflect known secrets in query/result text.
- `docs/SECURITY_THREAT_MODEL.md` documents current controls and remaining gaps.
- `LACUNES.md` accurately marks lacunes 2, 20, and 22.
- `cargo fmt`, `cargo test`, and `cargo check` pass.
