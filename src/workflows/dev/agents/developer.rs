#![allow(dead_code)]

use anyhow::Result;

use crate::tools::filesystem::FileSystem;
use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, bus_agent_done, bus_agent_started, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/developer.md");

/// Implement a single source file given the architecture context.
pub async fn run(file_path: &str, architecture: &str, options: &RunOptions) -> Result<String> {
    let agent_name = format!("developer:{}", file_path);
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: agent_name.clone(),
    });
    send_agent_progress(
        options,
        agent_name.clone(),
        format!("Implementation de {}", file_path),
    );
    bus_agent_started(options, &agent_name).await;

    let model = crate::providers::model_for_role("developer", &options.config)?;
    let prompt = format!(
        "Architecture:\n{}\n\nImplement this file: {}\n\nWrite only the complete source code.",
        architecture, file_path
    );
    let code = crate::providers::complete(model, PREAMBLE, &prompt, options, &agent_name)
        .await
        .map_err(|e| anyhow::anyhow!("Developer agent error for '{}': {}", file_path, e))?;

    send_agent_summary(options, agent_name.clone(), &code);
    bus_agent_done(options, &agent_name, &code).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("{} implemented ({} chars)", file_path, code.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });

    Ok(code)
}

/// Fix issues reported by QA for a specific file.
pub async fn fix(
    file_path: &str,
    current_code: &str,
    qa_report: &str,
    options: &RunOptions,
    fs: &FileSystem,
) -> Result<()> {
    let agent_name = format!("developer:fix:{}", file_path);
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: agent_name.clone(),
    });
    send_agent_progress(
        options,
        agent_name.clone(),
        format!("Correction de {}", file_path),
    );
    bus_agent_started(options, &agent_name).await;

    let model = crate::providers::model_for_role("developer", &options.config)?;
    let prompt = format!(
        "Fix the following issues in {}.\n\nCurrent code:\n{}\n\nQA Report:\n{}\n\nWrite the complete fixed source code.",
        file_path, current_code, qa_report
    );
    let fixed = crate::providers::complete(model, PREAMBLE, &prompt, options, &agent_name)
        .await
        .map_err(|e| anyhow::anyhow!("Developer fix error for '{}': {}", file_path, e))?;

    fs.write(file_path, &fixed)?;

    send_agent_summary(options, agent_name.clone(), &fixed);
    bus_agent_done(options, &agent_name, &fixed).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("{} fixed ({} chars)", file_path, fixed.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });
    Ok(())
}
