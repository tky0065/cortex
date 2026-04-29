/// Agentic assistant with tool-use capability.
///
/// Implements a ReAct-style loop: the LLM reasons, emits `<tool_call>` blocks,
/// the runtime executes them and feeds results back, until the task is done or
/// the iteration cap is reached.
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::Config;
use crate::tools::filesystem::FileSystem;
use crate::tools::terminal;
use crate::tui::events::{TuiEvent, TuiSender};

const MAX_ITERATIONS: usize = 15;

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
## RULES\n\
- For casual chat or general non-current knowledge, answer directly without tools.\n\
- For current news or time-sensitive facts, use web_search first.\n\
- For repo-specific questions, read the relevant files first.\n\
- For file edits or commands, use tools to complete the task.\n\
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
    WriteFile { path: String, content: String },
    ReadFile { path: String },
    ListFiles { path: String },
    RunCommand { command: String, args: String },
    WebSearch { query: String },
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
        _ => None,
    }
}

fn strip_tool_calls(text: &str) -> String {
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

async fn execute_tool(call: &ToolCall, sandbox: &FileSystem) -> ToolResult {
    match call {
        ToolCall::WriteFile { path, content } => {
            let display = format!("write_file({})", path);
            match sandbox.write(path, content) {
                Ok(()) => ToolResult {
                    call: display,
                    output: format!("✓ wrote {} bytes to '{}'", content.len(), path),
                    success: true,
                },
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
) -> Result<String> {
    let _ = config; // may be used in future for sandbox config

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let sandbox = FileSystem::new(&cwd);

    // Build a mutable conversation: system preamble is handled via `complete_chat`,
    // so we just accumulate user + assistant turns here.
    let mut conv_history = history;
    let mut current_message = message.to_string();
    let mut final_reply = String::new();

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

        let reply = crate::providers::complete_chat(
            model,
            PREAMBLE,
            conv_history.clone(),
            &current_message,
        )
        .await?;

        // Stream the LLM reply to the TUI
        for line in reply.lines() {
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "assistant".to_string(),
                    chunk: line.to_string(),
                },
            );
        }

        final_reply = reply.clone();

        // ── Parse tool calls ─────────────────────────────────────────────────
        let calls = parse_tool_calls(&reply);
        if calls.is_empty() {
            // No tool calls → task is done
            break;
        }

        // ── Execute tools ─────────────────────────────────────────────────────
        let mut results_block = String::new();
        for call in &calls {
            let result = execute_tool(call, &sandbox).await;

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
                result.call, result.output
            ));

            // Show first 3 lines of output as a log hint
            let preview: String = result
                .output
                .lines()
                .take(3)
                .collect::<Vec<_>>()
                .join(" | ");
            if !preview.is_empty() {
                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "assistant".to_string(),
                        chunk: format!("    → {}", preview),
                    },
                );
            }
        }

        // ── Prepare next turn: push assistant reply + tool results ────────────
        conv_history.push(rig::completion::Message::user(&current_message));
        conv_history.push(rig::completion::Message::assistant(&reply));

        current_message = format!(
            "Here are the tool results:\n{}\nContinue the task or summarize if complete.",
            results_block
        );
    }

    Ok(final_reply)
}

fn send(tx: &TuiSender, event: TuiEvent) {
    let _ = tx.send(event);
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
    fn empty_text_yields_no_calls() {
        assert!(parse_tool_calls("No tools here, just text.").is_empty());
    }
}
