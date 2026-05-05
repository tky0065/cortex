use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{RunOptions, Workflow, request_agent_review, send_phase_tasks, send_tool_action};
use crate::tools::filesystem::FileSystem;
use crate::tui::events::TuiEvent;

pub mod agents;

pub struct MarketingWorkflow;

const MARKETING_TASKS: &[&str] = &[
    "Creer la strategie marketing",
    "Generer les textes marketing",
    "Definir les metriques de campagne",
    "Construire le calendrier de publication",
    "Finaliser la campagne marketing",
];

#[async_trait]
impl Workflow for MarketingWorkflow {
    fn name(&self) -> &str {
        "marketing"
    }

    fn description(&self) -> &str {
        "Marketing campaign: Strategist → Copywriter ‖ Analyst → Social Media Manager"
    }

    async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
        let _ = options.tx.send(TuiEvent::WorkflowStarted {
            workflow: "marketing".into(),
            agents: vec![
                "strategist",
                "copywriter",
                "analyst",
                "social_media_manager",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        });

        // Create output directory
        let output_dir = options.project_dir.join("marketing-campaign");
        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Cannot create output dir: {}", output_dir.display()))?;

        let fs = FileSystem::new(&output_dir);
        let opts = RunOptions {
            project_dir: output_dir.clone(),
            ..options.clone()
        };
        send_phase_tasks(&opts, MARKETING_TASKS, 0);

        // ── Phase 1: Strategist → strategy.md ────────────────────────────
        let mut strategy = agents::strategist::run(&prompt, &opts).await?;
        send_tool_action(&opts, "strategist", "write_file", "strategy.md");
        fs.write("strategy.md", &strategy)?;
        send_phase_tasks(&opts, MARKETING_TASKS, 1);
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "strategy-ready".into(),
        });

        // Inter-agent review: Strategist output
        let mut strategist_input = prompt.clone();
        loop {
            if opts.cancel.is_cancelled() {
                return Ok(());
            }
            match request_agent_review("Strategist", &strategy, &opts).await? {
                None => break,
                Some(feedback) => {
                    strategist_input =
                        format!("{}\n\n## User feedback\n{}", strategist_input, feedback);
                    let new_strategy = agents::strategist::run(&strategist_input, &opts).await?;
                    fs.write("strategy.md", &new_strategy)?;
                    strategy = new_strategy;
                }
            }
        }

        // ── Phase 2: Copywriter ‖ Analyst (parallel) ─────────────────────
        let strategy_clone = strategy.clone();
        let opts_copy = opts.clone();
        let opts_analyst = opts.clone();

        let (copy_result, metrics_result) = tokio::join!(
            agents::copywriter::run(&strategy, &opts_copy),
            agents::analyst::run(&strategy_clone, &opts_analyst)
        );

        let mut copy = copy_result?;
        let metrics = metrics_result?;

        send_tool_action(&opts, "copywriter", "write_file", "posts.md");
        fs.write("posts.md", &copy)?;
        send_tool_action(&opts, "analyst", "write_file", "metrics.md");
        fs.write("metrics.md", &metrics)?;
        send_phase_tasks(&opts, MARKETING_TASKS, 3);
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "copy-and-metrics-ready".into(),
        });

        // Inter-agent review: Copywriter phase output
        let phase2_summary = format!(
            "Copywriter posts:\n{}\n\nAnalyst metrics:\n{}",
            copy, metrics
        );
        let mut copy_input = strategy.clone();
        loop {
            if opts.cancel.is_cancelled() {
                return Ok(());
            }
            match request_agent_review("Copywriter / Analyst", &phase2_summary, &opts).await? {
                None => break,
                Some(feedback) => {
                    copy_input = format!("{}\n\n## User feedback\n{}", copy_input, feedback);
                    let new_copy = agents::copywriter::run(&copy_input, &opts).await?;
                    fs.write("posts.md", &new_copy)?;
                    copy = new_copy;
                }
            }
        }

        // ── Phase 3: Social Media Manager → calendar.md ──────────────────
        let mut calendar = agents::social_media_manager::run(&strategy, &copy, &opts).await?;
        send_tool_action(&opts, "social_media_manager", "write_file", "calendar.md");
        fs.write("calendar.md", &calendar)?;
        send_phase_tasks(&opts, MARKETING_TASKS, MARKETING_TASKS.len());

        // Inter-agent review: Social Media Manager output
        let mut smm_input_strategy = strategy.clone();
        let mut smm_input_copy = copy.clone();
        loop {
            if opts.cancel.is_cancelled() {
                return Ok(());
            }
            match request_agent_review("Social Media Manager", &calendar, &opts).await? {
                None => break,
                Some(feedback) => {
                    smm_input_strategy =
                        format!("{}\n\n## User feedback\n{}", smm_input_strategy, feedback);
                    smm_input_copy =
                        format!("{}\n\n## User feedback\n{}", smm_input_copy, feedback);
                    let new_calendar = agents::social_media_manager::run(
                        &smm_input_strategy,
                        &smm_input_copy,
                        &opts,
                    )
                    .await?;
                    fs.write("calendar.md", &new_calendar)?;
                    calendar = new_calendar;
                }
            }
        }

        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "done".into(),
        });

        let _ = opts.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!("Marketing campaign ready at: {}", output_dir.display()),
        });

        Ok(())
    }
}
