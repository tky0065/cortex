# Concurrency And Cancellation Stress Design

## Context

`LACUNES.md` lists lacune 23 as open: Cortex uses Tokio, parallel workers, cancellation tokens, and an event bus, but the failure modes around cancellation and concurrent event flow are not stress-tested enough. Existing tests cover many normal paths and specific features, yet they do not prove that a slow provider, interrupted run, closed receiver, or failing parallel worker exits cleanly.

This matters because multi-agent runs can fail partially. A freeze, dropped final state, duplicate terminal event, or corrupt report/checkpoint is more damaging than a normal error message. The goal of this lot is to add deterministic coverage for those cases without depending on network providers or a real interactive terminal.

## Goals

- Add deterministic Rust tests for cancellation and concurrent event flow.
- Cover cancellation during a slow in-flight workflow or provider-like step.
- Cover worker failure or panic-like errors without deadlocking the orchestrator.
- Cover closed or lagging event consumers where practical.
- Verify interrupted and failed runs still write readable diagnostic artifacts.
- Keep the tests fast enough for `cargo test` and CI.
- Update `LACUNES.md` when the stress coverage is implemented.

## Non-Goals

- Do not add a production retry system in this lot.
- Do not redesign the orchestrator, event bus, TUI loop, or workflow trait unless a test exposes a concrete bug.
- Do not use real LLM providers, web search, terminal commands, or SMTP in these tests.
- Do not add flaky wall-clock stress tests that rely on long sleeps or machine load.
- Do not solve budget or cost tracking; that remains lacune 7.
- Do not add full interactive terminal snapshot testing; that remains lacune 15.

## Recommended Approach

Use focused fake workflows and fake event consumers inside Rust tests. The fakes should exercise the same public orchestrator paths that normal workflows use, but with deterministic synchronization via `tokio::sync` primitives such as `Notify`, `Barrier`, `oneshot`, and bounded channels.

The test suite should start with the smallest integration surface that can prove the behavior. If the current code makes a behavior hard to test, add narrow test hooks or small helper abstractions rather than broad refactors. Implementation changes should be driven by failing tests.

## Alternatives Considered

### Large End-To-End Stress Runner

A separate command could launch many real Cortex runs in parallel and interrupt them randomly. This could find issues, but it would be slow, expensive with remote providers, and hard to make deterministic in CI.

### TUI-Level Keyboard Stress Tests

Simulating full user input through the terminal would catch some cancellation issues, but it belongs with lacune 15. For lacune 23, the priority is the core concurrency behavior underneath the TUI.

### Manual Review Only

Auditing the async code can identify risks, but it does not prevent regressions. This lacune should be closed by executable tests that fail when cancellation or event handling regresses.

## Test Architecture

### Fake Workflows

Add test-only workflow implementations close to the orchestrator tests. They should support scenarios such as:

- `SlowWorkflow`: emits a start event, signals that it is in flight, then waits until cancelled.
- `FailingWorkflow`: emits one or more events and returns an error.
- `ParallelWorkersWorkflow`: starts several tasks, emits interleaved events, and joins them with controlled success or failure.
- `ArtifactWorkflow`: writes a small file event or uses the existing report path so cancellation and failure artifact behavior can be asserted.

These fakes should avoid real sleeps where possible. Short timeouts are acceptable only as guards to fail fast on deadlock.

### Event Consumer Scenarios

Tests should exercise event handling with:

- an active receiver that drains events normally,
- a dropped receiver before or during workflow execution,
- a lagging receiver when the channel type supports it,
- enough concurrent events to verify final status is still emitted and the run completes.

If the existing event channel intentionally drops messages when no receiver exists, the test should assert that this is non-fatal rather than requiring perfect delivery.

### Artifact Assertions

For interrupted and failed runs, tests should read generated artifacts from a temporary project directory and assert:

- `cortex.run.json` exists when the orchestrator is expected to write it,
- the JSON parses successfully,
- status is `interrupted` or `failed` as appropriate,
- the failure/interruption message is present and redacted,
- any checkpoint behavior touched by the test remains readable.

The tests should not assert fragile full JSON snapshots. They should inspect stable fields only.

## Expected Test Cases

1. `orchestrator_cancellation_interrupts_slow_workflow`
   Start a slow fake workflow, wait until it is in flight, trigger the cancellation token, and assert the run exits within a short timeout with an interrupted report.

2. `orchestrator_failure_does_not_deadlock_event_stream`
   Run a fake workflow that emits events and returns an error. Assert the orchestrator returns an error or records failed status cleanly, and the event drain finishes.

3. `orchestrator_survives_dropped_event_receiver`
   Drop the TUI/event receiver before running a fake workflow. Assert event send failures do not panic or hang the run.

4. `parallel_worker_failure_cancels_or_joins_siblings`
   Run a fake workflow with multiple worker tasks where one fails. Assert the workflow joins or aborts siblings deterministically and no background task keeps the test alive.

5. `parallel_event_burst_preserves_final_state`
   Emit many interleaved progress/token events from fake workers, then a final completion or failure event. Assert the run report stores a coherent final status and bounded aggregate data.

6. `cancelled_run_artifacts_remain_readable`
   Cancel a run after at least one event or file record. Assert report/checkpoint artifacts, if created by that path, parse successfully and do not contain partial JSON.

## Implementation Notes

- Prefer `tempfile::tempdir()` for project directories.
- Prefer `tokio::time::timeout()` as a deadlock guard around awaited runs.
- Keep timeouts short but not brittle, for example one to three seconds for tests that should complete immediately.
- Use `CancellationToken` directly instead of simulating keyboard input.
- If a spawned task is introduced in a test, make the test await or abort it explicitly.
- If a production bug is found, fix the smallest affected path and keep the regression test.

## Documentation

Update `LACUNES.md` after implementation:

- mark lacune 23 as `Termine`;
- replace the proof with the test files and scenarios added;
- add a dated lot entry for concurrency and cancellation stress coverage.

No README update is required unless implementation changes user-visible cancellation behavior.

## Testing

Verification for this lot should include:

- `cargo fmt`
- `cargo test` or the narrow stress test module while iterating
- `cargo check`

If the full suite is too slow during iteration, run the targeted tests first and finish with the broadest practical command before marking the lacune complete.
