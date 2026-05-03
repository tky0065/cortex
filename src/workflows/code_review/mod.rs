use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{RunOptions, Workflow, send_phase_tasks};
use crate::tools::filesystem::FileSystem;
use crate::tui::events::TuiEvent;

pub mod agents;

pub struct CodeReviewWorkflow;

const CODE_REVIEW_TASKS: &[&str] = &[
    "Scanner les fichiers source",
    "Produire les notes de revue generale",
    "Auditer la securite",
    "Analyser les performances",
    "Rediger le rapport final",
];

#[async_trait]
impl Workflow for CodeReviewWorkflow {
    fn name(&self) -> &str {
        "code-review"
    }

    fn description(&self) -> &str {
        "Code audit: Reviewer → Security ‖ Performance → Reporter"
    }

    async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
        let _ = options.tx.send(TuiEvent::WorkflowStarted {
            workflow: "code-review".into(),
            agents: vec!["reviewer", "security", "performance", "reporter"]
                .into_iter()
                .map(String::from)
                .collect(),
        });

        // Always scan the current working directory; prompt is a focus hint for agents
        let target_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        let focus_hint = {
            let p = prompt.trim();
            if p.is_empty() || p == "." {
                String::new()
            } else {
                format!(
                    "\n\n## Review Focus\n\nThe user specifically asked: {}\nPlease pay particular attention to this aspect in your analysis.\n",
                    p
                )
            }
        };

        // Create output directory for reports
        let output_dir = options.project_dir.join("code-review-output");
        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Cannot create output dir: {}", output_dir.display()))?;
        let fs = FileSystem::new(&output_dir);
        send_phase_tasks(&options, CODE_REVIEW_TASKS, 0);

        // Collect source files
        let _ = options.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!("Scanning source files in: {}", target_dir.display()),
        });
        let source_content = collect_source_files(&target_dir);
        send_phase_tasks(&options, CODE_REVIEW_TASKS, 1);

        if source_content.trim().is_empty() {
            let _ = options.tx.send(TuiEvent::TokenChunk {
                agent: "orchestrator".into(),
                chunk: "No source files found. Aborting review.".into(),
            });
            return Ok(());
        }

        // Build full agent input: directory context + optional focus hint + source files
        let agent_input = format!(
            "# Project Directory: {}\n{}\n{}",
            target_dir.display(),
            focus_hint,
            source_content
        );

        // ── Phase 1: Reviewer → review_notes.md ──────────────────────────
        let review_notes = agents::reviewer::run(&agent_input, &options).await?;
        fs.write("review_notes.md", &review_notes)?;
        send_phase_tasks(&options, CODE_REVIEW_TASKS, 2);
        let _ = options.tx.send(TuiEvent::PhaseComplete {
            phase: "review-done".into(),
        });

        if options.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 2: Security ‖ Performance (parallel) ────────────────────
        let opts2 = options.clone();
        let agent_input_clone = agent_input.clone();
        let (security_result, performance_result) = tokio::join!(
            agents::security::run(&agent_input, &options),
            agents::performance::run(&agent_input_clone, &opts2),
        );
        let security_report = security_result?;
        let performance_report = performance_result?;
        send_phase_tasks(&options, CODE_REVIEW_TASKS, 4);

        let _ = options.tx.send(TuiEvent::PhaseComplete {
            phase: "audit-done".into(),
        });

        if options.cancel.is_cancelled() {
            return Ok(());
        }

        // ── Phase 3: Reporter → code_review_report.md ────────────────────
        let combined = format!(
            "# General Code Review\n\n{}\n\n---\n\n# Security Audit\n\n{}\n\n---\n\n# Performance Analysis\n\n{}",
            review_notes, security_report, performance_report
        );
        let report = agents::reporter::run(&combined, &options).await?;
        fs.write("code_review_report.md", &report)?;
        send_phase_tasks(&options, CODE_REVIEW_TASKS, CODE_REVIEW_TASKS.len());
        let _ = options.tx.send(TuiEvent::PhaseComplete {
            phase: "done".into(),
        });

        let _ = options.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!("Code review complete. Report at: {}", output_dir.display()),
        });

        Ok(())
    }
}

/// Recursively collect source file contents, skipping build artifacts.
fn collect_source_files(dir: &std::path::Path) -> String {
    let mut content = String::new();
    let extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "swift", "c", "cpp", "h",
    ];
    let skip_dirs = ["target", "node_modules", ".git", "dist", "build", ".next"];

    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut sorted: Vec<_> = entries.flatten().collect();
        sorted.sort_by_key(|e| e.path());
        for entry in sorted {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !skip_dirs.contains(&name) {
                    content.push_str(&collect_source_files(&path));
                }
            } else if path.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if extensions.contains(&ext)
                    && let Ok(src) = std::fs::read_to_string(&path)
                {
                    // Truncate very large files to avoid context overflow
                    let truncated = if src.len() > 8000 {
                        format!("{}\n... (truncated)", &src[..8000])
                    } else {
                        src
                    };
                    content.push_str(&format!(
                        "\n\n## File: {}\n\n```{}\n{}\n```",
                        path.display(),
                        ext,
                        truncated
                    ));
                }
            }
        }
    }
    content
}
