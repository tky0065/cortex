use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::{RunOptions, Workflow};
use crate::tools::filesystem::FileSystem;
use crate::tui::events::TuiEvent;

pub mod agents;

pub struct ProspectingWorkflow;

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

        // ── Phase 1: Researcher → prospects.md ───────────────────────────
        let enriched_prompt = {
            let profile_prefix = load_profile(&options.project_dir).unwrap_or_default();
            format!("{}{}", profile_prefix, prompt)
        };
        let prospects_raw = agents::researcher::run(&enriched_prompt, &opts).await?;
        fs.write("prospects.md", &prospects_raw)?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "prospects-identified".into() });

        // ── Phase 2: Profiler ‖ Copywriter workers (parallel) ────────────
        let prospect_entries = parse_prospects(&prospects_raw);
        let sem = Arc::new(Semaphore::new(opts.config.limits.max_parallel_workers as usize));
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
                local_fs.write(format!("profiles/{}.md", slug), &profile)?;
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

        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "profiles-emails-ready".into() });

        // ── Phase 3: Outreach Manager → outreach_report.md ───────────────
        let report = agents::outreach_manager::run(&all_profiles_emails, &opts).await?;
        fs.write("outreach_report.md", &report)?;
        let _ = opts.tx.send(TuiEvent::PhaseComplete { phase: "done".into() });

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
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
