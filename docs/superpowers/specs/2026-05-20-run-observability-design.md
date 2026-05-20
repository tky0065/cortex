# Run Observability Design

## Context

`LACUNES.md` lists lacune 6 as open: Cortex has verbose logging and TUI events, but no structured way to understand why a run succeeded, failed, stalled, or produced weak output. A multi-agent run can involve provider calls, phases, tools, files, user pauses, cancellation, and generated artifacts. Today those signals are split across the TUI, optional `cortex.log`, and `cortex.manifest.json`.

The existing manifest identifies a generated project after a successful run. It is not a diagnostic artifact, and it is not written for failed runs. Cortex needs a structured run report that can be shared with beta support, used for local debugging, and extended later for budgets, quotas, and safer resume.

## Goals

- Write a structured `cortex.run.json` file for every run, including successful, failed, and interrupted runs.
- Capture a timeline of workflow, phase, agent, tool, file, stats, error, and interruption events.
- Summarize each agent's status, model when known, duration, errors, token chunks, and output character counts.
- Capture files written during the run with paths, basic metadata, and whether they were created or modified.
- Capture tool calls already visible through `TuiEvent::AgentToolCall`.
- Include metrics fields for total duration, tokens when known, approximate output activity, and cost status.
- Redact known secrets before writing the report to disk.
- Keep report generation independent from the TUI so it works in REPL, auto, and future resume flows.
- Update documentation and `LACUNES.md` after implementation.

## Non-Goals

- Do not add budget enforcement in this lot.
- Do not hardcode provider pricing tables.
- Do not promise exact per-agent token counts when providers do not expose them.
- Do not redesign the TUI event model.
- Do not replace `cortex.log`; verbose logs remain useful for full text inspection.
- Do not replace `cortex.manifest.json`; the manifest remains the generated project identity.
- Do not solve checkpointed resume in this lot, though the report should leave room for it.

## Recommended Approach

Use a dedicated run report module fed by a tee of existing `TuiEvent`s. This gives Cortex a useful report without forcing every workflow and agent to adopt a new instrumentation API immediately.

The first implementation should be broad enough to diagnose beta failures and structured enough to extend later, but conservative about precision. Durations and event order can be exact. Token and cost fields should distinguish known values from approximations and unknowns.

## Alternatives Considered

### Minimal Event Report

Write only a normalized event list from existing `TuiEvent`s. This is fast and low-risk, but it leaves users and maintainers to reconstruct agent state manually. It also does not create a natural place for metrics, cost status, or future resume data.

### Full Budget System

Add run reports, provider token accounting, cost estimates, per-run limits, and automatic cancellation before budget overruns. This is attractive, but too risky for this lot because provider support is uneven and exact pricing changes over time.

## Architecture

### `src/run_report.rs`

Add a focused module that owns report data structures, event ingestion, redaction, finalization, and JSON writing.

Core serializable types should include:

```rust
pub struct RunReport {
    pub schema_version: u32,
    pub run_id: String,
    pub cortex_version: String,
    pub workflow: String,
    pub prompt: String,
    pub provider: String,
    pub started_at_unix: u64,
    pub finished_at_unix: Option<u64>,
    pub status: RunStatus,
    pub timeline: Vec<RunTimelineEvent>,
    pub agents: Vec<AgentRunRecord>,
    pub tools: Vec<ToolRunRecord>,
    pub files: Vec<FileRunRecord>,
    pub metrics: RunMetrics,
    pub failure: Option<RunFailure>,
}
```

The exact field names can be adjusted during implementation for Rust style, but the JSON should remain obvious and stable.

### Collector

`RunReportCollector` should hold mutable in-memory state for one run. It should expose methods similar to:

- `new(workflow, prompt, config)`
- `record_event(&TuiEvent)`
- `finish_success()`
- `finish_error(message)`
- `finish_interrupted(message)`
- `write_to(project_dir)`

The collector should redact the prompt and any event text before persisting. Redaction should use `crate::secrets::SecretRedactor::from_config_and_env()`.

### Orchestrator Integration

`src/orchestrator.rs` should create a report collector at the start of `run_with_project_dir()`. Events sent to the TUI should also be sent to the collector through a tee, similar to the verbose log path.

The report should be written in all exit paths:

- workflow returned `Ok(())`: status `success`.
- workflow returned `Err(e)`: status `failed`, with `failure`.
- cancellation token won the `tokio::select!`: status `interrupted`.

The manifest should still be written only on successful runs unless a separate future design changes that behavior.

## Data Model Details

### Timeline

Timeline events should include:

- timestamp as unix milliseconds,
- event type,
- optional agent,
- optional phase,
- short redacted message,
- related path/tool when applicable.

The collector should record at least:

- `WorkflowStarted`
- `AgentStarted`
- `AgentProgress`
- `AgentSummary`
- `TokenChunk`
- `AgentDone`
- `PhaseComplete`
- `Error`
- `AgentToolCall`
- `WorkflowStats`
- `WorkflowComplete`
- `FileWritten`
- `WorkflowInterrupted`

For high-volume `TokenChunk` events, the timeline should not store every raw chunk. It should increment per-agent counters and store compact milestone events or final aggregate data. This prevents `cortex.run.json` from becoming another verbose log.

### Agents

Each agent record should track:

- agent name,
- model when known,
- status: `pending`, `running`, `done`, `error`, or `interrupted`,
- started and finished timestamps,
- duration milliseconds when both timestamps exist,
- token chunk count,
- output character count,
- last progress message,
- error messages.

Model lookup can use the existing config role mapping where possible. If the agent name is dynamic, such as `developer:src/main.rs`, store `model: null` rather than guessing incorrectly.

### Tools

Tool records should be populated from `TuiEvent::AgentToolCall` first:

- agent,
- tool,
- label,
- timestamp,
- status if later events make that clear, otherwise `observed`.

This lot does not require instrumenting every lower-level tool path. If simple call sites already emit tool events, they should be captured automatically by the tee.

### Files

File records should be populated from `TuiEvent::FileWritten`:

- path,
- agent,
- operation: `created`, `modified`, or `unknown`,
- byte length of new content,
- SHA-256 hash of new content using the existing `sha2` dependency,
- timestamp.

`old_content: None` means `created`; `Some(_)` means `modified`.

### Metrics And Cost Fields

Metrics should include:

- total duration milliseconds,
- total token count when `WorkflowStats` provides it,
- total token chunks,
- total output characters,
- agent count,
- file count,
- tool call count,
- `cost_status`: `unknown`, `estimated`, or `not_applicable`,
- `estimated_cost_usd`: nullable,
- `cost_notes`: short explanation.

For this lot, cost status will usually be `unknown` with a note explaining that provider-specific pricing and token accounting are not enforced yet.

### Failure Classification

On failure or interruption, store:

- type: `workflow_error`, `agent_error`, `tool_error`, `provider_error`, `interrupted`, or `unknown`,
- message,
- agent if known,
- phase if known,
- probable cause string.

The first version can infer failure type from available event/error text conservatively. If classification is uncertain, use `unknown` and preserve the redacted message.

## Documentation

Update `README.md` to explain:

- `cortex.run.json` is written for each run.
- `cortex.manifest.json` identifies a generated project; `cortex.run.json` diagnoses the run.
- `cortex.log` remains optional verbose text output.
- Users should review run reports before sharing them, even though known secrets are redacted.

Update beta failure reporting docs or issue template text so users can attach `cortex.run.json` when comfortable.

Update `LACUNES.md` after implementation:

- mark lacune 6 as `Terminé` once timeline, agents, errors, files, basic metrics, and failure summary are implemented.
- mark lacune 7 as `En cours` because metrics/cost fields exist but budget enforcement is not implemented.
- add a dated lot entry for run observability.

## Testing

Add focused unit tests for `RunReportCollector`:

- clean lifecycle records workflow, agent start/done, phase, and success.
- error lifecycle records status `failed` and a failure summary.
- interruption lifecycle records status `interrupted`.
- `FileWritten` records created vs modified and metadata.
- `WorkflowStats` updates total token count.
- high-volume token chunks update counters without storing every chunk in the timeline.
- report writing redacts configured secrets from prompt and event text.

Add an orchestrator-level test if practical:

- a lightweight workflow run writes `cortex.run.json` on success.
- a lightweight failing workflow writes `cortex.run.json` on error.

If an orchestrator-level test requires too much setup, keep it as a focused integration test around the tee/finalization helper rather than forcing a full provider call.

## Acceptance Criteria

- `cargo test` passes.
- A successful run writes `cortex.run.json`.
- A failed or interrupted run still writes `cortex.run.json`.
- The report includes workflow identity, redacted prompt, status, timeline, agent summaries, file records, metrics, and failure details when relevant.
- Known secrets from config/env are redacted in the report.
- README documents the new file and its relationship to existing artifacts.
- `LACUNES.md` marks lacune 6 complete and lacune 7 in progress after implementation.
