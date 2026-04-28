#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::tools::filesystem::FileSystem;
use crate::tools::terminal;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/devops.md");

/// Generate deployment files and run git commit.
pub async fn run(architecture: &str, options: &RunOptions, fs: &FileSystem) -> Result<()> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "devops".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("devops", &options.config)?;

    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!(
        "Create deployment infrastructure for this project:\n\n{}",
        architecture
    );

    let output = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("DevOps agent error: {e}"))?;

    // Parse and write the files
    parse_and_write_files(&output, fs)?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "devops".into(),
        chunk: "Deployment files created".into(),
    });

    // git init + add + commit
    git_commit(options).await?;

    let _ = options.tx.send(TuiEvent::AgentDone { agent: "devops".into() });
    Ok(())
}

fn parse_and_write_files(output: &str, fs: &FileSystem) -> Result<()> {
    let sections: Vec<&str> = output.split("=== FILE:").collect();
    for section in sections.iter().skip(1) {
        if let Some((header, content)) = section.split_once("===") {
            let file_path = header.trim();
            let file_content = content.trim_start_matches('\n');
            fs.write(file_path, file_content)?;
        }
    }
    Ok(())
}

async fn git_commit(options: &RunOptions) -> Result<()> {
    let dir = &options.project_dir;

    // git init (ignore error if already initialized)
    let _ = terminal::run("git", &["init"], Some(dir.as_path()), Some(30)).await;

    // git add
    let add_out = terminal::run("git", &["add", "."], Some(dir.as_path()), Some(30)).await?;
    if !add_out.success {
        let _ = options.tx.send(TuiEvent::Error {
            agent: "devops".into(),
            message: format!("git add failed: {}", add_out.stderr),
        });
        return Ok(()); // non-fatal
    }

    // git commit
    let commit_out = terminal::run(
        "git",
        &["commit", "-m", "chore: initial commit by cortex devops agent"],
        Some(dir.as_path()),
        Some(30),
    )
    .await?;

    if commit_out.success {
        let _ = options.tx.send(TuiEvent::TokenChunk {
            agent: "devops".into(),
            chunk: "git commit: initial commit".into(),
        });
    }
    Ok(())
}
