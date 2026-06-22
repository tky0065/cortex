# Budget Limits And TUI Smoke Coverage

## Run Budget Limits

Cortex supports conservative per-run budget limits in `~/.cortex/config.toml`:

```toml
[limits]
max_tokens_per_run = 100000
max_estimated_cost_usd = 5.00
```

`max_tokens_per_run` is enforced when a provider or workflow emits aggregate token usage through `WorkflowStats`.

`max_estimated_cost_usd` is enforced only when Cortex has a local static price entry for the selected provider and model. The estimate is not billing-grade. Provider dashboards remain the source of truth for invoices.

Set either value to `0` to disable that limit.

## Run Reports

Every `cortex.run.json` includes budget fields under `metrics`:

- `tokens_total`
- `max_tokens_per_run`
- `max_estimated_cost_usd`
- `budget_status`
- `budget_exceeded_reason`
- `cost_status`
- `estimated_cost_usd`
- `cost_notes`

`budget_status = "unknown"` means Cortex could not evaluate cost because pricing or token totals were unavailable. `budget_status = "not_applicable"` is expected for local providers such as Ollama.

## TUI Smoke Coverage

The Rust test suite includes scenario-style smoke tests for common terminal flows:

- command typing and submission;
- command history navigation;
- interrupt menu open and close;
- execution mode cycling;
- picker search and navigation;
- status bar rendering with token counts;
- full-frame headless rendering at normal and narrow terminal sizes.

These tests are deterministic and run without a real terminal. Manual release QA is still useful for platform-specific terminal behavior.
