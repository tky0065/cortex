use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::agent_loader::AgentLoader;
use crate::custom_defs::{CustomAgentDef, CustomWorkflowDef};
use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, Workflow, send_agent_progress};

pub struct CustomWorkflow {
    pub def: CustomWorkflowDef,
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
            agents: agent_names,
        });

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

            let _ = options.tx.send(TuiEvent::AgentStarted {
                agent: step.role.clone(),
            });
            send_agent_progress(&options, &step.role, "Running...");

            // After the first agent, include the original user request so every
            // downstream agent knows the criteria it is working against.
            let agent_input = if step_idx == 0 {
                pipeline_input.clone()
            } else {
                format!(
                    "## User request\n{}\n\n## Previous agent output\n{}",
                    original_prompt, pipeline_input
                )
            };

            // When web search is enabled, append a note so the agent knows
            // to use the injected `## Web Search Results` block.
            let system_prompt = if options.config.tools.web_search_enabled {
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

            let response = crate::providers::complete(
                &agent_def.model,
                &system_prompt,
                &agent_input,
                &options,
                &step.role,
            )
            .await
            .with_context(|| format!("agent '{}' LLM call failed", step.role))?;

            let _ = options.tx.send(TuiEvent::AgentDone {
                agent: step.role.clone(),
            });

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
