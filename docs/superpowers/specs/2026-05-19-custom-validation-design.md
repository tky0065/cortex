# Custom Definition Validation Design

## Context

`LACUNES.md` lists lacune 8 as open: custom agents and custom workflows are too permissive. Cortex currently parses Markdown files with YAML frontmatter, discovers definitions from `.cortex/agents/`, `.cortex/workflows/`, `~/.cortex/agents/`, and `~/.cortex/workflows/`, and can run a custom workflow even when a referenced agent is missing by falling back to a generic agent.

That behavior is convenient for experimentation, but it creates confusing failures and weakens the safety boundary around user-defined workflows. A malformed or risky custom definition should be diagnosed before execution, and critical problems should block the run with a clear message.

## Goals

- Add structured validation for custom agents and custom workflows.
- Add a CLI command that validates discovered custom definitions.
- Add a REPL command with the same validation behavior.
- Validate custom workflows automatically before execution.
- Block only critical errors, while reporting non-blocking warnings.
- Replace missing-agent fallback during custom workflow execution with a validation error.
- Update documentation and `LACUNES.md` after implementation.

## Non-Goals

- Do not build a full permissions UI.
- Do not remove support for custom agents or workflows.
- Do not validate built-in workflow Rust modules.
- Do not require live provider calls to validate definitions.
- Do not redesign the custom workflow file format.
- Do not solve all prompt-injection risks; this lot only validates local custom definition structure and declared tools.

## Recommended Approach

Use a hybrid validation model:

1. Provide explicit validation commands for users.
2. Run validation automatically before custom workflow execution.
3. Treat structural and safety problems as errors.
4. Treat compatibility or quality concerns as warnings.

This improves reliability without making every imperfect definition unusable.

## Alternatives Considered

### Non-Blocking Validation Only

Add `/validate` and `cortex validate`, but keep runtime behavior unchanged. This is less disruptive, but it does not close the main reliability and safety gap because invalid workflows can still run.

### Fully Strict Validation

Block any warning before execution. This is safer but too harsh for beta custom definitions, especially because the parser already tolerates some AI-generated YAML variants.

## Architecture

### `src/custom_validation.rs`

Add a dedicated module for validation logic. Parsing and file discovery can remain in `custom_defs.rs` and `agent_loader.rs`; the validator consumes those primitives and adds strict rules.

Core types:

```rust
pub enum ValidationSeverity {
    Error,
    Warning,
}

pub struct ValidationDiagnostic {
    pub severity: ValidationSeverity,
    pub path: std::path::PathBuf,
    pub target: String,
    pub code: &'static str,
    pub message: String,
}

pub struct ValidationReport {
    pub diagnostics: Vec<ValidationDiagnostic>,
}
```

`ValidationReport` should expose helpers such as:

- `has_errors()`
- `error_count()`
- `warning_count()`
- `format_human()`

The exact names can follow local Rust style during implementation.

### Validator Responsibilities

The validator should support:

- validating one agent file.
- validating one workflow file.
- validating all discovered local and global custom definitions.
- validating one named custom workflow before execution.

It should resolve definitions using the same shadowing rules as the runtime: project-local definitions take priority over global definitions.

### CLI Integration

Add:

```bash
cortex validate
```

The command validates all discovered custom agents and workflows from the current project and user home. It prints a human-readable report and exits non-zero if errors exist.

### REPL Integration

Add:

```text
/validate
```

The command emits the same report into the TUI logs. It does not start or stop a workflow.

### Runtime Integration

Before `workflows::get_workflow(custom_name)` returns a `CustomWorkflow`, validate the named workflow and its referenced agents.

If validation has errors, return an error that includes the formatted report. If validation has only warnings, allow execution and surface the warnings in the TUI logs where practical.

The current missing-agent fallback in `src/workflows/custom.rs` should no longer be reachable for validated workflow runs. It may be removed or kept only as defensive unreachable behavior, but runtime behavior should be: referenced custom agents must exist before execution.

## Validation Rules

### Names

Names are errors when they are empty or contain unsafe path-like syntax.

Allowed custom names:

```text
^[a-zA-Z0-9_-]+$
```

Disallowed examples:

- `../agent`
- `foo/bar`
- `agent.md`
- `agent name`
- empty string

### Custom Agent Rules

Errors:

- invalid or missing YAML frontmatter.
- missing or empty `name`.
- missing or empty `description`.
- missing or empty `model`.
- empty prompt body.
- invalid name format.
- unknown tool.

Warnings:

- description is very short.
- prompt body is very long.
- model has no provider prefix.
- filename stem differs from the declared `name`.
- custom agent declares a sensitive tool.

Known tools for this lot:

- `filesystem`
- `terminal`
- `web_search`
- `email`

Sensitive tools for warning purposes:

- `terminal`
- `email`

### Custom Workflow Rules

Errors:

- invalid or missing YAML frontmatter.
- missing or empty `name`.
- missing or empty `description`.
- missing or empty `agents`.
- invalid workflow name format.
- declared workflow name collides with a built-in workflow: `dev`, `marketing`, `prospecting`, or `code-review`.
- any workflow step has an empty `role`.
- any workflow step has an empty `agent`.
- any step role is duplicated.
- any referenced agent is missing after applying local-over-global shadowing.

Warnings:

- filename stem differs from declared `name`.
- workflow body is empty.
- workflow has many steps, because long custom pipelines can be expensive and harder to debug.

## Expected Output

Example CLI failure:

```text
Custom definition validation failed

ERROR .cortex/workflows/outreach.md [workflow:outreach] missing-agent
  step 'writer' references missing agent 'cold_email_writer'

WARNING .cortex/agents/sender.md [agent:sender] sensitive-tool
  custom agent uses email; verify dry-run/send behavior before running

2 diagnostics: 1 error, 1 warning
```

Example success with warnings:

```text
Custom definition validation passed with warnings

WARNING .cortex/agents/writer.md [agent:writer] model-without-provider
  model 'qwen2.5-coder:32b' has no provider prefix; Cortex will route through the active provider

1 diagnostic: 0 errors, 1 warning
```

Example clean success:

```text
Custom definition validation passed

0 diagnostics: 0 errors, 0 warnings
```

## Error Handling

- Validation should collect as many diagnostics as possible instead of failing at the first issue.
- File read errors are validation errors with path context.
- Parse errors are validation errors with path context.
- Validation should not panic on malformed custom files.
- Runtime validation errors should be concise but include enough detail for the user to fix the file.
- CLI `cortex validate` exits with status `1` when errors exist and `0` otherwise.

## Testing

Add focused tests without live providers:

- valid agent produces no diagnostics.
- invalid agent YAML produces an error.
- unknown agent tool produces an error.
- sensitive agent tool produces a warning.
- agent with empty body produces an error.
- workflow with no agents produces an error.
- workflow with a missing referenced agent produces an error.
- workflow with duplicated roles produces an error.
- custom workflow named `dev` produces an error.
- `workflows::get_workflow()` refuses an invalid custom workflow.
- human report formatting includes severity, path, target, code, and summary counts.

Verification commands:

```bash
cargo fmt
cargo test
cargo check
```

## Documentation

Update `README.md` to document:

- `cortex validate`
- `/validate`
- validation before custom workflow execution.
- the removal of missing-agent fallback as normal runtime behavior.

Update `LACUNES.md` after implementation:

- Mark lacune 8 as `Terminé`.
- Proof should mention `src/custom_validation.rs`, `cortex validate`, `/validate`, pre-execution validation, and tests.
- Add a lot entry under `Suivi des lots`.

## Acceptance Criteria

- A custom validation module exists and is covered by unit tests.
- `cortex validate` reports discovered custom definition errors and warnings.
- `/validate` reports the same validation results in the REPL/TUI logs.
- Custom workflow execution is blocked when validation errors exist.
- Missing custom agents are errors, not runtime fallback behavior.
- Warnings do not block execution.
- `README.md` documents the validation commands and behavior.
- `LACUNES.md` marks lacune 8 complete only after code and tests are verified.
- `cargo fmt`, `cargo test`, and `cargo check` pass.
