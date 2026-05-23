# Budget And TUI Smoke Coverage Design

## Context

`LACUNES.md` still lists two open reliability gaps:

- Lacune 7: cost and quota management is in progress. `cortex.run.json` already records basic metrics, `tokens_total` when available, and a cost status, but Cortex does not yet enforce per-run budgets or estimate known provider costs.
- Lacune 15: TUI widgets have headless tests, but longer terminal workflows and keyboard flows are not covered by scenario-style tests.

These gaps are independent enough to implement in parallel. Budget work belongs mostly in config, provider/run metrics, orchestration, and run reports. TUI smoke coverage belongs in test-only helpers around `App` state, input handlers, render paths, and widget invariants.

## Goals

- Add a conservative per-run budget model for tokens and estimated cost.
- Preserve honest reporting when token counts or provider pricing are unavailable.
- Interrupt runs cleanly when a configured token or estimated-cost budget is exceeded.
- Extend `cortex.run.json` with budget limits, budget status, and clear cost notes.
- Add deterministic TUI smoke tests for common keyboard and render flows.
- Keep the tests provider-free, network-free, and suitable for `cargo test`.
- Update `LACUNES.md` when both gaps have executable proof.

## Non-Goals

- Do not guarantee billing-grade cost precision.
- Do not fetch live provider pricing.
- Do not require every provider and model to have a price entry.
- Do not launch a real interactive terminal or pseudo-terminal in this lot.
- Do not snapshot full terminal screens character by character.
- Do not redesign the TUI event loop or provider abstraction unless tests expose a concrete bug.

## Recommended Approach

Implement the lot as two workstreams with a shared completion update in `LACUNES.md`.

The budget workstream should introduce small config fields and a deterministic accounting helper. It should treat token totals as authoritative only when providers emit them. Cost estimation should be opt-in by knowledge: known provider/model prices can produce an `estimated` status; unknown pricing must remain explicit as `unknown`.

The TUI workstream should add scenario-style Rust tests that drive the existing input handlers and render widgets through `TestBackend`. The tests should assert stable state transitions and no-panic rendering across normal and narrow terminal sizes.

## Alternatives Considered

### Strict Billing System

Cortex could block every remote call unless it has exact pricing for the selected model. This would reduce surprise costs, but it would also break unknown custom providers, local providers, and fast-moving model catalogs.

### Full Terminal Harness

A pseudo-terminal harness would be closer to real user behavior. It is also more fragile in CI and likely to overlap with crossterm internals. For this lot, direct handler and `TestBackend` coverage gives better reliability for the same product risk.

### Documentation Only

Documenting provider dashboards and manual TUI checks would be fast, but it would not close either lacune. These gaps need executable regression coverage.

## Budget Design

Add optional budget limits to `LimitsConfig`:

```toml
[limits]
max_qa_iterations = 5
max_tokens_per_call = 8192
max_parallel_workers = 4
max_tokens_per_run = 100000
max_estimated_cost_usd = 5.00
```

The defaults are intentionally permissive for beta use: `max_tokens_per_run = 100000` and `max_estimated_cost_usd = 5.00`. Existing config files that omit the new fields should receive these defaults through serde defaults. A value of `0` disables the corresponding limit, matching the common CLI convention that zero means unlimited.

Add a small accounting type, for example `BudgetState`, that can answer:

- current known token total;
- configured token limit;
- current estimated cost when available;
- configured estimated-cost limit;
- status: `not_applicable`, `unknown`, `within_budget`, or `exceeded`.

The first implementation can estimate cost only for stable, explicitly listed provider/model pairs. It should not guess unknown custom provider prices. For local providers such as Ollama, cost should be `not_applicable` unless future config allows user-supplied pricing.

## Budget Enforcement

Enforcement should happen at run-level boundaries where Cortex already observes events:

- When `WorkflowStats { tokens_total }` arrives, update token usage.
- If `max_tokens_per_run` is exceeded, request cancellation or return a clean budget error.
- If estimated cost is available and `max_estimated_cost_usd` is exceeded, interrupt with a clear budget message.
- If cost is unknown, do not interrupt based on cost; record that the configured cost limit could not be evaluated.

This intentionally avoids pre-call estimation. Pre-call estimation would require prompt tokenization by model family and risks blocking valid runs with poor approximations.

## Run Report Changes

Extend `RunMetrics` or add a nested budget record with:

- `max_tokens_per_run`;
- `max_estimated_cost_usd`;
- `budget_status`;
- `budget_exceeded_reason`;
- `cost_status`;
- `estimated_cost_usd`;
- `cost_notes`.

The report must remain redacted through the existing `SecretRedactor`. It should make the distinction between `estimated` and `unknown` clear enough for beta support and users.

## TUI Smoke Test Design

Add scenario-style tests near the existing TUI tests. The tests should exercise user-level flows through existing handlers where practical:

1. Type a long command and submit it with `Enter`.
2. Navigate command history with `Up` and `Down`.
3. Open and close the interrupt menu through `Esc` and double-`Esc`.
4. Switch execution mode with `Shift+Tab`.
5. Navigate a picker with search text, `Down`, `Enter`, and `Esc`.
6. Render status bar with token counts at normal and narrow widths.
7. Render a complete headless TUI frame with pipeline, agent panel, logs, input, and status bar.

Assertions should focus on stable invariants:

- active mode or overlay state changed as expected;
- input is submitted, cleared, or preserved correctly;
- command history selection is correct;
- picker search and selection state update correctly;
- no render panic at 80x24 and a small viewport such as 40x12;
- status text remains bounded enough not to panic or corrupt state.

## Documentation

Add concise documentation for both workstreams:

- Budget docs should explain token limits, estimated cost limits, unknown pricing, local provider behavior, and where to inspect `cortex.run.json`.
- TUI smoke docs should list covered scenarios and note that full manual terminal QA may still be useful before releases.

The docs can be a single focused file if that keeps the change small.

## LACUNES.md Update

After implementation and verification:

- Mark lacune 7 as `Terminé` only when token budget enforcement, estimated-cost handling, tests, and report fields are present.
- Mark lacune 15 as `Terminé` only when scenario-style TUI tests are in `cargo test`.
- Add a dated entry to "Suivi des lots" for the budget and TUI smoke coverage lot.

## Testing

Verification should include:

- targeted budget unit tests;
- targeted TUI smoke tests;
- `cargo fmt`;
- `cargo test` or the broadest practical test command;
- `cargo check`.

If full `cargo test` is too slow during iteration, run targeted tests first and finish with the broadest practical command before claiming either lacune is complete.
