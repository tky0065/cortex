/// Agentic assistant with tool-use capability.
///
/// Implements a ReAct-style loop: the LLM reasons, emits `<tool_call>` blocks,
/// the runtime executes them and feeds results back, until the task is done or
/// the iteration cap is reached.
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agent_bus::{AgentBus, AgentDirective, AgentStatus};
use crate::config::Config;
use crate::tools::filesystem::FileSystem;
use crate::tools::terminal;
use crate::tui::events::{TuiEvent, TuiSender};

const MAX_ITERATIONS: usize = 15;
const EMPTY_FINAL_REPLY: &str = "Je n'ai pas recu de reponse texte du modele. Reessaie avec un autre modele ou reformule la demande.";

/// System prompt describing the assistant's capabilities and tool format.
pub const PREAMBLE: &str = "\
You are Cortex Assistant, a powerful autonomous AI agent embedded in a software development CLI.\n\
You can answer questions directly and can use tools when the user's request needs local files, commands, edits, or live web context.\n\
Do not expose tool XML to the user. Tool calls are private runtime instructions.\n\
\n\
## AVAILABLE TOOLS\n\
\n\
Use tools only when needed by emitting XML blocks in your response:\n\
\n\
### Write a file\n\
<tool_call>\n\
<name>write_file</name>\n\
<path>relative/path/to/file.txt</path>\n\
<content>\n\
file content goes here\n\
</content>\n\
</tool_call>\n\
\n\
### Read a file\n\
<tool_call>\n\
<name>read_file</name>\n\
<path>relative/path/to/file.txt</path>\n\
</tool_call>\n\
\n\
### List directory\n\
<tool_call>\n\
<name>list_files</name>\n\
<path>.</path>\n\
</tool_call>\n\
\n\
### Run a command (allowed: cargo, go, npm, pip, git, docker)\n\
<tool_call>\n\
<name>run_command</name>\n\
<command>git</command>\n\
<args>status</args>\n\
</tool_call>\n\
\n\
### Search the web\n\
Use this for current news, recent versions, pricing, security advisories, or other time-sensitive information.\n\
<tool_call>\n\
<name>web_search</name>\n\
<query>agentic coding tools news</query>\n\
</tool_call>\n\
\n\
### Query agent status\n\
Check the status and output of a specific agent (or all agents) in the current workflow.\n\
<tool_call>\n\
<name>query_agent_status</name>\n\
<agent>pm</agent>\n\
</tool_call>\n\
Omit <agent> to query all agents at once.\n\
\n\
### Delegate a task to an agent\n\
Run a standalone agent with a custom instruction (does not require a running workflow).\n\
<tool_call>\n\
<name>delegate_to_agent</name>\n\
<agent>ceo</agent>\n\
<instruction>Write an executive brief for a note-taking app</instruction>\n\
</tool_call>\n\
\n\
### Inject a directive into the running workflow\n\
Send an instruction to a specific agent that will be picked up at the next phase boundary.\n\
<tool_call>\n\
<name>inject_directive</name>\n\
<agent>developer</agent>\n\
<instruction>Add unit tests for the authentication module</instruction>\n\
</tool_call>\n\
\n\
## RULES\n\
- For casual chat or general non-current knowledge, answer directly without tools.\n\
- For current news or time-sensitive facts, use web_search first.\n\
- If web_search fails or returns an error (e.g. disabled or no API key configured):\n\
  1. Clearly tell the user you cannot access live/current information right now.\n\
  2. Briefly explain how to enable it: /websearch enable and /apikey web_search <key>.\n\
  3. Only then share what you know from training data, and explicitly label it as potentially outdated.\n\
- For repo-specific questions, read the relevant files first.\n\
- For file edits or commands, use tools to complete the task.\n\
- For complex or multi-step tasks, ALWAYS create a `TASKS.md` file in the project root with a checklist (`- [ ] Task description`). Update this file by marking tasks as done (`- [x]`) as you complete them so the user can track progress.\n\
- Use query_agent_status to check what agents have done before delegating new tasks.\n\
- Use inject_directive when a workflow is running and you want to steer an agent at the next phase.\n\
- Use delegate_to_agent for on-demand agent tasks outside of a workflow.\n\
- After tool calls are executed, provide a concise natural-language answer.\n\
- You can chain multiple tool calls in one response.\n\
- Paths are relative to the current working directory.\n\
- For `run_command`, `args` is a single space-separated string (e.g. `init --name myproject`).\n\
";

// ---------------------------------------------------------------------------
// Tool call data structures
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum ToolCall {
    WriteFile {
        path: String,
        content: String,
    },
    ReadFile {
        path: String,
    },
    ListFiles {
        path: String,
    },
    RunCommand {
        command: String,
        args: String,
    },
    WebSearch {
        query: String,
    },
    /// Query the status and output of one or all workflow agents.
    QueryAgentStatus {
        agent: Option<String>,
    },
    /// Run a standalone agent with a custom instruction (outside any workflow).
    DelegateToAgent {
        agent: String,
        instruction: String,
    },
    /// Inject a directive into the currently-running workflow for an agent to pick up.
    InjectDirective {
        agent: String,
        instruction: String,
    },
}

#[derive(Debug)]
struct ToolResult {
    call: String,
    output: String,
    success: bool,
}

// ---------------------------------------------------------------------------
// XML parser for <tool_call> blocks
// ---------------------------------------------------------------------------

/// Extract all `<tool_call>` blocks from an LLM response and parse them.
fn parse_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<tool_call>") {
        let after_open = &remaining[start + "<tool_call>".len()..];
        if let Some(end) = after_open.find("</tool_call>") {
            let block = &after_open[..end];
            if let Some(call) = parse_single_call(block) {
                calls.push(call);
            }
            remaining = &after_open[end + "</tool_call>".len()..];
        } else {
            break;
        }
    }

    calls
}

fn extract_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)?;
    Some(text[start..start + end].trim())
}

fn parse_single_call(block: &str) -> Option<ToolCall> {
    let name = extract_tag(block, "name")?;
    match name {
        "write_file" => {
            let path = extract_tag(block, "path")?.to_string();
            // Content may contain leading/trailing newlines — preserve inner content
            let content = extract_content_tag(block)?;
            Some(ToolCall::WriteFile { path, content })
        }
        "read_file" => {
            let path = extract_tag(block, "path")?.to_string();
            Some(ToolCall::ReadFile { path })
        }
        "list_files" => {
            let path = extract_tag(block, "path").unwrap_or(".").to_string();
            Some(ToolCall::ListFiles { path })
        }
        "run_command" => {
            let command = extract_tag(block, "command")?.to_string();
            let args = extract_tag(block, "args").unwrap_or("").to_string();
            Some(ToolCall::RunCommand { command, args })
        }
        "web_search" => {
            let query = extract_tag(block, "query")?.to_string();
            Some(ToolCall::WebSearch { query })
        }
        "query_agent_status" => {
            let agent = extract_tag(block, "agent").map(|s| s.to_string());
            Some(ToolCall::QueryAgentStatus { agent })
        }
        "delegate_to_agent" => {
            let agent = extract_tag(block, "agent")?.to_string();
            let instruction = extract_tag(block, "instruction")?.to_string();
            Some(ToolCall::DelegateToAgent { agent, instruction })
        }
        "inject_directive" => {
            let agent = extract_tag(block, "agent")?.to_string();
            let instruction = extract_tag(block, "instruction")?.to_string();
            Some(ToolCall::InjectDirective { agent, instruction })
        }
        _ => None,
    }
}

pub(crate) fn strip_tool_calls_for_display(text: &str) -> String {
    let mut output = String::new();
    let mut remaining = text;

    while let Some(start) = remaining.find("<tool_call>") {
        output.push_str(&remaining[..start]);
        let after_open = &remaining[start + "<tool_call>".len()..];
        if let Some(end) = after_open.find("</tool_call>") {
            remaining = &after_open[end + "</tool_call>".len()..];
        } else {
            remaining = "";
            break;
        }
    }

    output.push_str(remaining);
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_tool_calls(text: &str) -> String {
    strip_tool_calls_for_display(text)
}

fn escape_tool_result(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Extract `<content>…</content>`, preserving internal newlines but stripping
/// exactly one leading and one trailing newline (added by the XML formatting).
fn extract_content_tag(block: &str) -> Option<String> {
    let start = block.find("<content>")? + "<content>".len();
    let end = block[start..].find("</content>")?;
    let raw = &block[start..start + end];
    // Strip at most one leading newline and one trailing newline
    let trimmed = raw.strip_prefix('\n').unwrap_or(raw);
    let trimmed = trimmed.strip_suffix('\n').unwrap_or(trimmed);
    Some(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// Tool executor
// ---------------------------------------------------------------------------

async fn execute_tool(
    call: &ToolCall,
    sandbox: &FileSystem,
    config: Arc<RwLock<Config>>,
    agent_bus: Option<Arc<AgentBus>>,
    tx: &TuiSender,
) -> ToolResult {
    match call {
        ToolCall::WriteFile { path, content } => {
            let display = format!("write_file({})", path);
            let old_content = sandbox.read(path).ok();
            match sandbox.write(path, content) {
                Ok(()) => {
                    let _ = tx.send(TuiEvent::FileWritten {
                        agent: "assistant".to_string(),
                        path: path.clone(),
                        old_content,
                        new_content: content.clone(),
                    });
                    ToolResult {
                        call: display,
                        output: format!("✓ wrote {} bytes to '{}'", content.len(), path),
                        success: true,
                    }
                }
                Err(e) => ToolResult {
                    call: display,
                    output: format!("error: {e}"),
                    success: false,
                },
            }
        }
        ToolCall::ReadFile { path } => {
            let display = format!("read_file({})", path);
            match sandbox.read(path) {
                Ok(content) => ToolResult {
                    call: display,
                    output: content,
                    success: true,
                },
                Err(e) => ToolResult {
                    call: display,
                    output: format!("error: {e}"),
                    success: false,
                },
            }
        }
        ToolCall::ListFiles { path } => {
            let display = format!("list_files({})", path);
            match sandbox.list(path) {
                Ok(entries) => {
                    let listing = entries
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ToolResult {
                        call: display,
                        output: if listing.is_empty() {
                            "(empty)".to_string()
                        } else {
                            listing
                        },
                        success: true,
                    }
                }
                Err(e) => ToolResult {
                    call: display,
                    output: format!("error: {e}"),
                    success: false,
                },
            }
        }
        ToolCall::RunCommand { command, args } => {
            let display = format!("run_command({} {})", command, args);
            let arg_vec: Vec<&str> = if args.is_empty() {
                vec![]
            } else {
                args.split_whitespace().collect()
            };
            match terminal::run(command, &arg_vec, None, Some(60)).await {
                Ok(out) => {
                    let combined = if out.stderr.is_empty() {
                        out.stdout.clone()
                    } else if out.stdout.is_empty() {
                        out.stderr.clone()
                    } else {
                        format!("{}\n{}", out.stdout, out.stderr)
                    };
                    ToolResult {
                        call: display,
                        output: if combined.is_empty() {
                            "(no output)".to_string()
                        } else {
                            combined
                        },
                        success: out.success,
                    }
                }
                Err(e) => ToolResult {
                    call: display,
                    output: format!("error: {e}"),
                    success: false,
                },
            }
        }
        ToolCall::WebSearch { query } => {
            let display = format!("web_search({})", query);
            let config = config.read().await;
            if !config.tools.web_search_enabled {
                return ToolResult {
                    call: display,
                    output: "error: web search is disabled. Enable it with /websearch enable and configure a Brave Search API key with /apikey web_search <key>.".to_string(),
                    success: false,
                };
            }
            if config.api_keys.web_search.is_none() {
                return ToolResult {
                    call: display,
                    output: "error: web search is enabled but no Brave Search API key is configured. Use /apikey web_search <key>.".to_string(),
                    success: false,
                };
            }

            let context = crate::tools::web_search::fetch_context(query, &config).await;
            let success = !context.trim().is_empty();
            ToolResult {
                call: display,
                output: if success {
                    context
                } else {
                    "error: web search returned no results.".to_string()
                },
                success,
            }
        }
        ToolCall::QueryAgentStatus { agent } => {
            let display = match agent {
                Some(a) => format!("query_agent_status({})", a),
                None => "query_agent_status(all)".to_string(),
            };
            match &agent_bus {
                None => ToolResult {
                    call: display,
                    output: "No workflow is currently running. No agent bus available.".to_string(),
                    success: false,
                },
                Some(bus) => {
                    let output = match agent {
                        Some(name) => match bus.get_status(name).await {
                            None => format!("Agent '{}' has no recorded status yet.", name),
                            Some(record) => {
                                let out_part = match &record.output {
                                    None => String::new(),
                                    Some(o) => {
                                        format!("\nOutput preview:\n{}", &o[..o.len().min(500)])
                                    }
                                };
                                format!("Agent '{}' — status: {}{}", name, record.status, out_part)
                            }
                        },
                        None => {
                            let all = bus.get_all_statuses().await;
                            if all.is_empty() {
                                "No agent activity recorded yet.".to_string()
                            } else {
                                all.iter()
                                    .map(|(name, rec)| {
                                        let out_hint = rec
                                            .output
                                            .as_ref()
                                            .map(|o| format!(" ({} chars)", o.len()))
                                            .unwrap_or_default();
                                        format!("  {:12} [{}]{}", name, rec.status, out_hint)
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            }
                        }
                    };
                    ToolResult {
                        call: display,
                        output,
                        success: true,
                    }
                }
            }
        }
        ToolCall::DelegateToAgent { agent, instruction } => {
            let display = format!("delegate_to_agent({}, ...)", agent);
            let cfg = config.read().await;
            let model = match crate::providers::model_for_role(agent, &cfg) {
                Ok(m) => m.to_string(),
                Err(e) => {
                    return ToolResult {
                        call: display,
                        output: format!("error: unknown agent '{}': {}", agent, e),
                        success: false,
                    };
                }
            };
            // Resolve the agent system prompt (preamble) by role.
            let preamble = agent_preamble_for_role(agent);
            drop(cfg);

            // Build minimal RunOptions for a standalone call.
            let cfg2 = config.read().await;
            let (resume_tx, resume_rx) = tokio::sync::mpsc::channel::<()>(1);
            let (answer_tx, answer_rx) = tokio::sync::mpsc::channel::<String>(1);
            let opts = crate::workflows::RunOptions {
                auto: true,
                config: Arc::new(cfg2.clone()),
                tx: tx.clone(),
                project_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
                cancel: tokio_util::sync::CancellationToken::new(),
                resume_tx: Arc::new(resume_tx),
                resume_rx: Arc::new(tokio::sync::Mutex::new(resume_rx)),
                answer_tx: Arc::new(answer_tx),
                answer_rx: Arc::new(tokio::sync::Mutex::new(answer_rx)),
                verbose: false,
                agent_bus: agent_bus.clone(),
            };
            drop(cfg2);

            match crate::providers::complete(&model, &preamble, instruction, &opts, agent).await {
                Ok(response) => {
                    // Register the result in the bus so future queries can see it.
                    if let Some(ref bus) = agent_bus {
                        bus.update_agent(agent, AgentStatus::Done, Some(response.clone()))
                            .await;
                    }
                    ToolResult {
                        call: display,
                        output: response,
                        success: true,
                    }
                }
                Err(e) => {
                    if let Some(ref bus) = agent_bus {
                        bus.update_agent(agent, AgentStatus::Error(e.to_string()), None)
                            .await;
                    }
                    ToolResult {
                        call: display,
                        output: format!("error: {e}"),
                        success: false,
                    }
                }
            }
        }
        ToolCall::InjectDirective { agent, instruction } => {
            let display = format!("inject_directive({}, ...)", agent);
            match &agent_bus {
                None => ToolResult {
                    call: display,
                    output: "No workflow is currently running — directive cannot be injected."
                        .to_string(),
                    success: false,
                },
                Some(bus) => match bus.send_directive(AgentDirective {
                    target_agent: agent.clone(),
                    instruction: instruction.clone(),
                }) {
                    Ok(()) => ToolResult {
                        call: display,
                        output: format!(
                            "Directive queued for '{}'. It will be processed at the next phase boundary.",
                            agent
                        ),
                        success: true,
                    },
                    Err(e) => ToolResult {
                        call: display,
                        output: format!("error: {e}"),
                        success: false,
                    },
                },
            }
        }
    }
}

fn describe_tool(call: &ToolCall) -> String {
    match call {
        ToolCall::WriteFile { path, .. } => format!("writing {}", path),
        ToolCall::ReadFile { path } => format!("reading {}", path),
        ToolCall::ListFiles { path } => format!("listing {}", path),
        ToolCall::RunCommand { command, args } if args.is_empty() => format!("running {}", command),
        ToolCall::RunCommand { command, args } => format!("running {} {}", command, args),
        ToolCall::WebSearch { query } => format!("searching web for {}", query),
        ToolCall::QueryAgentStatus { agent: Some(a) } => {
            format!("querying status of agent '{}'", a)
        }
        ToolCall::QueryAgentStatus { agent: None } => "querying all agent statuses".to_string(),
        ToolCall::DelegateToAgent { agent, .. } => format!("delegating task to agent '{}'", agent),
        ToolCall::InjectDirective { agent, .. } => {
            format!("injecting directive to agent '{}'", agent)
        }
    }
}

fn empty_final_retry_prompt(original_message: &str, previous_prompt: &str) -> String {
    format!(
        "Your previous answer contained no visible text for the user.\n\
         Answer the original request in natural language now. Do not call tools unless strictly necessary, and do not include tool_call XML.\n\n\
         Original request:\n{}\n\n\
         Available context/tool results from the previous turn:\n{}",
        original_message, previous_prompt
    )
}

/// Return a concise system preamble for a given role when running standalone.
fn agent_preamble_for_role(role: &str) -> String {
    // Use the workflow agent's own embedded preamble when possible.
    // Unknown roles fall back to a generic instruction.
    match role {
        "ceo" => include_str!("workflows/dev/prompts/ceo.md").to_string(),
        "pm" => include_str!("workflows/dev/prompts/pm.md").to_string(),
        "tech_lead" => include_str!("workflows/dev/prompts/tech_lead.md").to_string(),
        "developer" => include_str!("workflows/dev/prompts/developer.md").to_string(),
        "qa" => include_str!("workflows/dev/prompts/qa.md").to_string(),
        "devops" => include_str!("workflows/dev/prompts/devops.md").to_string(),
        _ => format!(
            "You are the '{}' agent. Complete the given task to the best of your ability. For complex tasks, ALWAYS create and update a `TASKS.md` file using `- [ ]` checklist format to track your progress.",
            role
        ),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run the assistant agentic loop for a free-form user message.
///
/// - Streams `TuiEvent::TokenChunk` for each LLM token.
/// - Emits tool call / result log lines.
/// - Loops until no more tool calls or `MAX_ITERATIONS` is reached.
pub async fn run(
    message: &str,
    history: Vec<rig::completion::Message>,
    model: &str,
    tx: &TuiSender,
    config: Arc<RwLock<Config>>,
    agent_bus: Option<Arc<AgentBus>>,
) -> Result<String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let sandbox = FileSystem::new(&cwd);

    // Build a mutable conversation: system preamble is handled via `complete_chat`,
    // so we just accumulate user + assistant turns here.
    let mut conv_history = history;
    let mut current_message = message.to_string();
    let mut final_reply = String::new();
    let mut retried_empty_final = false;

    for iteration in 0..MAX_ITERATIONS {
        // ── LLM call ──────────────────────────────────────────────────────────
        send(
            tx,
            TuiEvent::AgentProgress {
                agent: "assistant".to_string(),
                message: if iteration == 0 {
                    "Thinking…".to_string()
                } else {
                    format!("Iteration {}…", iteration + 1)
                },
            },
        );

        let config_snapshot = config.read().await.clone();
        let reply = crate::providers::complete_chat_stream(
            model,
            &assistant_preamble(&config, &current_message).await,
            conv_history.clone(),
            &current_message,
            &config_snapshot,
            tx,
            "assistant",
        )
        .await?;

        // ── Parse tool calls ─────────────────────────────────────────────────
        let calls = parse_tool_calls(&reply);
        if calls.is_empty() {
            let visible = strip_tool_calls(&reply);
            if visible.trim().is_empty() {
                if !retried_empty_final {
                    conv_history.push(rig::completion::Message::user(&current_message));
                    conv_history.push(rig::completion::Message::assistant(&reply));
                    current_message = empty_final_retry_prompt(message, &current_message);
                    retried_empty_final = true;
                    continue;
                }

                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "assistant".to_string(),
                        chunk: EMPTY_FINAL_REPLY.to_string(),
                    },
                );
                final_reply = EMPTY_FINAL_REPLY.to_string();
                break;
            }
            final_reply = visible;
            // No tool calls → task is done
            break;
        }

        let visible = strip_tool_calls(&reply);
        final_reply = visible;

        // ── Execute tools ─────────────────────────────────────────────────────
        let mut results_block = String::new();
        for call in &calls {
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "assistant".to_string(),
                    chunk: format!("{}...", describe_tool(call)),
                },
            );

            let result =
                execute_tool(call, &sandbox, Arc::clone(&config), agent_bus.clone(), tx).await;

            let status_icon = if result.success { "✓" } else { "✗" };
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "assistant".to_string(),
                    chunk: format!("  {} [{}]", status_icon, result.call),
                },
            );

            // Append to results block for next LLM turn
            results_block.push_str(&format!(
                "<tool_result>\n<call>{}</call>\n<output>{}</output>\n</tool_result>\n",
                escape_tool_result(&result.call),
                escape_tool_result(&result.output)
            ));
        }

        // ── Prepare next turn: push assistant reply + tool results ────────────
        conv_history.push(rig::completion::Message::user(&current_message));
        conv_history.push(rig::completion::Message::assistant(&reply));

        current_message = format!(
            "Here are the tool results:\n{}\nContinue the task if more tools are needed. Otherwise, provide the final answer in natural language. Do not include tool_call XML.",
            results_block
        );
    }

    if final_reply.trim().is_empty() {
        send(
            tx,
            TuiEvent::TokenChunk {
                agent: "assistant".to_string(),
                chunk: EMPTY_FINAL_REPLY.to_string(),
            },
        );
        final_reply = EMPTY_FINAL_REPLY.to_string();
    }

    Ok(final_reply)
}

fn send(tx: &TuiSender, event: TuiEvent) {
    let _ = tx.send(event);
}

async fn assistant_preamble(config: &Arc<RwLock<Config>>, prompt: &str) -> String {
    let cfg = config.read().await;
    let mut preamble = PREAMBLE.to_string();
    if cfg.tools.skills_enabled {
        let skill_context = crate::skills::context_for_prompt(
            "assistant",
            prompt,
            cfg.tools.max_skill_context_chars,
        )
        .unwrap_or_default();
        if !skill_context.is_empty() {
            preamble.push_str(&skill_context);
        }
    }
    let project_context = crate::project_context::load_agents_context(16_000);
    if project_context.is_empty() {
        preamble
    } else {
        format!("{preamble}{project_context}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_write_file_call() {
        let text = r#"Sure, let me write it.
<tool_call>
<name>write_file</name>
<path>hello.txt</path>
<content>
Hello, world!
</content>
</tool_call>
Done."#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        if let ToolCall::WriteFile { path, content } = &calls[0] {
            assert_eq!(path, "hello.txt");
            assert_eq!(content, "Hello, world!");
        } else {
            panic!("Expected WriteFile");
        }
    }

    #[test]
    fn parses_multiple_calls() {
        let text = r#"
<tool_call><name>read_file</name><path>src/main.rs</path></tool_call>
<tool_call><name>list_files</name><path>src</path></tool_call>
"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn parses_run_command() {
        let text = r#"
<tool_call>
<name>run_command</name>
<command>git</command>
<args>status</args>
</tool_call>
"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        if let ToolCall::RunCommand { command, args } = &calls[0] {
            assert_eq!(command, "git");
            assert_eq!(args, "status");
        } else {
            panic!("Expected RunCommand");
        }
    }

    #[test]
    fn parses_web_search_call() {
        let text = r#"
<tool_call>
<name>web_search</name>
<query>agentic coding tools news</query>
</tool_call>
"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        if let ToolCall::WebSearch { query } = &calls[0] {
            assert_eq!(query, "agentic coding tools news");
        } else {
            panic!("Expected WebSearch");
        }
    }

    #[test]
    fn strips_tool_calls_from_visible_text() {
        let text = r#"I'll inspect that.
<tool_call><name>read_file</name><path>AGENTS.md</path></tool_call>
Then I will summarize."#;

        let visible = strip_tool_calls(text);
        assert_eq!(visible, "I'll inspect that.\nThen I will summarize.");
        assert!(!visible.contains("<tool_call>"));
        assert!(!visible.contains("read_file"));
    }

    #[test]
    fn strips_only_tool_call_when_no_visible_text() {
        let text = "<tool_call><name>read_file</name><path>AGENTS.md</path></tool_call>";
        assert_eq!(strip_tool_calls(text), "");
    }

    #[test]
    fn empty_final_retry_prompt_keeps_request_and_context() {
        let prompt = empty_final_retry_prompt(
            "je veux apprendre les concepts avances de rust",
            "Here are the tool results...",
        );

        assert!(prompt.contains("je veux apprendre les concepts avances de rust"));
        assert!(prompt.contains("Here are the tool results..."));
        assert!(prompt.contains("no visible text"));
        assert!(prompt.contains("do not include tool_call XML"));
    }

    #[test]
    fn escapes_tool_results_for_feedback() {
        let escaped = escape_tool_result("<name>read_file</name> & data");
        assert_eq!(escaped, "&lt;name&gt;read_file&lt;/name&gt; &amp; data");
    }

    #[test]
    fn empty_text_yields_no_calls() {
        assert!(parse_tool_calls("No tools here, just text.").is_empty());
    }
}
