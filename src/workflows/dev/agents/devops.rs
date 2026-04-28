#![allow(dead_code)]

use anyhow::Result;

use crate::tools::filesystem::FileSystem;
use crate::tools::terminal;
use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/devops.md");

/// Generate deployment files and run git commit.
pub async fn run(architecture: &str, options: &RunOptions, fs: &FileSystem) -> Result<()> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "devops".into(),
    });
    send_agent_progress(options, "devops", "Preparation des fichiers de deploiement");

    let model = crate::providers::model_for_role("devops", &options.config)?;
    let prompt = format!(
        "Create deployment infrastructure for this project:\n\n{}",
        architecture
    );
    let output = crate::providers::complete(model, PREAMBLE, &prompt)
        .await
        .map_err(|e| anyhow::anyhow!("DevOps agent error: {e}"))?;

    send_agent_progress(options, "devops", "Ecriture des fichiers de deploiement");
    parse_and_write_files(&output, fs)?;
    send_agent_summary(options, "devops", &output);

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "devops".into(),
        chunk: "Deployment files created".into(),
    });

    git_commit(options).await?;

    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "devops".into(),
    });
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

    send_agent_progress(
        options,
        "devops",
        "Initialisation git et creation du commit",
    );
    let _ = terminal::run("git", &["init"], Some(dir.as_path()), Some(30)).await;

    let add_out = terminal::run("git", &["add", "."], Some(dir.as_path()), Some(30)).await?;
    if !add_out.success {
        let _ = options.tx.send(TuiEvent::Error {
            agent: "devops".into(),
            message: format!("git add failed: {}", add_out.stderr),
        });
        return Ok(());
    }

    let commit_out = terminal::run(
        "git",
        &[
            "commit",
            "-m",
            "chore: initial commit by cortex devops agent",
        ],
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
