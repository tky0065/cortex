use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::agent_loader::AgentLoader;
use crate::custom_defs::{CustomAgentDef, CustomWorkflowDef};
use crate::tui::events::Task;
use crate::tui::events::TuiEvent;
use crate::workflows::{
    RunOptions, Workflow, request_agent_review, send_agent_progress, send_tool_action,
};

pub struct CustomWorkflow {
    pub def: CustomWorkflowDef,
}

/// Resolve the effective model for a custom agent.
///
/// If the agent's model string has a provider prefix that matches the currently
/// configured provider (e.g. `"ollama/qwen2.5-coder:32b"` while ollama is active),
/// use it as-is. Otherwise — including bare model names like `"gemini-flash-latest"`
/// that have no prefix — fall back to the assistant model so the call goes to the
/// correct provider regardless of what the agent YAML file was originally written for.
fn resolve_model(agent_model: &str, config: &crate::config::Config) -> String {
    let configured = config.provider.default.to_lowercase();
    if let Some((prefix, _)) = agent_model.split_once('/') {
        if prefix.to_lowercase() == configured {
            return agent_model.to_string();
        }
        // Provider prefix present but different → fall back
    }
    // No prefix, or mismatched prefix → use the configured assistant model
    config.models.assistant.clone()
}

#[async_trait]
impl Workflow for CustomWorkflow {
    fn name(&self) -> &str {
        &self.def.name
    }

    fn description(&self) -> &str {
        &self.def.description
    }

    async fn run(&self, prompt: String, options: RunOptions) -> Result<()> {
        let agent_names: Vec<String> = self.def.agents.iter().map(|s| s.role.clone()).collect();

        let _ = options.tx.send(TuiEvent::WorkflowStarted {
            workflow: self.def.name.clone(),
            agents: agent_names.clone(),
        });

        // Build task list from agent roles and display it
        let task_labels: Vec<String> = agent_names.clone();
        let send_tasks = |completed: usize| {
            let tasks: Vec<Task> = task_labels
                .iter()
                .enumerate()
                .map(|(i, name)| Task {
                    description: name.clone(),
                    is_done: i < completed,
                })
                .collect();
            let _ = options
                .tx
                .send(crate::tui::events::TuiEvent::TasksUpdated { tasks });
        };
        send_tasks(0);

        let project_root = std::env::current_dir().ok();
        let project_root_ref = project_root.as_deref();
        let original_prompt = prompt.clone();
        let mut pipeline_input = prompt;

        for (step_idx, step) in self.def.agents.iter().enumerate() {
            if options.cancel.is_cancelled() {
                return Ok(());
            }

            let agent_def = match AgentLoader::load_agent(&step.agent, project_root_ref)
                .with_context(|| format!("failed to load agent '{}'", step.agent))?
            {
                Some(def) => def,
                None => {
                    // Agent file not found — use a generic fallback so the workflow
                    // still runs. Log a hint so the user knows to create the file.
                    let _ = options.tx.send(TuiEvent::TokenChunk {
                        agent: step.role.clone(),
                        chunk: format!(
                            "  [WARNING] No agent file found for '{}'. \
                             Using generic fallback — output quality will be poor. \
                             Fix this: /agent create {}",
                            step.agent, step.agent
                        ),
                    });
                    let model = options.config.models.assistant.clone();
                    CustomAgentDef {
                        name: step.agent.clone(),
                        description: format!("Generic fallback for role '{}'", step.role),
                        model,
                        tools: vec![],
                        system_prompt: format!(
                            "You are a professional {}. Complete the task given to you \
                             thoroughly and accurately. Output only the result of your work, \
                             no meta-commentary.",
                            step.role
                        ),
                    }
                }
            };

            // Build per-step options carrying this agent's declared tools.
            // This controls whether web search fires for this specific agent.
            let step_opts = RunOptions {
                agent_tools: Some(agent_def.tools.clone()),
                ..options.clone()
            };

            let _ = step_opts.tx.send(TuiEvent::AgentStarted {
                agent: step.role.clone(),
            });
            send_agent_progress(&step_opts, &step.role, "Running...");

            // After the first agent, include the original user request so every
            // downstream agent knows the criteria it is working against.
            let agent_input = if step_idx == 0 {
                send_tool_action(&step_opts, &step.role, "read_input", "user prompt");
                pipeline_input.clone()
            } else {
                let prev_role = self
                    .def
                    .agents
                    .get(step_idx - 1)
                    .map(|s| s.role.as_str())
                    .unwrap_or("previous");
                send_tool_action(
                    &step_opts,
                    &step.role,
                    "read_input",
                    &format!("{} output", prev_role),
                );
                format!(
                    "## User request\n{}\n\n## Previous agent output\n{}",
                    original_prompt, pipeline_input
                )
            };

            // Tell the agent about web search only if it declared the tool.
            let uses_web_search = agent_def
                .tools
                .iter()
                .any(|t| t.eq_ignore_ascii_case("web_search"));
            let system_prompt = if uses_web_search {
                format!(
                    "{}\n\nIMPORTANT: The user message may contain a \
                     `## Web Search Results` section with live data fetched \
                     from the web. Use those results as your primary real-world \
                     data source. Do NOT invent or hallucinate information.",
                    agent_def.system_prompt
                )
            } else {
                agent_def.system_prompt.clone()
            };

            let effective_model = resolve_model(&agent_def.model, &options.config);
            let mut response = crate::providers::complete(
                &effective_model,
                &system_prompt,
                &agent_input,
                &step_opts,
                &step.role,
            )
            .await
            .with_context(|| format!("agent '{}' LLM call failed", step.role))?;

            let _ = step_opts.tx.send(TuiEvent::AgentDone {
                agent: step.role.clone(),
            });

            // Inter-agent review: wait for user approval or feedback
            let mut current_input = agent_input.clone();
            loop {
                if step_opts.cancel.is_cancelled() {
                    return Ok(());
                }
                match request_agent_review(&step.role, &response, &step_opts).await? {
                    None => break,
                    Some(feedback) => {
                        current_input =
                            format!("{}\n\n## User feedback\n{}", current_input, feedback);
                        response = crate::providers::complete(
                            &effective_model,
                            &system_prompt,
                            &current_input,
                            &step_opts,
                            &step.role,
                        )
                        .await
                        .with_context(|| format!("agent '{}' retry LLM call failed", step.role))?;
                    }
                }
            }

            send_tasks(step_idx + 1);
            pipeline_input = response;
        }

        // Write final output to disk so the REPL can count and display it.
        if let Err(e) = std::fs::create_dir_all(&options.project_dir) {
            let _ = options.tx.send(TuiEvent::TokenChunk {
                agent: "orchestrator".into(),
                chunk: format!("Warning: could not create output dir: {}", e),
            });
        } else {
            let filename = format!("{}_output.md", self.def.name);
            let output_path = options.project_dir.join(&filename);
            if let Err(e) = std::fs::write(&output_path, &pipeline_input) {
                let _ = options.tx.send(TuiEvent::TokenChunk {
                    agent: "orchestrator".into(),
                    chunk: format!("Warning: could not write output file: {}", e),
                });
            }
        }

        let _ = options.tx.send(TuiEvent::TokenChunk {
            agent: "orchestrator".into(),
            chunk: format!(
                "Custom workflow '{}' complete ({} chars).",
                self.def.name,
                pipeline_input.len()
            ),
        });

        Ok(())
    }
}
