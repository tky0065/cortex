mod assistant;
mod config;
mod context;
mod orchestrator;
mod project_context;
mod providers;
mod repl;
mod skills;
mod tools;
mod tui;
mod updater;
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
    /// Scan the current project and generate or update AGENTS.md
    Init {
        /// Regenerate the Cortex-managed AGENTS.md section even when it already exists
        #[arg(long)]
        force: bool,
    },
    /// Check for or install Cortex updates from GitHub Releases
    Update {
        /// Only check whether an update is available
        #[arg(long)]
        check: bool,
        /// Install a specific release tag, e.g. v0.1.3
        #[arg(long)]
        version: Option<String>,
    },
    /// Manage Cortex skills
    #[command(alias = "skills")]
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
}

#[derive(Subcommand)]
enum SkillCommands {
    /// List installed skills
    #[command(alias = "ls")]
    List,
    /// Show an installed skill
    Show { name: String },
    /// Add a skill from skills.sh, GitHub, URL, or a local path
    #[command(alias = "install")]
    Add {
        source: String,
        #[arg(long)]
        skill: Option<String>,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        project: bool,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        disabled: bool,
    },
    /// Enable an installed skill
    #[command(alias = "activate")]
    Enable {
        name: String,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        project: bool,
    },
    /// Disable an installed skill
    #[command(alias = "deactivate")]
    Disable {
        name: String,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        project: bool,
    },
    /// Remove an installed skill
    #[command(alias = "rm", alias = "delete", alias = "supprimer")]
    Remove {
        name: String,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        project: bool,
    },
    /// Create a new local skill
    Create {
        name: String,
        #[arg(short, long, default_value = "")]
        description: String,
        #[arg(long)]
        global: bool,
        #[arg(long)]
        project: bool,
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
        Some(Commands::Start {
            idea,
            auto,
            workflow,
            model,
            provider,
        }) => {
            let config = apply_overrides(config, model, provider);
            let wf = workflows::get_workflow(&workflow)?;
            let orch = Orchestrator::new(wf, Arc::new(config));
            orch.run_with_opts(idea, auto, verbose, None).await?;
        }
        Some(Commands::Run {
            workflow,
            prompt,
            model,
            provider,
        }) => {
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
            let orch = Orchestrator::new(workflows::get_workflow("dev")?, Arc::new(config));
            let prompt = format!(
                "Resume and complete the project in: {}",
                project_dir.display()
            );
            orch.run_with_opts(prompt, true, verbose, None).await?;
        }
        Some(Commands::Init { force }) => {
            let outcome =
                project_context::init_current_project(Arc::new(config), None, force).await?;
            println!(
                "{} AGENTS.md at {}",
                if outcome.created {
                    "Created"
                } else {
                    "Updated"
                },
                outcome.path.display()
            );
            println!(
                "Scanned {} files; detected: {}",
                outcome.files_scanned,
                if outcome.detected_stack.is_empty() {
                    "unknown".to_string()
                } else {
                    outcome.detected_stack.join(", ")
                }
            );
            if outcome.used_fallback {
                println!(
                    "Warning: LLM generation failed or returned unusable output; wrote deterministic AGENTS.md instead."
                );
            }
        }
        Some(Commands::Update { check, version }) => {
            if check {
                let status = updater::check_latest().await?;
                if status.update_available {
                    println!(
                        "Update available: {} -> {}. Run `cortex update` to install.",
                        status.current, status.latest
                    );
                } else {
                    println!("Cortex is up to date ({})", status.current);
                }
            } else {
                let install_version = if version.is_none() {
                    let status = updater::check_latest().await?;
                    if !status.update_available {
                        println!("Cortex is up to date ({})", status.current);
                        return Ok(());
                    }
                    Some(status.latest)
                } else {
                    version
                };
                let outcome = updater::update(install_version.as_deref()).await?;
                println!(
                    "Updated cortex {} -> {} at {}",
                    outcome.previous,
                    outcome.installed,
                    outcome.binary_path.display()
                );
                if outcome.restart_required {
                    println!("Restart your terminal to use the new version.");
                }
            }
        }
        Some(Commands::Skill { command }) => {
            for line in handle_skill_cli(command).await? {
                println!("{line}");
            }
        }
    }

    Ok(())
}

async fn handle_skill_cli(command: SkillCommands) -> Result<Vec<String>> {
    use skills::SkillScope;

    let scope = |global: bool, project: bool| -> Option<SkillScope> {
        if global {
            Some(SkillScope::Global)
        } else if project {
            Some(SkillScope::Project)
        } else {
            None
        }
    };

    match command {
        SkillCommands::List => skills::run_command(&["list".to_string()]),
        SkillCommands::Show { name } => skills::show(&name),
        SkillCommands::Add {
            source,
            skill,
            global,
            project,
            all,
            disabled,
        } => {
            let records = skills::add_from_source(
                &source,
                skills::AddOptions {
                    scope: scope(global, project),
                    skill,
                    all,
                    enabled: !disabled,
                },
            )
            .await?;
            Ok(records
                .into_iter()
                .map(|record| {
                    format!(
                        "added {} [{}] enabled={}",
                        record.name,
                        match record.scope {
                            SkillScope::Global => "global",
                            SkillScope::Project => "project",
                        },
                        record.enabled
                    )
                })
                .collect())
        }
        SkillCommands::Enable {
            name,
            global,
            project,
        } => {
            let record = skills::set_enabled(&name, scope(global, project), true)?;
            Ok(vec![format!("enabled {}", record.name)])
        }
        SkillCommands::Disable {
            name,
            global,
            project,
        } => {
            let record = skills::set_enabled(&name, scope(global, project), false)?;
            Ok(vec![format!("disabled {}", record.name)])
        }
        SkillCommands::Remove {
            name,
            global,
            project,
        } => {
            let record = skills::remove(&name, scope(global, project))?;
            Ok(vec![format!("removed {}", record.name)])
        }
        SkillCommands::Create {
            name,
            description,
            global,
            project,
        } => {
            let record = skills::create(
                &name,
                skills::CreateOptions {
                    scope: scope(global, project),
                    description,
                },
            )?;
            Ok(vec![format!(
                "created {} at {}",
                record.name,
                record.path.display()
            )])
        }
    }
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
