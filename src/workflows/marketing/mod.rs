use anyhow::{Context, Result};
use async_trait::async_trait;

use super::{RunOptions, Workflow, send_phase_tasks};
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
        let strategy = agents::strategist::run(&prompt, &opts).await?;
        fs.write("strategy.md", &strategy)?;
        send_phase_tasks(&opts, MARKETING_TASKS, 1);
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "strategy-ready".into(),
        });

        // ── Phase 2: Copywriter ‖ Analyst (parallel) ─────────────────────
        let strategy_clone = strategy.clone();
        let opts_copy = opts.clone();
        let opts_analyst = opts.clone();

        let (copy_result, metrics_result) = tokio::join!(
            agents::copywriter::run(&strategy, &opts_copy),
            agents::analyst::run(&strategy_clone, &opts_analyst)
        );

        let copy = copy_result?;
        let metrics = metrics_result?;

        fs.write("posts.md", &copy)?;
        fs.write("metrics.md", &metrics)?;
        send_phase_tasks(&opts, MARKETING_TASKS, 3);
        let _ = opts.tx.send(TuiEvent::PhaseComplete {
            phase: "copy-and-metrics-ready".into(),
        });

        // ── Phase 3: Social Media Manager → calendar.md ──────────────────
        let calendar = agents::social_media_manager::run(&strategy, &copy, &opts).await?;
        fs.write("calendar.md", &calendar)?;
        send_phase_tasks(&opts, MARKETING_TASKS, MARKETING_TASKS.len());
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
