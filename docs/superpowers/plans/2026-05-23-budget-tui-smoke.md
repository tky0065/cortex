# Budget And TUI Smoke Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close `LACUNES.md` gaps 7 and 15 by adding conservative run budget enforcement/reporting and deterministic TUI smoke tests.

**Architecture:** Add a focused budget module that owns budget status, cost estimation, and limit checks; wire it into config, run reports, and the orchestrator event tee. Add TUI smoke tests as test-only helpers around existing handlers and `ratatui::backend::TestBackend`, avoiding a real terminal.

**Tech Stack:** Rust, Tokio, serde/TOML, ratatui `TestBackend`, crossterm events, existing `TuiEvent` event bus, existing `RunReportCollector`.

---

## File Structure

- Create `src/budget.rs`: budget limit types, budget status enum, provider/model price lookup, `BudgetState`, and unit tests.
- Modify `src/main.rs`: add `mod budget;`.
- Modify `src/config.rs`: add serde-defaulted `LimitsConfig.max_tokens_per_run` and `LimitsConfig.max_estimated_cost_usd`.
- Modify `src/run_report.rs`: add budget fields to `RunMetrics`, initialize and update budget data from `BudgetState`.
- Modify `src/orchestrator.rs`: update budget state while teeing events; cancel runs when token or estimated-cost limits are exceeded.
- Modify `src/tui/mod.rs`: add test-only constructors/helpers and scenario tests for keyboard flows and full-frame rendering.
- Modify `src/tui/widgets/status_bar.rs`: add narrow-width status bar tests.
- Create `docs/BUDGET_AND_TUI_SMOKE.md`: short user/maintainer docs for budget behavior and TUI smoke coverage.
- Modify `LACUNES.md`: mark lacunes 7 and 15 as complete after tests and docs pass.

## Parallelization Notes

Tasks 1-5 are the budget workstream and should be owned by one worker. Tasks 6-7 are the TUI smoke workstream and can be owned by another worker at the same time. Task 8 must happen after both streams pass because it updates docs and `LACUNES.md`.

### Task 1: Budget Core Module

**Files:**
- Create: `src/budget.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create failing budget tests**

Add `src/budget.rs` with the tests first:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    NotApplicable,
    Unknown,
    WithinBudget,
    Exceeded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetLimits {
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetSnapshot {
    pub tokens_total: Option<u64>,
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
    pub estimated_cost_usd: Option<f64>,
    pub status: BudgetStatus,
    pub exceeded_reason: Option<String>,
    pub cost_notes: String,
}

#[derive(Debug, Clone)]
pub struct BudgetState {
    provider: String,
    model: String,
    limits: BudgetLimits,
    tokens_total: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_provider_is_not_applicable_for_cost_until_tokens_arrive() {
        let state = BudgetState::new("ollama", "qwen2.5-coder:32b", BudgetLimits {
            max_tokens_per_run: 100_000,
            max_estimated_cost_usd: 5.0,
        });

        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::NotApplicable);
        assert_eq!(snapshot.estimated_cost_usd, None);
        assert_eq!(snapshot.exceeded_reason, None);
    }

    #[test]
    fn token_limit_exceeded_when_known_total_is_above_limit() {
        let mut state = BudgetState::new("ollama", "qwen2.5-coder:32b", BudgetLimits {
            max_tokens_per_run: 10,
            max_estimated_cost_usd: 0.0,
        });

        state.record_tokens_total(11);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::Exceeded);
        assert_eq!(
            snapshot.exceeded_reason.as_deref(),
            Some("token budget exceeded: 11 > 10")
        );
    }

    #[test]
    fn zero_limits_disable_enforcement() {
        let mut state = BudgetState::new("openai", "gpt-4.1", BudgetLimits {
            max_tokens_per_run: 0,
            max_estimated_cost_usd: 0.0,
        });

        state.record_tokens_total(1_000_000);
        let snapshot = state.snapshot();

        assert_ne!(snapshot.status, BudgetStatus::Exceeded);
        assert!(snapshot.exceeded_reason.is_none());
    }

    #[test]
    fn known_openai_model_estimates_cost_and_can_exceed_limit() {
        let mut state = BudgetState::new("openai", "gpt-4.1", BudgetLimits {
            max_tokens_per_run: 0,
            max_estimated_cost_usd: 0.0001,
        });

        state.record_tokens_total(10_000);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::Exceeded);
        assert!(snapshot.estimated_cost_usd.unwrap() > 0.0001);
        assert_eq!(
            snapshot.exceeded_reason.as_deref(),
            Some("estimated cost budget exceeded")
        );
    }

    #[test]
    fn unknown_remote_provider_reports_unknown_cost_without_blocking() {
        let mut state = BudgetState::new("custom_llm", "my-model", BudgetLimits {
            max_tokens_per_run: 100_000,
            max_estimated_cost_usd: 5.0,
        });

        state.record_tokens_total(1000);
        let snapshot = state.snapshot();

        assert_eq!(snapshot.status, BudgetStatus::Unknown);
        assert_eq!(snapshot.estimated_cost_usd, None);
        assert!(snapshot.cost_notes.contains("No local price entry"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test budget::tests -- --nocapture`

Expected: compile errors for missing `BudgetState::new`, `record_tokens_total`, and `snapshot`.

- [ ] **Step 3: Implement budget module**

Replace the non-test part of `src/budget.rs` with:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    NotApplicable,
    Unknown,
    WithinBudget,
    Exceeded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetLimits {
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetSnapshot {
    pub tokens_total: Option<u64>,
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
    pub estimated_cost_usd: Option<f64>,
    pub status: BudgetStatus,
    pub exceeded_reason: Option<String>,
    pub cost_notes: String,
}

#[derive(Debug, Clone)]
pub struct BudgetState {
    provider: String,
    model: String,
    limits: BudgetLimits,
    tokens_total: Option<u64>,
}

impl BudgetState {
    pub fn new(provider: impl Into<String>, model: impl Into<String>, limits: BudgetLimits) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            limits,
            tokens_total: None,
        }
    }

    pub fn record_tokens_total(&mut self, tokens_total: u64) {
        self.tokens_total = Some(tokens_total);
    }

    pub fn snapshot(&self) -> BudgetSnapshot {
        let estimated_cost_usd = self
            .tokens_total
            .and_then(|tokens| estimate_cost_usd(&self.provider, &self.model, tokens));

        if let Some(tokens) = self.tokens_total {
            if self.limits.max_tokens_per_run > 0 && tokens > self.limits.max_tokens_per_run {
                return BudgetSnapshot {
                    tokens_total: self.tokens_total,
                    max_tokens_per_run: self.limits.max_tokens_per_run,
                    max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
                    estimated_cost_usd,
                    status: BudgetStatus::Exceeded,
                    exceeded_reason: Some(format!(
                        "token budget exceeded: {} > {}",
                        tokens, self.limits.max_tokens_per_run
                    )),
                    cost_notes: self.cost_notes(estimated_cost_usd),
                };
            }
        }

        if let Some(cost) = estimated_cost_usd {
            if self.limits.max_estimated_cost_usd > 0.0 && cost > self.limits.max_estimated_cost_usd {
                return BudgetSnapshot {
                    tokens_total: self.tokens_total,
                    max_tokens_per_run: self.limits.max_tokens_per_run,
                    max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
                    estimated_cost_usd,
                    status: BudgetStatus::Exceeded,
                    exceeded_reason: Some("estimated cost budget exceeded".to_string()),
                    cost_notes: self.cost_notes(estimated_cost_usd),
                };
            }

            return BudgetSnapshot {
                tokens_total: self.tokens_total,
                max_tokens_per_run: self.limits.max_tokens_per_run,
                max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
                estimated_cost_usd,
                status: BudgetStatus::WithinBudget,
                exceeded_reason: None,
                cost_notes: self.cost_notes(estimated_cost_usd),
            };
        }

        let status = if is_local_provider(&self.provider) {
            BudgetStatus::NotApplicable
        } else {
            BudgetStatus::Unknown
        };

        BudgetSnapshot {
            tokens_total: self.tokens_total,
            max_tokens_per_run: self.limits.max_tokens_per_run,
            max_estimated_cost_usd: self.limits.max_estimated_cost_usd,
            estimated_cost_usd,
            status,
            exceeded_reason: None,
            cost_notes: self.cost_notes(estimated_cost_usd),
        }
    }

    fn cost_notes(&self, estimated_cost_usd: Option<f64>) -> String {
        if estimated_cost_usd.is_some() {
            return "Estimated from local static provider/model pricing; actual billing may differ.".to_string();
        }
        if is_local_provider(&self.provider) {
            return "Local provider cost is not applicable; token budget can still be enforced when token totals are available.".to_string();
        }
        format!(
            "No local price entry for provider '{}' and model '{}'; cost budget could not be evaluated.",
            self.provider, self.model
        )
    }
}

fn is_local_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "ollama" | "lmstudio" | "local"
    )
}

fn estimate_cost_usd(provider: &str, model: &str, tokens_total: u64) -> Option<f64> {
    let provider = provider.trim().to_ascii_lowercase();
    let model = model.trim().to_ascii_lowercase();
    let usd_per_million_tokens = match (provider.as_str(), model.as_str()) {
        ("openai", "gpt-4.1") | ("openai_chatgpt", "gpt-4.1") => 3.0,
        ("openai", "gpt-4.1-mini") | ("openai_chatgpt", "gpt-4.1-mini") => 0.8,
        ("openrouter", model) if model.contains("openai/gpt-4.1") => 3.0,
        ("groq", model) if model.contains("llama") => 0.6,
        ("together", model) if model.contains("qwen") => 1.2,
        _ => return None,
    };

    Some((tokens_total as f64 / 1_000_000.0) * usd_per_million_tokens)
}
```

Keep the tests from Step 1 below this implementation.

- [ ] **Step 4: Register the module**

Add this line near the other `mod` declarations in `src/main.rs`:

```rust
mod budget;
```

- [ ] **Step 5: Run tests**

Run: `cargo test budget::tests -- --nocapture`

Expected: all budget tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/budget.rs src/main.rs
git commit -m "feat: add run budget accounting"
```

### Task 2: Budget Config Defaults

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write failing config tests**

Add these tests near the end of `src/config.rs` inside a new `#[cfg(test)] mod tests` block.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_include_run_budget() {
        let config = Config::default();

        assert_eq!(config.limits.max_tokens_per_run, 100_000);
        assert_eq!(config.limits.max_estimated_cost_usd, 5.0);
    }

    #[test]
    fn old_config_without_budget_fields_uses_defaults() {
        let raw = r#"
[provider]
default = "ollama"

[models]
ceo = "qwen2.5-coder:32b"
pm = "qwen2.5-coder:32b"
tech_lead = "qwen2.5-coder:32b"
developer = "qwen2.5-coder:32b"
qa = "qwen2.5-coder:14b"
devops = "qwen2.5-coder:14b"
assistant = "qwen2.5-coder:32b"

[limits]
max_qa_iterations = 5
max_tokens_per_call = 8192
max_parallel_workers = 4
"#;

        let config: Config = toml::from_str(raw).unwrap();

        assert_eq!(config.limits.max_tokens_per_run, 100_000);
        assert_eq!(config.limits.max_estimated_cost_usd, 5.0);
    }

    #[test]
    fn config_can_disable_budget_limits_with_zero() {
        let raw = r#"
[provider]
default = "ollama"

[models]
ceo = "qwen2.5-coder:32b"
pm = "qwen2.5-coder:32b"
tech_lead = "qwen2.5-coder:32b"
developer = "qwen2.5-coder:32b"
qa = "qwen2.5-coder:14b"
devops = "qwen2.5-coder:14b"
assistant = "qwen2.5-coder:32b"

[limits]
max_qa_iterations = 5
max_tokens_per_call = 8192
max_parallel_workers = 4
max_tokens_per_run = 0
max_estimated_cost_usd = 0.0
"#;

        let config: Config = toml::from_str(raw).unwrap();

        assert_eq!(config.limits.max_tokens_per_run, 0);
        assert_eq!(config.limits.max_estimated_cost_usd, 0.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test config::tests::default_limits_include_run_budget config::tests::old_config_without_budget_fields_uses_defaults config::tests::config_can_disable_budget_limits_with_zero`

Expected: compile errors for missing fields.

- [ ] **Step 3: Add config fields and defaults**

Update `LimitsConfig` in `src/config.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_qa_iterations: u32,
    pub max_tokens_per_call: u32,
    pub max_parallel_workers: u32,
    #[serde(default = "default_max_tokens_per_run")]
    pub max_tokens_per_run: u64,
    #[serde(default = "default_max_estimated_cost_usd")]
    pub max_estimated_cost_usd: f64,
}

fn default_max_tokens_per_run() -> u64 {
    100_000
}

fn default_max_estimated_cost_usd() -> f64 {
    5.0
}
```

Update `Config::default()` limits:

```rust
limits: LimitsConfig {
    max_qa_iterations: 5,
    max_tokens_per_call: 8192,
    max_parallel_workers: 4,
    max_tokens_per_run: default_max_tokens_per_run(),
    max_estimated_cost_usd: default_max_estimated_cost_usd(),
},
```

- [ ] **Step 4: Run config tests**

Run: `cargo test config::tests -- --nocapture`

Expected: all config tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat: add budget limits to config"
```

### Task 3: Run Report Budget Fields

**Files:**
- Modify: `src/run_report.rs`

- [ ] **Step 1: Write failing run report tests**

Add this import inside the `tests` module:

```rust
use crate::budget::{BudgetLimits, BudgetState, BudgetStatus};
```

Add tests:

```rust
#[test]
fn report_initializes_budget_fields_from_config() {
    let config = Config::default();
    let collector = RunReportCollector::new("dev", "build", &config);
    let report = collector.report();

    assert_eq!(report.metrics.max_tokens_per_run, 100_000);
    assert_eq!(report.metrics.max_estimated_cost_usd, 5.0);
    assert_eq!(report.metrics.budget_status, BudgetStatus::NotApplicable);
    assert_eq!(report.metrics.budget_exceeded_reason, None);
}

#[test]
fn collector_applies_budget_snapshot() {
    let config = Config::default();
    let mut collector = RunReportCollector::new("dev", "build", &config);
    let mut budget = BudgetState::new("openai", "gpt-4.1", BudgetLimits {
        max_tokens_per_run: 10,
        max_estimated_cost_usd: 0.0,
    });

    budget.record_tokens_total(11);
    collector.apply_budget_snapshot(&budget.snapshot());

    let metrics = &collector.report().metrics;
    assert_eq!(metrics.tokens_total, Some(11));
    assert_eq!(metrics.budget_status, BudgetStatus::Exceeded);
    assert_eq!(
        metrics.budget_exceeded_reason.as_deref(),
        Some("token budget exceeded: 11 > 10")
    );
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test run_report::tests::report_initializes_budget_fields_from_config run_report::tests::collector_applies_budget_snapshot`

Expected: compile errors for missing fields and method.

- [ ] **Step 3: Add budget fields to `RunMetrics`**

In `src/run_report.rs`, import budget types:

```rust
use crate::budget::{BudgetSnapshot, BudgetStatus};
```

Extend `RunMetrics`:

```rust
pub struct RunMetrics {
    pub duration_ms: Option<u64>,
    pub tokens_total: Option<usize>,
    pub token_chunks_total: usize,
    pub output_chars_total: usize,
    pub agent_count: usize,
    pub file_count: usize,
    pub tool_call_count: usize,
    pub max_tokens_per_run: u64,
    pub max_estimated_cost_usd: f64,
    pub budget_status: BudgetStatus,
    pub budget_exceeded_reason: Option<String>,
    pub cost_status: CostStatus,
    pub estimated_cost_usd: Option<f64>,
    pub cost_notes: String,
}
```

Initialize the new fields in `RunReportCollector::new`:

```rust
max_tokens_per_run: config.limits.max_tokens_per_run,
max_estimated_cost_usd: config.limits.max_estimated_cost_usd,
budget_status: if config.provider.default == "ollama" {
    BudgetStatus::NotApplicable
} else {
    BudgetStatus::Unknown
},
budget_exceeded_reason: None,
```

- [ ] **Step 4: Add `apply_budget_snapshot`**

Add this public method in `impl RunReportCollector`:

```rust
pub fn apply_budget_snapshot(&mut self, snapshot: &BudgetSnapshot) {
    self.report.metrics.tokens_total = snapshot.tokens_total.map(|tokens| tokens as usize);
    self.report.metrics.max_tokens_per_run = snapshot.max_tokens_per_run;
    self.report.metrics.max_estimated_cost_usd = snapshot.max_estimated_cost_usd;
    self.report.metrics.budget_status = snapshot.status;
    self.report.metrics.budget_exceeded_reason = snapshot.exceeded_reason.clone();
    self.report.metrics.estimated_cost_usd = snapshot.estimated_cost_usd;
    self.report.metrics.cost_status = match snapshot.status {
        BudgetStatus::NotApplicable => CostStatus::NotApplicable,
        BudgetStatus::Unknown => CostStatus::Unknown,
        BudgetStatus::WithinBudget | BudgetStatus::Exceeded => {
            if snapshot.estimated_cost_usd.is_some() {
                CostStatus::Estimated
            } else {
                CostStatus::Unknown
            }
        }
    };
    self.report.metrics.cost_notes = snapshot.cost_notes.clone();
}
```

In the existing `WorkflowStats` branch, keep setting `tokens_total` directly. The orchestrator will apply full budget snapshots in Task 4.

- [ ] **Step 5: Run run report tests**

Run: `cargo test run_report::tests -- --nocapture`

Expected: all run report tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/run_report.rs
git commit -m "feat: include budget state in run reports"
```

### Task 4: Orchestrator Budget Enforcement

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write failing orchestrator budget test**

Inside `src/orchestrator.rs` tests module, add imports:

```rust
use serde_json::Value;
```

Add a fake workflow and test:

```rust
struct StatsWorkflow;

#[async_trait]
impl Workflow for StatsWorkflow {
    fn name(&self) -> &'static str {
        "stats"
    }

    fn agents(&self) -> Vec<&'static str> {
        vec!["developer"]
    }

    async fn run(&self, _prompt: String, opts: RunOptions) -> Result<()> {
        let _ = opts.tx.send(TuiEvent::WorkflowStarted {
            workflow: "stats".to_string(),
            agents: vec!["developer".to_string()],
        });
        let _ = opts.tx.send(TuiEvent::WorkflowStats { tokens_total: 11 });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(())
    }
}

#[tokio::test]
async fn token_budget_exceeded_interrupts_run_and_writes_report() {
    let project_dir = std::env::temp_dir().join(format!(
        "cortex-budget-test-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = Config::default();
    config.limits.max_tokens_per_run = 10;
    config.limits.max_estimated_cost_usd = 0.0;

    let orchestrator = crate::orchestrator::Orchestrator::new(
        Box::new(StatsWorkflow),
        Arc::new(config),
    );

    orchestrator
        .run_with_project_dir(
            "budget test".to_string(),
            true,
            false,
            None,
            Some(project_dir.clone()),
        )
        .await
        .unwrap();

    let report_path = project_dir.join("cortex.run.json");
    let report: Value =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();

    assert_eq!(report["status"], "interrupted");
    assert_eq!(report["metrics"]["budget_status"], "exceeded");
    assert_eq!(
        report["metrics"]["budget_exceeded_reason"],
        "token budget exceeded: 11 > 10"
    );

    let _ = std::fs::remove_dir_all(project_dir);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test orchestrator::tests::token_budget_exceeded_interrupts_run_and_writes_report -- --nocapture`

Expected: test fails because the run succeeds and no exceeded budget status is applied.

- [ ] **Step 3: Add budget state to report tee**

Import budget types at the top of `src/orchestrator.rs`:

```rust
use crate::budget::{BudgetLimits, BudgetState, BudgetStatus};
```

Before spawning the report tee, build shared budget state:

```rust
let budget_state = Arc::new(tokio::sync::Mutex::new(BudgetState::new(
    self.config.provider.default.clone(),
    self.config.models.developer.clone(),
    BudgetLimits {
        max_tokens_per_run: self.config.limits.max_tokens_per_run,
        max_estimated_cost_usd: self.config.limits.max_estimated_cost_usd,
    },
)));
let budget_state_for_tee = Arc::clone(&budget_state);
let cancel_for_budget = self.cancel.clone();
```

Pass these into the spawned tee task. Update `handle_report_event` signature:

```rust
async fn handle_report_event(
    ev: TuiEvent,
    collector: &Arc<tokio::sync::Mutex<crate::run_report::RunReportCollector>>,
    budget_state: &Arc<tokio::sync::Mutex<BudgetState>>,
    cancel: &CancellationToken,
    log_tx: Option<&TuiSender>,
    real_tx: &TuiSender,
) {
    if let TuiEvent::WorkflowStats { tokens_total } = &ev {
        let snapshot = {
            let mut budget = budget_state.lock().await;
            budget.record_tokens_total(*tokens_total as u64);
            budget.snapshot()
        };
        collector.lock().await.apply_budget_snapshot(&snapshot);
        if snapshot.status == BudgetStatus::Exceeded {
            let _ = real_tx.send(TuiEvent::WorkflowInterrupted {
                message: snapshot
                    .exceeded_reason
                    .clone()
                    .unwrap_or_else(|| "budget exceeded".to_string()),
            });
            cancel.cancel();
        }
    }

    collector.lock().await.record_event(&ev);
    if let Some(log_tx) = log_tx {
        let _ = log_tx.send(ev.clone());
    }
    let _ = real_tx.send(ev);
}
```

Update both call sites inside the tee task to pass `&budget_state_for_tee` and `&cancel_for_budget`.

- [ ] **Step 4: Preserve final budget snapshot before report write**

Before every `finalize_run_report(...)` call, apply the latest snapshot:

```rust
let snapshot = budget_state.lock().await.snapshot();
collector.apply_budget_snapshot(&snapshot);
```

Do this in success, failed, and interrupted branches.

- [ ] **Step 5: Run orchestrator budget test**

Run: `cargo test orchestrator::tests::token_budget_exceeded_interrupts_run_and_writes_report -- --nocapture`

Expected: test passes.

- [ ] **Step 6: Run affected orchestrator tests**

Run: `cargo test orchestrator::tests -- --nocapture`

Expected: all orchestrator tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat: enforce run budget limits"
```

### Task 5: Budget Documentation

**Files:**
- Create: `docs/BUDGET_AND_TUI_SMOKE.md`
- Modify: `README.md`

- [ ] **Step 1: Create budget documentation**

Create `docs/BUDGET_AND_TUI_SMOKE.md`:

```markdown
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
```

- [ ] **Step 2: Link docs from README**

Add one bullet near the existing docs links in `README.md`:

```markdown
- [Budget limits and TUI smoke coverage](docs/BUDGET_AND_TUI_SMOKE.md) — token/cost budget behavior, run report fields, and terminal smoke-test coverage.
```

- [ ] **Step 3: Commit**

```bash
git add docs/BUDGET_AND_TUI_SMOKE.md README.md
git commit -m "docs: document budgets and tui smoke coverage"
```

### Task 6: TUI Scenario Test Helpers

**Files:**
- Modify: `src/tui/mod.rs`

- [ ] **Step 1: Add test helper skeleton and first smoke test**

Inside `src/tui/mod.rs` tests module, replace the import line with:

```rust
use super::{
    App, LogEntry, PopupState, Tui, qualify_model_string, sync_models_for_provider,
};
use crate::config::Config;
use crate::tui::events::channel;
use crate::workflows::ExecutionMode;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::sync::Arc;
use tokio::sync::RwLock;
```

Add helper functions and test-only accessors:

```rust
fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn test_app() -> App {
    App::new(Arc::new(RwLock::new(Config::default())))
}

#[cfg(test)]
impl App {
    fn set_input_for_test(&mut self, value: &str) {
        self.input_bar.input = tui_input::Input::new(value.to_string());
    }

    fn input_value_for_test(&self) -> &str {
        self.input_bar.input.value()
    }

    fn logs_contain_for_test(&self, needle: &str) -> bool {
        self.logs.iter().any(|entry| entry.message.contains(needle))
    }
}

#[tokio::test]
async fn smoke_submits_long_command_and_records_history() {
    let mut app = test_app();
    let (tx, _rx) = channel();
    let command = "/status this is a deliberately long command that should remain stable";
    app.set_input_for_test(command);

    let should_quit = Tui::handle_input(&mut app, &key(KeyCode::Enter), &tx).await;

    assert!(!should_quit);
    assert_eq!(app.input_value_for_test(), "");
    assert!(app.logs_contain_for_test(command));
}
```

- [ ] **Step 2: Run test to verify current behavior**

Run: `cargo test tui::tests::smoke_submits_long_command_and_records_history -- --nocapture`

Expected: test passes.

- [ ] **Step 3: Confirm helper scope**

Check that the helper impl is guarded by `#[cfg(test)]` and the production `App` fields remain private. The helper calls in the test should be:

```rust
app.set_input_for_test(command);
assert_eq!(app.input_value_for_test(), "");
assert!(app.logs_contain_for_test(command));
```

- [ ] **Step 4: Run first TUI smoke test**

Run: `cargo test tui::tests::smoke_submits_long_command_and_records_history -- --nocapture`

Expected: test passes.

- [ ] **Step 5: Commit helper foundation**

```bash
git add src/tui/mod.rs
git commit -m "test: add tui smoke test helpers"
```

### Task 7: TUI Smoke Scenarios

**Files:**
- Modify: `src/tui/mod.rs`
- Modify: `src/tui/widgets/status_bar.rs`

- [ ] **Step 1: Add keyboard scenario tests**

Add these tests to `src/tui/mod.rs` tests module:

```rust
#[tokio::test]
async fn smoke_navigates_command_history() {
    let mut app = test_app();
    let (tx, _rx) = channel();

    app.set_input_for_test("/status");
    Tui::handle_input(&mut app, &key(KeyCode::Enter), &tx).await;
    app.set_input_for_test("/help");
    Tui::handle_input(&mut app, &key(KeyCode::Enter), &tx).await;

    Tui::handle_input(&mut app, &key(KeyCode::Up), &tx).await;
    assert_eq!(app.input_value_for_test(), "/help");

    Tui::handle_input(&mut app, &key(KeyCode::Up), &tx).await;
    assert_eq!(app.input_value_for_test(), "/status");

    Tui::handle_input(&mut app, &key(KeyCode::Down), &tx).await;
    assert_eq!(app.input_value_for_test(), "/help");
}

#[tokio::test]
async fn smoke_cycles_execution_mode_with_shift_tab() {
    let mut app = test_app();
    let (tx, mut rx) = channel();

    assert_eq!(app.execution_mode, ExecutionMode::Normal);

    Tui::handle_input(&mut app, &modified_key(KeyCode::BackTab, KeyModifiers::SHIFT), &tx).await;

    assert_eq!(app.execution_mode, ExecutionMode::Plan);
    assert!(matches!(rx.try_recv().unwrap(), crate::tui::events::TuiEvent::ModeChanged(_)));
}

#[tokio::test]
async fn smoke_interrupt_menu_closes_with_escape() {
    let mut app = test_app();
    let (tx, _rx) = channel();

    app.popup = PopupState::InterruptMenu {
        message: "interrupted".to_string(),
        has_resume: false,
    };

    Tui::handle_input(&mut app, &key(KeyCode::Esc), &tx).await;

    assert!(matches!(app.popup, PopupState::None));
}
```

- [ ] **Step 2: Add picker scenario test**

Use the existing provider picker because it is already user-facing:

```rust
#[tokio::test]
async fn smoke_provider_picker_search_navigation_and_escape() {
    let mut app = test_app();
    let (tx, _rx) = channel();

    app.popup = PopupState::ProviderPicker(crate::tui::widgets::picker::PickerState::new(
        "Provider",
        vec![
        crate::tui::widgets::picker::PickerGroup {
            title: "Providers".to_string(),
            items: vec![
                crate::tui::widgets::picker::PickerItem {
                    id: "ollama".to_string(),
                    label: "ollama".to_string(),
                    description: Some("Local".to_string()),
                    checked: true,
                },
                crate::tui::widgets::picker::PickerItem {
                    id: "openai".to_string(),
                    label: "openai".to_string(),
                    description: Some("Remote".to_string()),
                    checked: false,
                },
            ],
        },
    ]));

    Tui::handle_input(&mut app, &key(KeyCode::Char('o')), &tx).await;
    Tui::handle_input(&mut app, &key(KeyCode::Down), &tx).await;
    Tui::handle_input(&mut app, &key(KeyCode::Esc), &tx).await;

    assert!(matches!(app.popup, PopupState::None));
}
```

- [ ] **Step 3: Add full-frame render smoke test**

Add:

```rust
#[tokio::test]
async fn smoke_renders_full_tui_frame_at_normal_and_small_sizes() {
    fn render_once(width: u16, height: u16) {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let config = Arc::new(RwLock::new(Config::default()));
        let mut app = App::new(config);
        app.logs.push(LogEntry::system("render smoke"));

        terminal
            .draw(|frame| {
                app.draw(frame);
            })
            .unwrap();
    }

    render_once(80, 24);
    render_once(40, 12);
}
```

- [ ] **Step 4: Add status bar narrow-width test**

In `src/tui/widgets/status_bar.rs`, add this new tests module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_with_tokens_at_narrow_width() {
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = StatusBarState {
            provider: "openai",
            model: "openai/gpt-4.1",
            elapsed_secs: 65,
            tokens_total: 12345,
            cwd: "/tmp/demo",
            git_info: Some("main"),
            mode: "AUTO",
        };

        terminal
            .draw(|frame| {
                StatusBarWidget { state: &state }.render(frame, frame.area());
            })
            .unwrap();
    }
}
```

- [ ] **Step 5: Run TUI smoke tests**

Run: `cargo test tui::tests::smoke_ -- --nocapture`

Expected: TUI smoke tests pass.

Run: `cargo test tui::widgets::status_bar::tests -- --nocapture`

Expected: status bar tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/tui/mod.rs src/tui/widgets/status_bar.rs
git commit -m "test: cover tui smoke scenarios"
```

### Task 8: Close Lacunes And Verify

**Files:**
- Modify: `LACUNES.md`
- Modify: `docs/BUDGET_AND_TUI_SMOKE.md` if actual implemented scenarios differ from the draft

- [ ] **Step 1: Update lacune 7 status and proof**

In `LACUNES.md`, replace lacune 7 status/proof with:

```markdown
**Statut:** Terminé
**Preuve:** Couvert par les limites `max_tokens_per_run` et `max_estimated_cost_usd`, le module budget, l'interruption propre des runs quand une limite évaluable est dépassée, les champs budget/coût dans `cortex.run.json`, les tests Rust dédiés et `docs/BUDGET_AND_TUI_SMOKE.md`.
```

- [ ] **Step 2: Update lacune 15 status and proof**

Replace lacune 15 status/proof with:

```markdown
**Statut:** Terminé
**Preuve:** Couvert par des smoke tests TUI déterministes dans `cargo test`: saisie/submit de commande, historique clavier, menu interruption, bascule de mode, picker, status bar étroite et rendu headless complet à tailles normale et réduite. Documenté dans `docs/BUDGET_AND_TUI_SMOKE.md`.
```

- [ ] **Step 3: Add lot tracking entry**

Add this dated entry under "Suivi des lots":

```markdown
- 2026-05-23 — Lot budget + TUI smoke terminé: limites de tokens/coût estimé par run, reporting budget dans `cortex.run.json`, interruption propre sur dépassement évaluable, documentation budget, et smoke tests TUI scénarisés/headless. Lacunes terminées: 7, 15.
```

- [ ] **Step 4: Run formatting and targeted tests**

Run:

```bash
cargo fmt
cargo test budget::tests -- --nocapture
cargo test config::tests -- --nocapture
cargo test run_report::tests -- --nocapture
cargo test orchestrator::tests::token_budget_exceeded_interrupts_run_and_writes_report -- --nocapture
cargo test tui::tests::smoke_ -- --nocapture
cargo test tui::widgets::status_bar::tests -- --nocapture
```

Expected: all targeted tests pass.

- [ ] **Step 5: Run broad verification**

Run:

```bash
cargo check
cargo test
```

Expected: both commands pass.

- [ ] **Step 6: Commit closure**

```bash
git add LACUNES.md docs/BUDGET_AND_TUI_SMOKE.md README.md src
git commit -m "test: close budget and tui smoke lacunes"
```

## Self-Review

- Spec coverage: budget config, report fields, enforcement, docs, TUI smoke scenarios, `LACUNES.md` closure, and verification are all mapped to tasks.
- Red-flag scan: no marker text or unspecified test steps remain.
- Type consistency: `BudgetStatus`, `BudgetLimits`, `BudgetSnapshot`, `BudgetState`, and `apply_budget_snapshot` are defined before use in later tasks.
- Parallel safety: budget tasks touch `budget/config/run_report/orchestrator`; TUI tasks touch `tui/mod.rs` and `status_bar.rs`; docs closure is last.
