mod assistant;
mod config;
mod context;
mod orchestrator;
mod providers;
mod repl;
mod tools;
mod tui;
mod workflows;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::sync::RwLock;

use config::Config;
use orchestrator::Orchestrator;

#[derive(Parser)]
#[command(name = "cortex", version, about = "Your entire team, in one command.")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Log complete agent prompts/responses to cortex.log
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch a project from the command line (one-shot mode)
    Start {
        /// The natural-language idea to build
        idea: String,
        /// Run autonomously without interactive pauses
        #[arg(long)]
        auto: bool,
        /// Workflow to use (default: dev)
        #[arg(long, default_value = "dev")]
        workflow: String,
        /// Override model for all agents, e.g. groq/llama3-70b-8192 (not saved)
        #[arg(long)]
        model: Option<String>,
        /// Override default provider prefix, e.g. openrouter (not saved)
        #[arg(long)]
        provider: Option<String>,
    },
    /// Run an explicit workflow (alias for start --workflow)
    Run {
        #[arg(long)]
        workflow: String,
        prompt: String,
        /// Override model for all agents, e.g. groq/llama3-70b-8192 (not saved)
        #[arg(long)]
        model: Option<String>,
        /// Override default provider prefix, e.g. openrouter (not saved)
        #[arg(long)]
        provider: Option<String>,
    },
    /// Resume an interrupted workflow from an existing project directory
    Resume {
        /// Path to a previously started cortex project directory
        project_dir: std::path::PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let cli = Cli::parse();
    let verbose = cli.verbose;

    match cli.command {
        None => {
            let shared = Arc::new(RwLock::new(config));
            let (tx, rx) = tui::events::channel();
            tui::Tui::new()?.run(rx, tx, Arc::clone(&shared)).await?;
        }
        Some(Commands::Start { idea, auto, workflow, model, provider }) => {
            let config = apply_overrides(config, model, provider);
            let wf = workflows::get_workflow(&workflow)?;
            let orch = Orchestrator::new(wf, Arc::new(config));
            orch.run_with_opts(idea, auto, verbose, None).await?;
        }
        Some(Commands::Run { workflow, prompt, model, provider }) => {
            let config = apply_overrides(config, model, provider);
            let wf = workflows::get_workflow(&workflow)?;
            let orch = Orchestrator::new(wf, Arc::new(config));
            orch.run_with_opts(prompt, false, verbose, None).await?;
        }
        Some(Commands::Resume { project_dir }) => {
            if !project_dir.exists() {
                eprintln!(
                    "error: project directory does not exist: {}",
                    project_dir.display()
                );
                std::process::exit(1);
            }
            let orch = Orchestrator::new(
                workflows::get_workflow("dev")?,
                Arc::new(config),
            );
            let prompt = format!(
                "Resume and complete the project in: {}",
                project_dir.display()
            );
            orch.run_with_opts(prompt, true, verbose, None).await?;
        }
    }

    Ok(())
}

/// Apply one-shot `--model` / `--provider` overrides without persisting to disk.
fn apply_overrides(mut config: Config, model: Option<String>, provider: Option<String>) -> Config {
    if let Some(m) = model {
        // `--model` sets all roles to the given model string
        let _ = config.set_model("all", m);
    }
    if let Some(p) = provider {
        config.set_provider(p);
    }
    config
}
