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
| Model output to filesystem tool | Model may request path traversal or sandbox escape | Relative path validation, containment checks, and symlink escape checks in `src/tools/filesystem.rs` |
| Web search result to agent prompt | Search result may contain prompt injection or reflected secrets | Web-search context is redacted and explicitly labeled as untrusted external content before injection |
| Email tool output to user | Email body or SMTP errors may contain secrets | Dry-run previews and SMTP errors are redacted |
| Run artifacts to disk | Logs and manifests may persist tokens from prompts or agent output | `cortex.log` and manifest prompt fields are redacted |
| Custom agents and workflows | Custom definitions may request unsafe tools or malformed execution | Structured custom agent/workflow validation in `src/custom_validation.rs`; future fine-grained permissions remain a possible hardening area |
| Updater | Release/update path may be compromised | Release process exists; checksum entries, malformed checksums, and suspicious archive names are covered by deterministic tests |

## Adversaries And Abuse Cases

- Malicious web content that instructs an agent to reveal local secrets.
- Malicious or careless prompt content containing API keys or SMTP credentials.
- Model output that tries to execute shell commands outside the allowlist.
- Model output that tries to read files outside the filesystem sandbox.
- Model output that tries to escape the filesystem sandbox through symbolic links.
- Custom workflow definitions that request unsafe behavior.
- Provider or SMTP errors that include request metadata.

## Controls Added In This Lot

- Central `SecretRedactor` for configured API keys, selected environment secrets, bearer tokens, private key blocks, and common assignment patterns.
- Redaction for verbose logs written to `cortex.log`.
- Redaction for the prompt stored in `cortex.manifest.json`.
- Redaction for email dry-run previews and returned SMTP errors.
- Redaction for web-search context blocks before prompt injection.
- Adversarial tests for redaction and selected tool boundaries.
- Canonical path containment checks that reject symlink escapes outside the filesystem sandbox.
- Explicit untrusted-content labeling for web-search context blocks.
- Adversarial web-search tests for prompt-injection-like snippets and secret-like result content.
- Adversarial custom-definition tests for shell-like tool names, path-like workflow references, and pre-execution validation of referenced agents.
- Composed filesystem and terminal boundary tests.
- Email dry-run default and multi-field redaction tests.
- Updater tests for missing checksums, malformed checksums, and suspicious archive names.

## Remaining Gaps

- Lacune 2 is closed for the beta threat model scope: tool boundaries, custom workflow validation, web-search prompt-injection labeling, email safeguards, secret redaction, and updater checksum/archive-name rejection are documented and tested. A future permission system could further reduce risk, but is outside the beta gap.
- Custom workflows and agents could still benefit from future fine-grained permission prompts and per-tool policy scopes beyond the current validation layer.
- Lacune 20 is closed for the current adversarial suite: composed attacks now cover web search, custom agents/workflows, terminal, filesystem, email, updater, and secret redaction.
- Web-search labeling and redaction reduce prompt-injection and secret-reflection risk, but they do not guarantee that a model will ignore malicious instructions embedded in search results.
- Redaction is best-effort. It reduces accidental leakage in Cortex-owned output surfaces, but it does not prevent users from sending secrets to configured model providers.
