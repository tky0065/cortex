#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::tools::filesystem::FileSystem;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/qa.md");

/// Run QA review on the project. Returns the QA report string.
pub async fn run(architecture: &str, options: &RunOptions, fs: &FileSystem) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "qa".into() });

    // Collect source files for review
    let source_files = collect_source_files(fs);

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("qa", &options.config)?;

    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!(
        "Architecture:\n{}\n\nSource files to review:\n{}\n\nProduce a QA report.",
        architecture,
        source_files
    );

    let report = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("QA agent error: {e}"))?;

    let passed = report.contains("RECOMMENDATION: APPROVE");
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "qa".into(),
        chunk: if passed { "QA: APPROVED".into() } else { "QA: NEEDS_FIXES".into() },
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "qa".into() });

    Ok(report)
}

fn collect_source_files(fs: &FileSystem) -> String {
    // Try to list src/ directory; gracefully handle if it doesn't exist
    let entries = fs.list("src").unwrap_or_default();
    let mut result = String::new();
    for entry in entries.iter().take(10) {
        let path_str = entry.to_string_lossy();
        if let Ok(content) = fs.read(path_str.as_ref()) {
            result.push_str(&format!(
                "\n=== {} ===\n{}\n",
                path_str,
                &content[..content.len().min(2000)]
            ));
        }
    }
    result
}
