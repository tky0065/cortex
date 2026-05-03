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
        let mut pipeline_input = prompt;

        for step in &self.def.agents {
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
                            "  [hint] no agent file for '{}' — using generic fallback. \
                             Run /agent create {} to customise it.",
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

            let response = crate::providers::complete(
                &agent_def.model,
                &agent_def.system_prompt,
                &pipeline_input,
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
