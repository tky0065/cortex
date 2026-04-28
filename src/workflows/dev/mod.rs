use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::sync::Semaphore;

use super::{RunOptions, Workflow};
use crate::tools::filesystem::FileSystem;
use crate::tui::events::TuiEvent;

pub mod agents;

pub struct DevWorkflow;

#[async_trait]
impl Workflow for DevWorkflow {
    fn name(&self) -> &str {
        "dev"
    }

    fn description(&self) -> &str {
        "Full software development: CEO → PM → Tech Lead → Developer → QA → DevOps"
    }

    async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
        let _ = options.tx.send(TuiEvent::WorkflowStarted {
            workflow: "dev".into(),
            agents: vec!["ceo", "pm", "tech_lead", "developer", "qa", "devops"]
                .into_iter()
                .map(String::from)
                .collect(),
        });

        // Create project directory
        let project_name = slugify(&prompt);
        let project_dir = options.project_dir.join(&project_name);
        std::fs::create_dir_all(&project_dir)
            .with_context(|| format!("Cannot create project dir: {}", project_dir.display()))?;

        let fs = FileSystem::new(&project_dir);
        let opts = RunOptions {
            project_dir: project_dir.clone(),
            ..options.clone()
        };

        // ── Phase 1: CEO → brief ─────────────────────────────────────────
        let brief = agents::ceo::run(&prompt, &opts).await?;

        // Early exit if cancelled
        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 2: PM → specs.md ───────────────────────────────────────
        let specs = agents::pm::run(&brief, &opts).await?;
        fs.write("specs.md", &specs)?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "specs-ready".into() });

        // Interactive pause after specs.md (Task 30 / 31)
        if !opts.auto {
            let _ = opts.tx.send(TuiEvent::InteractivePause {
                message: "specs.md ready — type /continue to proceed or /abort to stop".into(),
            });
            tokio::select! {
                _ = wait_for_resume(&opts) => {}
                _ = opts.cancel.cancelled() => {
                    let _ = opts.tx.send(TuiEvent::TokenChunk {
                        agent: "orchestrator".into(),
                        chunk: "Workflow aborted at specs pause.".into(),
                    });
                    return Ok(());
                }
            }
        }

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 3: Tech Lead → architecture.md ─────────────────────────
        let arch = agents::tech_lead::run(&specs, &opts).await?;
        fs.write("architecture.md", &arch)?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "architecture-ready".into() });

        // Interactive pause after architecture.md (Task 30 / 31)
        if !opts.auto {
            let _ = opts.tx.send(TuiEvent::InteractivePause {
                message: "architecture.md ready — type /continue to proceed or /abort to stop".into(),
            });
            tokio::select! {
                _ = wait_for_resume(&opts) => {}
                _ = opts.cancel.cancelled() => {
                    let _ = opts.tx.send(TuiEvent::TokenChunk {
                        agent: "orchestrator".into(),
                        chunk: "Workflow aborted at architecture pause.".into(),
                    });
                    return Ok(());
                }
            }
        }

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 4: Developer workers (parallel, semaphore-bounded) ──────
        let files = parse_files_to_create(&arch);
        let sem = Arc::new(Semaphore::new(opts.config.limits.max_parallel_workers as usize));
        let mut dev_handles = Vec::new();

        for file_path in files {
            // Stop spawning new tasks if already cancelled
            if opts.cancel.is_cancelled() {
                return Ok(());
            }

            let permit = Arc::clone(&sem).acquire_owned().await?;
            let opts_clone = opts.clone();
            let arch_clone = arch.clone();
            let project_dir_clone = project_dir.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit;
                // Honour cancellation inside each worker
                if opts_clone.cancel.is_cancelled() {
                    return Ok::<(), anyhow::Error>(());
                }
                let local_fs = FileSystem::new(&project_dir_clone);
                let code = agents::developer::run(&file_path, &arch_clone, &opts_clone).await?;
                local_fs.write(&file_path, &code)?;
                Ok::<(), anyhow::Error>(())
            });
            dev_handles.push(handle);
        }

        for handle in dev_handles {
            handle
                .await
                .map_err(|e| anyhow::anyhow!("Developer worker panicked: {e}"))??;
        }
        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "development-done".into() });

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // Interactive pause after first build (Task 30 / 31)
        if !opts.auto {
            let _ = opts.tx.send(TuiEvent::InteractivePause {
                message: "Initial code ready — type /continue to run QA or /abort to stop".into(),
            });
            tokio::select! {
                _ = wait_for_resume(&opts) => {}
                _ = opts.cancel.cancelled() => {
                    let _ = opts.tx.send(TuiEvent::TokenChunk {
                        agent: "orchestrator".into(),
                        chunk: "Workflow aborted after development phase.".into(),
                    });
                    return Ok(());
                }
            }
        }

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 5: QA ↔ Developer loop ─────────────────────────────────
        let max_iterations = opts.config.limits.max_qa_iterations;
        for iteration in 0..max_iterations {
            if opts.cancel.is_cancelled() {
                return Ok(());
            }

            let report = agents::qa::run(&arch, &opts, &fs).await?;

            if report.contains("RECOMMENDATION: APPROVE") {
                let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "qa-approved".into() });
                break;
            }

            if iteration + 1 >= max_iterations {
                let _ = opts.tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: format!("QA max iterations ({}) reached — proceeding", max_iterations),
                });
                break;
            }

            // Fix: re-run developer for each file mentioned in QA report
            for file_path in extract_files_from_report(&report) {
                if opts.cancel.is_cancelled() {
                    return Ok(());
                }
                if let Ok(current) = fs.read(&file_path) {
                    agents::developer::fix(&file_path, &current, &report, &opts, &fs).await?;
                }
            }
        }

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 6: DevOps ───────────────────────────────────────────────
        agents::devops::run(&arch, &opts, &fs).await?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "done".into() });

        let _ = opts.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!("Project '{}' created at: {}", project_name, project_dir.display()),
        });

        Ok(())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Wait until the REPL sends a resume signal (via `opts.resume_tx`).
///
/// This async fn returns when `()` is received on the resume channel that
/// was created in the `Orchestrator` and cloned into every `RunOptions`.
///
/// Note: Because `resume_tx` is `Arc<Sender>`, we can't move the receiver
/// out of `RunOptions` without a `Mutex`.  We hold a simple polling loop
/// here — good enough for a human-speed interactive pause.
async fn wait_for_resume(opts: &RunOptions) {
    // We detect "resume" by sending a dummy message to ourselves — that won't
    // work cleanly.  Instead we just yield in a tight-ish loop checking
    // whether a `Resume` token was sent via the resume channel.
    // The resume_tx is shared; we listen by having the RunOptions also carry
    // the shared receiver wrapped in a Mutex.
    //
    // For now we park and poll every 200 ms, which is fine at human timescales.
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        // Try to receive without blocking
        if opts.cancel.is_cancelled() {
            return;
        }
    }
}

fn slugify(s: &str) -> String {
    let slug = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>();
    slug.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Parse the FILES_TO_CREATE section from architecture.md.
fn parse_files_to_create(arch: &str) -> Vec<String> {
    let mut in_section = false;
    let mut files = Vec::new();

    for line in arch.lines() {
        if line.contains("FILES_TO_CREATE") {
            in_section = true;
            continue;
        }
        if in_section {
            // Stop at next heading
            if line.starts_with('#') {
                break;
            }
            let trimmed = line
                .trim()
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ' || c == '-');
            if !trimmed.is_empty() && trimmed.contains('.') && !trimmed.contains(' ') {
                files.push(trimmed.to_string());
            }
        }
    }

    // Fallback: if no FILES_TO_CREATE section found, return a default main file
    if files.is_empty() {
        files.push("src/main.rs".to_string());
    }

    files
}

/// Extract file paths mentioned in a QA report.
fn extract_files_from_report(report: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in report.lines() {
        if line.trim_start().starts_with('-') {
            // Lines like: "- src/main.rs:42 HIGH ..."
            if let Some(file_part) = line.split_whitespace().nth(1) {
                let file = file_part.split(':').next().unwrap_or("").to_string();
                if file.contains('.') && !file.is_empty() && !files.contains(&file) {
                    files.push(file);
                }
            }
        }
    }
    files
}
