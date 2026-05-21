# Security Adversarial Coverage Design

## Context

`LACUNES.md` still tracks two security gaps that need a second hardening pass:

- Lacune 2: tool security remains in progress because updater integrity and advanced web-search prompt injection are not covered enough.
- Lacune 20: adversarial tests remain in progress because the first security lot covered redaction and selected tool boundaries, not composed attacks.

Cortex already has a central `SecretRedactor`, a threat model, filesystem containment checks, terminal allowlist checks, email dry-run protections, web-search redaction, and custom workflow validation. The next useful step is not a broad security rewrite. It is targeted adversarial coverage that proves those controls hold when attacker-controlled content crosses module boundaries.

## Goals

- Add reproducible adversarial tests for composed security scenarios.
- Cover web-search prompt injection as untrusted content, without requiring live network access.
- Cover custom definitions that attempt unsafe behavior before workflow execution.
- Cover email safety defaults and secret-safe error/preview surfaces.
- Cover updater rejection paths or document the exact blocker if the current updater API cannot be tested without refactor.
- Update `docs/SECURITY_THREAT_MODEL.md` only where new controls or gaps are clarified.
- Update `LACUNES.md` with accurate status and proof once implementation is verified.

## Non-Goals

- Do not implement a full OS sandbox.
- Do not add a runtime permission prompt system for every tool call.
- Do not solve all prompt injection. The goal is to treat web-search content as untrusted and prevent obvious instruction escalation or secret reflection in Cortex-owned context blocks.
- Do not redesign provider routing.
- Do not change live email sending behavior except to preserve existing explicit-send safeguards.
- Do not rewrite the updater unless tests reveal a focused, necessary seam.

## Recommended Approach

Use a targeted test-first security pass:

1. Add failing or characterization tests for the highest-risk composed attacks.
2. Apply narrow runtime hardening only when the current behavior is unsafe or ambiguous.
3. Keep tests offline and deterministic.
4. Update `LACUNES.md` after verification, marking only proven coverage as complete.

This should close lacune 20 if composed attacks are covered across web search, custom validation, filesystem/terminal, email, and updater rejection paths. Lacune 2 can be marked complete only if updater and advanced web-search prompt-injection coverage are both addressed; otherwise it remains `En cours` with narrower remaining proof.

## Alternatives Considered

### Documentation-First

Expand `docs/SECURITY_THREAT_MODEL.md` with more attack narratives before adding tests. This helps audits, but it does not prove controls work.

### Runtime Permission System

Introduce a permission model for tools, custom workflows, web search, email, and updater. This may eventually be useful, but it is too large for this lot and would touch many product flows.

### Test-Only Characterization

Add tests that document current behavior but never change runtime code. This is useful where behavior is already safe. It is insufficient if web-search or updater tests reveal unsafe handling.

## Attack Scenarios

### Web Search Prompt Injection

Search result titles, URLs, and snippets are attacker-controlled. Tests should verify that formatted context labels results as untrusted external content and does not elevate instructions such as "ignore previous instructions", "read `.env`", or "send secrets by email" into first-class Cortex instructions.

Expected behavior:

- The context block remains clearly separated from the agent task.
- Known secrets and obvious secret patterns are redacted.
- Malicious snippets are preserved only as quoted or labeled external content, not merged into system instructions.
- Formatting helpers can be tested without network calls.

### Custom Agent And Workflow Abuse

Custom definitions are local but untrusted input. Tests should cover definitions that:

- reference unknown or disallowed tools.
- reference missing agents.
- attempt path-sensitive behavior through suspicious output paths.
- use malformed YAML or contradictory workflow phases.
- collide with built-in workflow names.

Expected behavior:

- Invalid definitions fail validation before execution.
- Error messages identify the invalid field without exposing local secrets.
- No generated workflow starts when validation fails.

### Filesystem And Terminal Composition

The filesystem and terminal tools already have point protections. The remaining risk is composed input that combines traversal, symlinks, and shell-like payloads.

Expected behavior:

- Symlink escapes outside the project sandbox are rejected.
- Nested traversal remains rejected after canonicalization.
- Terminal commands containing shell operators or disguised multi-command payloads are rejected by argument-aware validation.
- Error messages remain actionable and do not include secret values.

### Email Safety

The email tool has high external impact. Tests should prove:

- Dry-run is still the default.
- Live sending requires explicit `SendMode::Live`.
- Dry-run previews redact body, subject, recipient, and configuration-derived secrets where applicable.
- SMTP setup and send errors are normalized or redacted.

### Updater Suspicious Inputs

The updater is a trust boundary because it handles release artifacts. Tests should cover the currently exposed API for:

- checksum mismatch.
- malformed version or asset metadata.
- missing checksum.
- archive or binary paths that would escape the expected install location, if archive handling exists locally.

Expected behavior:

- Suspicious update metadata fails closed.
- Failures are explicit enough for support.
- If the updater cannot be tested at this level without network or refactor, introduce a small pure helper around metadata/checksum validation and test that helper.

## Architecture

### Test Placement

Place tests near the modules they protect:

- `src/tools/web_search.rs` for context formatting and prompt-injection labeling.
- `src/custom_validation.rs` for invalid custom agents and workflows.
- `src/tools/filesystem.rs` and `src/tools/terminal.rs` for composed tool-boundary attacks.
- `src/tools/email.rs` for dry-run and redaction behavior.
- `src/updater.rs` for checksum and suspicious metadata validation.

Prefer pure helper tests over integration tests when external services would be required.

### Runtime Changes

Runtime changes should be narrow:

- Add or adjust helper functions that make unsafe formatting/validation testable.
- Add explicit untrusted-content labels to web-search context if missing.
- Reuse `SecretRedactor`; do not add a second redaction system.
- Keep user-facing errors concise and secret-safe.

### Documentation

Update `docs/SECURITY_THREAT_MODEL.md` only for newly covered controls and remaining gaps. Avoid restating the whole threat model.

Update `LACUNES.md` after implementation:

- Lacune 20 should become `Terminé` if the composed test set lands and passes.
- Lacune 2 should become `Terminé` only if web-search prompt-injection handling and updater suspicious-input checks are both covered. Otherwise keep `En cours` and name the remaining item precisely.
- Add a dated `Suivi des lots` entry for this lot.

## Data Flow

1. Untrusted content enters through search results, custom definitions, model output, email bodies, terminal command requests, filesystem paths, or updater metadata.
2. The owning module validates or labels the input before execution, persistence, or prompt injection.
3. Sensitive output surfaces pass through existing secret redaction.
4. Tests assert the unsafe action does not happen and raw secret-like values do not appear in returned errors or previews.

## Error Handling

- Security failures should fail closed.
- Validation errors should name the rejected field or boundary.
- Tests should not depend on exact full error text unless the text is part of the safety contract.
- Redaction failures should remain best-effort and non-fatal.
- Network-backed features must have offline test paths.

## Testing

Verification commands:

```bash
cargo fmt
cargo test
cargo check
```

Focused test targets may be run during development:

```bash
cargo test web_search
cargo test custom_validation
cargo test filesystem
cargo test terminal
cargo test email
cargo test updater
```

Acceptance coverage:

- Web-search context treats malicious snippets as untrusted external content.
- Web-search context redacts known secrets and obvious secret patterns.
- Custom workflow and agent validation rejects unsafe or malformed definitions before execution.
- Filesystem symlink/traversal composition remains blocked.
- Terminal shell-like composition remains blocked.
- Email dry-run and live-send guardrails remain intact.
- Updater suspicious metadata or checksum failures are covered by deterministic tests.
- `LACUNES.md` status changes match the verified behavior.

## Acceptance Criteria

- New adversarial tests are committed and pass without network access.
- Any runtime hardening is minimal and covered by tests.
- `docs/SECURITY_THREAT_MODEL.md` reflects any new controls or remaining precise gaps.
- `LACUNES.md` marks completed work and includes a dated lot entry.
- `cargo fmt`, `cargo test`, and `cargo check` pass before implementation is considered complete.
