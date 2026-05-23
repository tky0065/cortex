# Concurrency Cancellation Stress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close lacune 23 by adding deterministic Rust stress coverage for orchestration cancellation, concurrent event flow, dropped receivers, worker failures, and readable failure/interruption artifacts.

**Architecture:** Keep the first implementation inside the existing `#[cfg(test)] mod tests` in `src/orchestrator.rs`, because that module already owns fake workflows, `RunOptions`, event channel tests, run-report finalization, and checkpoint assertions. Add small fake workflow structs and JSON helper functions only for tests; change production code only if a new test exposes a real bug.

**Tech Stack:** Rust, Tokio, `tokio_util::sync::CancellationToken`, `async_trait`, `anyhow`, existing `serde_json`, existing `uuid`, existing `crate::tui::events` channel, existing `RunReportCollector`.

---

## File Structure

- Modify: `src/orchestrator.rs`
  - Add test helpers inside the existing `#[cfg(test)] mod tests`.
  - Add fake workflows: `SlowUntilCancelledWorkflow`, `FailingWorkflow`, `DroppedReceiverWorkflow`, `ParallelWorkerFailureWorkflow`, `ParallelEventBurstWorkflow`, `FileThenCancelWorkflow`.
  - Add helpers: `temp_test_dir(prefix)`, `read_run_report_status(dir)`, `read_run_report_json(dir)`, `drain_events_until_closed(rx)`.
  - Add six deterministic `#[tokio::test]` cases matching the design spec.
- Modify: `LACUNES.md`
  - Mark lacune 23 as `Terminé`.
  - Replace proof with the concrete orchestrator stress test coverage.
  - Add a dated lot entry in "Suivi des lots".

---

### Task 1: Add Test Helpers For Temporary Dirs, Report Parsing, And Event Draining

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Add failing helper-usage test**

Add this test near the existing orchestrator tests, before the current `CancelThenOkWorkflow` struct:

```rust
    #[tokio::test]
    async fn stress_helpers_create_isolated_project_dir_and_parse_report_status() {
        let dir = temp_test_dir("cortex_stress_helper");
        let config = Config::default();
        let mut collector = crate::run_report::RunReportCollector::new("dev", "build", &config);
        finalize_run_report(&mut collector, &dir, &config, RunReportOutcome::Interrupted("stop".into()));

        assert_eq!(read_run_report_status(&dir), "interrupted");

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test stress_helpers_create_isolated_project_dir_and_parse_report_status
```

Expected: FAIL with unresolved functions `temp_test_dir` and `read_run_report_status`.

- [ ] **Step 3: Add helper implementations**

Add these helpers near `record_test_event` in `src/orchestrator.rs`:

```rust
    fn temp_test_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{}_{}", prefix, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read_run_report_json(dir: &std::path::Path) -> serde_json::Value {
        let content = std::fs::read_to_string(dir.join("cortex.run.json")).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn read_run_report_status(dir: &std::path::Path) -> String {
        read_run_report_json(dir)["status"].as_str().unwrap().to_string()
    }

    async fn drain_events_until_closed(
        mut rx: crate::tui::events::TuiReceiver,
    ) -> Vec<TuiEvent> {
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        events
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test stress_helpers_create_isolated_project_dir_and_parse_report_status
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: add orchestrator stress helpers"
```

---

### Task 2: Cover Cancellation During A Slow In-Flight Workflow

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add this test after the helper test:

```rust
    #[tokio::test]
    async fn orchestrator_cancellation_interrupts_slow_workflow() {
        let dir = temp_test_dir("cortex_cancel_slow_workflow");
        let in_flight = Arc::new(tokio::sync::Notify::new());
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(
            Box::new(SlowUntilCancelledWorkflow {
                in_flight: Arc::clone(&in_flight),
            }),
            config,
        );
        let cancel = orch.cancel_token();

        let run = tokio::spawn({
            let dir = dir.clone();
            async move {
                orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir))
                    .await
            }
        });

        tokio::time::timeout(std::time::Duration::from_secs(1), in_flight.notified())
            .await
            .expect("workflow did not start");
        cancel.cancel();

        let result = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .expect("orchestrator deadlocked after cancellation")
            .expect("run task panicked");

        result.unwrap();
        assert_eq!(read_run_report_status(&dir), "interrupted");
        let report = read_run_report_json(&dir);
        assert_eq!(report["failure"]["failure_type"], "interrupted");

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test orchestrator_cancellation_interrupts_slow_workflow
```

Expected: FAIL with unresolved type `SlowUntilCancelledWorkflow`.

- [ ] **Step 3: Add the fake workflow**

Add this struct and impl near `CancelThenOkWorkflow`:

```rust
    struct SlowUntilCancelledWorkflow {
        in_flight: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl Workflow for SlowUntilCancelledWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "slow cancellation test workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            options.tx.send(TuiEvent::AgentStarted {
                agent: "slow".to_string(),
            }).ok();
            self.in_flight.notify_waiters();
            options.cancel.cancelled().await;
            options.tx.send(TuiEvent::WorkflowInterrupted {
                message: "slow workflow observed cancellation".to_string(),
            }).ok();
            Ok(())
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test orchestrator_cancellation_interrupts_slow_workflow
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: cover slow workflow cancellation"
```

---

### Task 3: Cover Workflow Failure Without Event Deadlock

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add this test:

```rust
    #[tokio::test]
    async fn orchestrator_failure_does_not_deadlock_event_stream() {
        let dir = temp_test_dir("cortex_failure_event_stream");
        let (tx, rx) = channel();
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(FailingWorkflow), config);

        let run = orch.run_with_project_dir(
            "build".to_string(),
            true,
            false,
            Some(tx),
            Some(dir.clone()),
        );

        let err = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .expect("orchestrator deadlocked on workflow failure")
            .unwrap_err()
            .to_string();
        assert!(err.contains("intentional workflow failure"));

        let events = tokio::time::timeout(std::time::Duration::from_secs(1), drain_events_until_closed(rx))
            .await
            .expect("event stream did not close after failure");
        assert!(events.iter().any(|event| matches!(event, TuiEvent::Error { agent, message } if agent == "failing" && message.contains("intentional workflow failure"))));
        assert_eq!(read_run_report_status(&dir), "failed");
        assert_eq!(read_run_report_json(&dir)["failure"]["failure_type"], "agent_error");

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test orchestrator_failure_does_not_deadlock_event_stream
```

Expected: FAIL with unresolved type `FailingWorkflow`.

- [ ] **Step 3: Add the fake workflow**

Add:

```rust
    struct FailingWorkflow;

    #[async_trait]
    impl Workflow for FailingWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "failing test workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            options.tx.send(TuiEvent::AgentStarted {
                agent: "failing".to_string(),
            }).ok();
            options.tx.send(TuiEvent::Error {
                agent: "failing".to_string(),
                message: "intentional workflow failure".to_string(),
            }).ok();
            anyhow::bail!("intentional workflow failure")
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test orchestrator_failure_does_not_deadlock_event_stream
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: cover workflow failure event drain"
```

---

### Task 4: Cover Dropped Event Receiver

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
    #[tokio::test]
    async fn orchestrator_survives_dropped_event_receiver() {
        let dir = temp_test_dir("cortex_dropped_receiver");
        let (tx, rx) = channel();
        drop(rx);

        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(DroppedReceiverWorkflow), config);

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir("build".to_string(), true, false, Some(tx), Some(dir.clone())),
        )
        .await
        .expect("orchestrator deadlocked when event receiver was dropped");

        result.unwrap();
        assert_eq!(read_run_report_status(&dir), "success");

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test orchestrator_survives_dropped_event_receiver
```

Expected: FAIL with unresolved type `DroppedReceiverWorkflow`.

- [ ] **Step 3: Add the fake workflow**

Add:

```rust
    struct DroppedReceiverWorkflow;

    #[async_trait]
    impl Workflow for DroppedReceiverWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "dropped receiver workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            for i in 0..25 {
                options.tx.send(TuiEvent::TokenChunk {
                    agent: "dropped_receiver".to_string(),
                    chunk: format!("chunk-{i}"),
                }).ok();
            }
            options.tx.send(TuiEvent::AgentDone {
                agent: "dropped_receiver".to_string(),
            }).ok();
            Ok(())
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test orchestrator_survives_dropped_event_receiver
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: cover dropped event receiver"
```

---

### Task 5: Cover Parallel Worker Failure And Sibling Join

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
    #[tokio::test]
    async fn parallel_worker_failure_cancels_or_joins_siblings() {
        let dir = temp_test_dir("cortex_parallel_worker_failure");
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(ParallelWorkerFailureWorkflow), config);

        let err = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir.clone())),
        )
        .await
        .expect("parallel workflow deadlocked after worker failure")
        .unwrap_err()
        .to_string();

        assert!(err.contains("worker 2 failed"));
        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "failed");
        assert!(report["metrics"]["agent_count"].as_u64().unwrap() >= 3);

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test parallel_worker_failure_cancels_or_joins_siblings
```

Expected: FAIL with unresolved type `ParallelWorkerFailureWorkflow`.

- [ ] **Step 3: Add the fake workflow**

Add:

```rust
    struct ParallelWorkerFailureWorkflow;

    #[async_trait]
    impl Workflow for ParallelWorkerFailureWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "parallel worker failure workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            let mut handles = Vec::new();
            for worker_id in 0..4 {
                let tx = options.tx.clone();
                handles.push(tokio::spawn(async move {
                    let agent = format!("worker-{worker_id}");
                    tx.send(TuiEvent::AgentStarted { agent: agent.clone() }).ok();
                    tx.send(TuiEvent::TokenChunk {
                        agent: agent.clone(),
                        chunk: format!("worker {worker_id} started"),
                    }).ok();
                    if worker_id == 2 {
                        tx.send(TuiEvent::Error {
                            agent,
                            message: "worker 2 failed".to_string(),
                        }).ok();
                        anyhow::bail!("worker 2 failed");
                    }
                    tx.send(TuiEvent::AgentDone { agent }).ok();
                    Ok::<(), anyhow::Error>(())
                }));
            }

            let mut failure = None;
            for handle in handles {
                match handle.await {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => failure = Some(err),
                    Err(err) => failure = Some(anyhow::anyhow!("worker join failed: {err}")),
                }
            }

            if let Some(err) = failure {
                Err(err)
            } else {
                Ok(())
            }
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test parallel_worker_failure_cancels_or_joins_siblings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: cover parallel worker failure"
```

---

### Task 6: Cover Parallel Event Burst Final State

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
    #[tokio::test]
    async fn parallel_event_burst_preserves_final_state() {
        let dir = temp_test_dir("cortex_parallel_event_burst");
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(ParallelEventBurstWorkflow), config);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir.clone())),
        )
        .await
        .expect("parallel event burst deadlocked")
        .unwrap();

        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "success");
        assert_eq!(report["metrics"]["token_chunks_total"], 100);
        assert!(report["metrics"]["output_chars_total"].as_u64().unwrap() > 0);

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test parallel_event_burst_preserves_final_state
```

Expected: FAIL with unresolved type `ParallelEventBurstWorkflow`.

- [ ] **Step 3: Add the fake workflow**

Add:

```rust
    struct ParallelEventBurstWorkflow;

    #[async_trait]
    impl Workflow for ParallelEventBurstWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "parallel event burst workflow"
        }

        async fn run(&self, _prompt: String, options: RunOptions) -> Result<()> {
            let mut handles = Vec::new();
            for worker_id in 0..10 {
                let tx = options.tx.clone();
                handles.push(tokio::spawn(async move {
                    let agent = format!("burst-{worker_id}");
                    tx.send(TuiEvent::AgentStarted { agent: agent.clone() }).ok();
                    for chunk_id in 0..10 {
                        tx.send(TuiEvent::TokenChunk {
                            agent: agent.clone(),
                            chunk: format!("worker={worker_id} chunk={chunk_id}"),
                        }).ok();
                    }
                    tx.send(TuiEvent::AgentDone { agent }).ok();
                }));
            }

            for handle in handles {
                handle.await.expect("burst worker panicked");
            }
            Ok(())
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test parallel_event_burst_preserves_final_state
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: cover parallel event burst reporting"
```

---

### Task 7: Cover Cancelled Run Artifacts Remain Readable

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add:

```rust
    #[tokio::test]
    async fn cancelled_run_artifacts_remain_readable() {
        let dir = temp_test_dir("cortex_cancelled_artifacts");
        let config = Arc::new(Config::default());
        let orch = super::Orchestrator::new(Box::new(FileThenCancelWorkflow), config);

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            orch.run_with_project_dir("build".to_string(), true, false, None, Some(dir.clone())),
        )
        .await
        .expect("cancelled artifact workflow deadlocked")
        .unwrap();

        let report = read_run_report_json(&dir);
        assert_eq!(report["status"], "interrupted");
        assert_eq!(report["files"][0]["path"], "partial.txt");

        let checkpoint = crate::checkpoint::Checkpoint::load(&dir).unwrap();
        assert_eq!(
            checkpoint.status,
            crate::checkpoint::CheckpointStatus::Interrupted
        );

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test cancelled_run_artifacts_remain_readable
```

Expected: FAIL with unresolved type `FileThenCancelWorkflow`.

- [ ] **Step 3: Add the fake workflow**

Add:

```rust
    struct FileThenCancelWorkflow;

    #[async_trait]
    impl Workflow for FileThenCancelWorkflow {
        fn name(&self) -> &str {
            "dev"
        }

        fn description(&self) -> &str {
            "file then cancel workflow"
        }

        async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
            let checkpoint =
                crate::checkpoint::Checkpoint::new("run-artifact", self.name(), prompt, &options.config);
            checkpoint.write_to(&options.project_dir, &options.config)?;
            options.tx.send(TuiEvent::FileWritten {
                agent: "artifact".to_string(),
                path: "partial.txt".to_string(),
                old_content: None,
                new_content: "partial content".to_string(),
            }).ok();
            options.cancel.cancel();
            Ok(())
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test cancelled_run_artifacts_remain_readable
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs
git commit -m "test: cover cancelled run artifacts"
```

---

### Task 8: Update LACUNES.md For Lacune 23

**Files:**
- Modify: `LACUNES.md`

- [ ] **Step 1: Update lacune 23 status and proof**

Replace the lacune 23 status/proof block:

```markdown
### 23. Controle de concurrence et annulation a tester sous charge
**Statut:** Terminé
**Preuve:** Couvert par les tests de stress orchestrateur dans `src/orchestrator.rs`: annulation d'un workflow lent, échec workflow sans deadlock event stream, receiver TUI fermé, échec worker parallèle, rafale d'événements concurrents et artefacts lisibles après annulation.
```

Keep the existing `Constat`, `Pourquoi c'est important`, and `Action recommandee` paragraphs unless implementation reveals a more precise wording.

- [ ] **Step 2: Add dated lot entry**

Append this line in "Suivi des lots":

```markdown
- 2026-05-23 — Lot concurrence/annulation terminé: tests de stress orchestrateur pour annulation, échec, receivers fermés, workers parallèles, rafales d'événements et lisibilité des artefacts après interruption. Lacune terminée: 23.
```

- [ ] **Step 3: Verify the lacune tracker**

Run:

```bash
rg -n "23\\. Controle|Statut: À faire|Statut: En cours|concurrence/annulation" LACUNES.md
```

Expected: lacune 23 shows `Terminé`; remaining open statuses should only be unrelated lacunes such as 7 and 15.

- [ ] **Step 4: Commit**

```bash
git add LACUNES.md
git commit -m "docs: mark concurrency stress coverage complete"
```

---

### Task 9: Final Verification

**Files:**
- Verify: `src/orchestrator.rs`
- Verify: `LACUNES.md`

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: no output on success.

- [ ] **Step 2: Run targeted stress tests**

Run:

```bash
cargo test orchestrator_cancellation_interrupts_slow_workflow orchestrator_failure_does_not_deadlock_event_stream orchestrator_survives_dropped_event_receiver parallel_worker_failure_cancels_or_joins_siblings parallel_event_burst_preserves_final_state cancelled_run_artifacts_remain_readable
```

Expected: this may fail because Cargo test filtering accepts one filter string. If it fails with usage/filter behavior, run the six tests individually:

```bash
cargo test orchestrator_cancellation_interrupts_slow_workflow
cargo test orchestrator_failure_does_not_deadlock_event_stream
cargo test orchestrator_survives_dropped_event_receiver
cargo test parallel_worker_failure_cancels_or_joins_siblings
cargo test parallel_event_burst_preserves_final_state
cargo test cancelled_run_artifacts_remain_readable
```

Expected: all targeted tests PASS.

- [ ] **Step 3: Run orchestrator test module**

Run:

```bash
cargo test orchestrator
```

Expected: all orchestrator tests PASS.

- [ ] **Step 4: Run broad checks**

Run:

```bash
cargo check
cargo test
```

Expected: both PASS.

- [ ] **Step 5: Inspect git status**

Run:

```bash
git status --short
```

Expected: only unrelated pre-existing untracked files may remain, such as `.DS_Store`, `.claude/`, and `.idea/`.

- [ ] **Step 6: Commit final formatting if needed**

If `cargo fmt` changed files after earlier commits:

```bash
git add src/orchestrator.rs LACUNES.md
git commit -m "style: format concurrency stress coverage"
```

Expected: commit only if there are formatting changes.

---

## Self-Review

- Spec coverage: the plan covers cancellation during slow workflow, workflow failure, dropped receiver, parallel worker failure, parallel event burst, readable interrupted artifacts, and `LACUNES.md` tracking.
- Placeholder scan: no `TBD`, `TODO`, "similar to", or unspecified "add tests" steps remain.
- Type consistency: helper names and fake workflow names are introduced before later use or in the same task that needs them; all tests use existing `Orchestrator::run_with_project_dir`, `TuiEvent`, `RunOptions`, `Config`, `channel`, and `Workflow` APIs.
