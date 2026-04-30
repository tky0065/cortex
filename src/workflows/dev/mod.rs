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

        // Use the launch directory as the project sandbox.
        let project_dir = options.project_dir.clone();
        std::fs::create_dir_all(&project_dir)
            .with_context(|| format!("Cannot create project dir: {}", project_dir.display()))?;

        let fs = FileSystem::new(&project_dir);
        let opts = RunOptions {
            project_dir: project_dir.clone(),
            ..options.clone()
        };

        // ── Phase 1: CEO → brief ─────────────────────────────────────────
        // The CEO may output `CLARIFICATION_NEEDED: <question>` when the prompt
        // is genuinely ambiguous. We ask the user once, then re-run CEO with
        // the enriched context. For clear prompts CEO proceeds directly.
        let brief = {
            let first = agents::ceo::run(&prompt, &opts).await?;
            if let Some(question) = parse_clarification_needed(&first) {
                let answer = ask_user("ceo", &question, &opts).await?;
                if answer.trim().is_empty() {
                    first
                } else {
                    let enriched = format!("{}\n\nAdditional context: {}", prompt, answer.trim());
                    agents::ceo::run(&enriched, &opts).await?
                }
            } else {
                first
            }
        };

        // Early exit if cancelled
        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 2: PM → specs.md ───────────────────────────────────────
        let specs = agents::pm::run(&brief, &opts).await?;
        fs.write("specs.md", &specs)?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "specs-ready".into(),
        });

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 3: Tech Lead → architecture.md ─────────────────────────
        let arch = agents::tech_lead::run(&specs, &opts).await?;
        fs.write("architecture.md", &arch)?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "architecture-ready".into(),
        });

        if opts.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 4: Developer workers (parallel, semaphore-bounded) ──────
        let files = parse_files_to_create(&arch);
        let sem = Arc::new(Semaphore::new(
            opts.config.limits.max_parallel_workers as usize,
        ));
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
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "development-done".into(),
        });

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
                let _ = opts.tx.send(TuiEvent::PhaseComplete {
                    phase: "qa-approved".into(),
                });
                break;
            }

            if iteration + 1 >= max_iterations {
                let _ = opts.tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: format!(
                        "QA max iterations ({}) reached — proceeding",
                        max_iterations
                    ),
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
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "done".into(),
        });

        let _ = opts.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!("Project created at: {}", project_dir.display()),
        });

        Ok(())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Ask the user a question and wait for their text answer.
///
/// Emits `TuiEvent::UserQuestion` so the TUI can show a text-input popup.
/// In `--auto` mode the question is skipped and an empty string is returned.
pub async fn ask_user(agent: &str, question: &str, opts: &RunOptions) -> Result<String> {
    if opts.auto {
        return Ok(String::new());
    }

    let _ = opts.tx.send(TuiEvent::UserQuestion {
        agent: agent.to_string(),
        question: question.to_string(),
    });

    tokio::select! {
        answer = async {
            let mut rx = opts.answer_rx.lock().await;
            rx.recv().await.unwrap_or_default()
        } => Ok(answer),
        _ = opts.cancel.cancelled() => Ok(String::new()),
    }
}

/// If the CEO output is a clarification request, extract the question text.
/// Returns `Some(question)` when the entire output is `CLARIFICATION_NEEDED: <question>`,
/// `None` otherwise (i.e. CEO produced a normal brief).
fn parse_clarification_needed(output: &str) -> Option<String> {
    let trimmed = output.trim();
    trimmed
        .strip_prefix("CLARIFICATION_NEEDED:")
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
}

/// Parse the FILES_TO_CREATE section from architecture.md.
///
/// Handles multiple LLM output styles:
///   `1. main.go`
///   `1. **main.go** – entry point`
///   `- `cmd/hello/main.go``
///   `1. main.go  # comment`
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

            // Strip leading list markers: `1.`, `-`, `*`, digits, spaces
            let stripped = line.trim().trim_start_matches(|c: char| {
                c.is_ascii_digit() || c == '.' || c == ' ' || c == '-' || c == '*'
            });

            if stripped.is_empty() {
                continue;
            }

            // Strip markdown bold/italic (`**`, `__`, `*`, backticks)
            let no_md = stripped
                .trim_start_matches('`')
                .trim_start_matches("**")
                .trim_start_matches('*')
                .trim_start_matches("__");

            // Take only the file path — stop at first space, ` –`, ` -`, ` #`, or `:`
            // (handles "main.go – entry point", "main.go: does X", "main.go # comment")
            let path_candidate = no_md
                .split([' ', '\t', '#', ':'])
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| c == '`' || c == '*' || c == '_' || c == '.');

            // Must look like a file path: non-empty, contains a dot or a slash, no forbidden chars
            let is_valid = !path_candidate.is_empty()
                && (path_candidate.contains('.') || path_candidate.contains('/'))
                && !path_candidate.contains('*')
                && !path_candidate.contains('"')
                && !path_candidate.contains('(');

            if is_valid {
                files.push(path_candidate.to_string());
            }
        }
    }

    // Fallback: could not parse FILES_TO_CREATE — use a generic entry point
    if files.is_empty() {
        files.push("main.go".to_string());
    }

    files
}

/// Extract file paths mentioned in a QA report.
fn extract_files_from_report(report: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in report.lines() {
        if line.trim_start().starts_with('-') {
            // Lines like: "- src/main.rs:42 HIGH ..." or "- `src/main.rs`:42 HIGH ..."
            if let Some(file_part) = line.split_whitespace().nth(1) {
                let file = file_part
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .trim_matches('`')
                    .to_string();
                if file.contains('.') && !file.is_empty() && !files.contains(&file) {
                    files.push(file);
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::{parse_clarification_needed, parse_files_to_create};

    // ── parse_clarification_needed ────────────────────────────────────────

    #[test]
    fn detects_clarification_marker() {
        let out = "CLARIFICATION_NEEDED: What programming language should be used?";
        assert_eq!(
            parse_clarification_needed(out),
            Some("What programming language should be used?".into())
        );
    }

    #[test]
    fn ignores_normal_brief() {
        let out = "## Overview\nA Go hello-world CLI tool.\n## Target Users\n...";
        assert_eq!(parse_clarification_needed(out), None);
    }

    #[test]
    fn ignores_partial_match_inside_text() {
        let out = "## Overview\nCLARIFICATION_NEEDED: something buried in a paragraph";
        assert_eq!(parse_clarification_needed(out), None);
    }

    // ── parse_files_to_create ─────────────────────────────────────────────

    #[test]
    fn parses_plain_list() {
        let arch = "## FILES_TO_CREATE\n1. go.mod\n2. main.go\n";
        let files = parse_files_to_create(arch);
        assert_eq!(files, vec!["go.mod", "main.go"]);
    }

    #[test]
    fn parses_bold_with_description() {
        // Format LLMs often produce: `**main.go** – entry point`
        let arch =
            "## FILES_TO_CREATE\n1. **go.mod** – module file\n2. **main.go** – entry point\n";
        let files = parse_files_to_create(arch);
        assert_eq!(files, vec!["go.mod", "main.go"]);
    }

    #[test]
    fn parses_backtick_paths() {
        let arch = "## FILES_TO_CREATE\n- `cmd/hello/main.go`\n- `go.mod`\n";
        let files = parse_files_to_create(arch);
        assert_eq!(files, vec!["cmd/hello/main.go", "go.mod"]);
    }

    #[test]
    fn stops_at_next_heading() {
        let arch = "## FILES_TO_CREATE\n1. main.go\n## Key Dependencies\n2. should-be-ignored.go\n";
        let files = parse_files_to_create(arch);
        assert_eq!(files, vec!["main.go"]);
    }

    #[test]
    fn fallback_when_section_missing() {
        let arch = "## Technology Stack\nGo 1.22\n";
        let files = parse_files_to_create(arch);
        assert_eq!(files, vec!["main.go"]);
    }
}
