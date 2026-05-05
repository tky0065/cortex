use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::{RunOptions, Workflow, request_agent_review, send_phase_tasks, send_tool_action};
use crate::tools::filesystem::FileSystem;
use crate::tui::events::TuiEvent;

pub mod agents;

pub struct ProspectingWorkflow;

const PROSPECTING_TASKS: &[&str] = &[
    "Identifier les prospects",
    "Generer les profils prospects",
    "Rediger les emails personnalises",
    "Produire le rapport d'outreach",
    "Finaliser la campagne dry-run",
];

#[async_trait]
impl Workflow for ProspectingWorkflow {
    fn name(&self) -> &str {
        "prospecting"
    }

    fn description(&self) -> &str {
        "Freelance outreach: Researcher → Profiler ‖ Copywriter → Outreach Manager"
    }

    async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
        let _ = options.tx.send(TuiEvent::WorkflowStarted {
            workflow: "prospecting".into(),
            agents: vec!["researcher", "profiler", "copywriter", "outreach_manager"]
                .into_iter()
                .map(String::from)
                .collect(),
        });

        // RGPD guardrail: warn user
        let _ = options.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: "⚠ RGPD: Only public data used. Dry-run mode (no emails sent). Reply STOP to unsubscribe.".into(),
        });

        // Create output directory
        let output_dir = options.project_dir.join("prospecting-campaign");
        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Cannot create output dir: {}", output_dir.display()))?;

        let fs = FileSystem::new(&output_dir);
        let opts = RunOptions {
            project_dir: output_dir.clone(),
            ..options.clone()
        };
        send_phase_tasks(&opts, PROSPECTING_TASKS, 0);

        // ── Phase 1: Researcher → prospects.md ───────────────────────────
        let enriched_prompt = {
            let profile_prefix = load_profile(&options.project_dir).unwrap_or_default();
            format!("{}{}", profile_prefix, prompt)
        };
        let mut prospects_raw = agents::researcher::run(&enriched_prompt, &opts).await?;
        send_tool_action(&opts, "researcher", "write_file", "prospects.md");
        fs.write("prospects.md", &prospects_raw)?;
        send_phase_tasks(&opts, PROSPECTING_TASKS, 1);
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "prospects-identified".into(),
        });

        // Inter-agent review: Researcher output
        let mut researcher_input = enriched_prompt.clone();
        loop {
            if opts.cancel.is_cancelled() {
                return Ok(());
            }
            match request_agent_review("Researcher", &prospects_raw, &opts).await? {
                None => break,
                Some(feedback) => {
                    researcher_input =
                        format!("{}\n\n## User feedback\n{}", researcher_input, feedback);
                    let new_prospects = agents::researcher::run(&researcher_input, &opts).await?;
                    fs.write("prospects.md", &new_prospects)?;
                    prospects_raw = new_prospects;
                }
            }
        }

        // ── Phase 2: Profiler ‖ Copywriter workers (parallel) ────────────
        let prospect_entries = parse_prospects(&prospects_raw);
        let prospect_count = prospect_entries.len();
        let sem = Arc::new(Semaphore::new(
            opts.config.limits.max_parallel_workers as usize,
        ));
        let mut handles = Vec::new();

        for entry in prospect_entries {
            let permit = Arc::clone(&sem).acquire_owned().await?;
            let opts_clone = opts.clone();
            let prompt_clone = prompt.clone();
            let output_dir_clone = output_dir.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit;
                let local_fs = FileSystem::new(&output_dir_clone);

                // Profile
                let profile = agents::profiler::run(&entry, &opts_clone).await?;

                // Email (dry-run)
                let email = agents::copywriter::run(&profile, &prompt_clone, &opts_clone).await?;

                // Save per-prospect files
                let slug = slugify_prospect(&entry);
                send_tool_action(
                    &opts_clone,
                    "profiler",
                    "write_file",
                    &format!("profiles/{}.md", slug),
                );
                local_fs.write(format!("profiles/{}.md", slug), &profile)?;
                send_tool_action(
                    &opts_clone,
                    "copywriter",
                    "write_file",
                    &format!("emails/{}.md", slug),
                );
                local_fs.write(format!("emails/{}.md", slug), &email)?;

                Ok::<(String, String), anyhow::Error>((profile, email))
            });
            handles.push(handle);
        }

        let mut all_profiles_emails = String::new();
        for handle in handles {
            let (profile, email) = handle
                .await
                .map_err(|e| anyhow::anyhow!("Profiler/Copywriter worker panicked: {e}"))??;
            all_profiles_emails.push_str(&format!(
                "\n\n--- PROFILE ---\n{}\n\n--- EMAIL ---\n{}",
                profile, email
            ));
        }

        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "profiles-emails-ready".into(),
        });
        send_phase_tasks(&opts, PROSPECTING_TASKS, 3);

        // Inter-agent review: Profiler/Copywriter phase summary
        // Re-running the full parallel pool is expensive so we accept one round of feedback
        // and forward it to the outreach manager rather than spawning new worker tasks.
        if !opts.cancel.is_cancelled() {
            let phase2_summary = format!(
                "Profiler and Copywriter generated {} prospect profiles and emails.",
                prospect_count
            );
            if let Some(feedback) =
                request_agent_review("Profiler / Copywriter", &phase2_summary, &opts).await?
            {
                let _ = opts.tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: format!("Profiler/Copywriter feedback noted: {}", feedback),
                });
                all_profiles_emails =
                    format!("{}\n\n## User feedback\n{}", all_profiles_emails, feedback);
            }
        }

        // ── Phase 3: Outreach Manager → outreach_report.md ───────────────
        let mut report = agents::outreach_manager::run(&all_profiles_emails, &opts).await?;
        send_tool_action(
            &opts,
            "outreach_manager",
            "write_file",
            "outreach_report.md",
        );
        fs.write("outreach_report.md", &report)?;
        send_phase_tasks(&opts, PROSPECTING_TASKS, PROSPECTING_TASKS.len());

        // Inter-agent review: Outreach Manager output
        let mut outreach_input = all_profiles_emails.clone();
        loop {
            if opts.cancel.is_cancelled() {
                return Ok(());
            }
            match request_agent_review("Outreach Manager", &report, &opts).await? {
                None => break,
                Some(feedback) => {
                    outreach_input =
                        format!("{}\n\n## User feedback\n{}", outreach_input, feedback);
                    let new_report = agents::outreach_manager::run(&outreach_input, &opts).await?;
                    fs.write("outreach_report.md", &new_report)?;
                    report = new_report;
                }
            }
        }

        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "done".into(),
        });

        let _ = opts.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!(
                "Prospecting campaign ready (DRY-RUN) at: {}",
                output_dir.display()
            ),
        });

        Ok(())
    }
}

/// Load user profile from profile.toml in the project dir, if present.
fn load_profile(project_dir: &std::path::Path) -> Option<String> {
    let profile_path = project_dir.join("profile.toml");
    let content = std::fs::read_to_string(&profile_path).ok()?;
    Some(format!("## User Profile\n\n```toml\n{}\n```\n\n", content))
}

fn parse_prospects(raw: &str) -> Vec<String> {
    // Split on "## " headings (each prospect starts with ##)
    raw.split("\n## ")
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("## {}", s))
        .collect()
}

fn slugify_prospect(entry: &str) -> String {
    entry
        .lines()
        .next()
        .unwrap_or("prospect")
        .trim_start_matches('#')
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
