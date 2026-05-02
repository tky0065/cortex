/// Agentic assistant with tool-use capability.
///
/// Implements a ReAct-style loop: the LLM reasons, emits `<tool_call>` blocks,
/// the runtime executes them and feeds results back, until the task is done or
/// the iteration cap is reached.
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::agent_bus::{AgentBus, AgentDirective, AgentStatus};
use crate::config::Config;
use crate::tools::filesystem::FileSystem;
use crate::tools::terminal;
use crate::tui::events::{Task, TuiEvent, TuiSender};

const MAX_ITERATIONS: usize = 15;
const MAX_EMPTY_RETRIES: u8 = 2;
const EMPTY_FINAL_REPLY: &str = "Je n'ai pas recu de reponse texte du modele. Reessaie avec un autre modele ou reformule la demande.";

/// System prompt describing the assistant's capabilities and tool format.
pub const PREAMBLE: &str = "\
You are Cortex, a powerful general-purpose AI assistant — similar to GitHub Copilot or Claude. \
Your goal is to assist the user with any task, whether it is coding, research, writing, or general analysis. \
You are an expert in many domains and can adapt your tone to the user's needs.\n\
\n\
## IDENTITY & SCOPE\n\
- You are NOT just a coding assistant; you are a multi-domain intelligence.\n\
- You can answer questions about people, news, history, science, and technology.\n\
- You have access to the web, files, and local system commands.\n\
\n\
## TOOL USAGE GUIDELINES\n\
- **Proactive Research**: For any subject where you lack 100% certainty (specific people, latest news, obscure libraries), ALWAYS use `web_search` or `fetch_url`. Do not hallucinate or rely on outdated training data.\n\
- **Surgical Actions**: Use `write_file` and `run_command` to perform real-world tasks. Always verify your work.\n\
- **Transparency**: Do not show raw tool XML to the user. Explain what you are doing in natural language.\n\
\n\
## AVAILABLE TOOLS\n\
\n\
Use tools by emitting XML blocks in your response:\n\
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
### Run a command (allowed: cargo, go, npm, pip, git, docker, gh, curl, jq, rg)\n\
<tool_call>\n\
<name>run_command</name>\n\
<command>git</command>\n\
<args>status</args>\n\
</tool_call>\n\
\n\
### Fetch a URL directly\n\
Use this when the user gives a specific http(s) URL or asks for information from a known page.\n\
<tool_call>\n\
<name>fetch_url</name>\n\
<url>https://example.com/docs</url>\n\
</tool_call>\n\
\n\
### Search the web\n\
Use this for ANY topic you are not 100% certain about (people, projects, events, news).\n\
<tool_call>\n\
<name>web_search</name>\n\
<query>latest news about rust 2024</query>\n\
</tool_call>\n\
\n\
### Query agent status\n\
Check the status of workflow agents.\n\
<tool_call>\n\
<name>query_agent_status</name>\n\
<agent>developer</agent>\n\
</tool_call>\n\
\n\
### Delegate a task\n\
Run a standalone workflow agent (ceo, pm, tech_lead, developer, qa, devops).\n\
<tool_call>\n\
<name>delegate_to_agent</name>\n\
<agent>developer</agent>\n\
<instruction>Implement the feature according to the specs</instruction>\n\
</tool_call>\n\
\n\
### Inject a directive\n\
Steer a running agent at the next boundary.\n\
<tool_call>\n\
<name>inject_directive</name>\n\
<agent>qa</agent>\n\
<instruction>Focus on security edge cases</instruction>\n\
</tool_call>\n\
\n\
## CORE RULES\n\
1. **Always use web_search** for unknown subjects. Never say 'I don't know' without searching first.\n\
2. For complex tasks, create and maintain a `TASKS.md` file using `- [ ]` checklist format.\n\
3. Prefer direct evidence (files, command output, web results) over general knowledge.\n\
4. Keep your final answers concise and helpful.\n\
\n\
## IMPORTANT — Tool calls are OPTIONAL\n\
- For simple questions (greetings, general knowledge, short explanations), respond directly in plain text. Do NOT emit XML tool blocks.\n\
- Only use tool calls when you genuinely need to read a file, run a command, or search the web for information you don't already have.\n\
- A plain text response is always valid. If you are uncertain whether to use a tool, err on the side of answering directly.\n\
- Web search context may already be provided in the message — use it directly without calling web_search again.\n\
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
    FetchUrl {
        url: String,
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

struct AssistantTaskTracker {
    enabled: bool,
    using_file_tasks: bool,
    completed_default_tasks: usize,
    custom_tasks: Option<Vec<String>>,
    completed_custom_tasks: usize,
}

const ASSISTANT_DEFAULT_TASKS: &[&str] = &[
    "Comprendre la demande",
    "Executer les actions necessaires",
    "Verifier le resultat",
    "Resumer le resultat",
];

impl AssistantTaskTracker {
    fn new(message: &str) -> Self {
        Self {
            enabled: should_track_assistant_task(message),
            using_file_tasks: false,
            completed_default_tasks: 0,
            custom_tasks: None,
            completed_custom_tasks: 0,
        }
    }

    fn maybe_publish_initial(&self, tx: &TuiSender) {
        if self.enabled {
            publish_default_assistant_tasks(tx, self.completed_default_tasks);
        }
    }

    fn ensure_started_for_tools(&mut self, calls: &[ToolCall], tx: &TuiSender) {
        if self.enabled || !calls_indicate_tracked_work(calls) {
            return;
        }

        self.enabled = true;
        self.completed_default_tasks = 0;
        publish_default_assistant_tasks(tx, self.completed_default_tasks);
    }

    fn mark_understood(&mut self, tx: &TuiSender) {
        self.mark_default_progress(1, tx);
    }

    fn begin_delegate_task(&mut self, agent: &str, tx: &TuiSender) {
        if !self.enabled || self.using_file_tasks || self.custom_tasks.is_some() {
            return;
        }

        self.custom_tasks = Some(vec![
            "Comprendre la demande".to_string(),
            format!("Deleguer la tache a {agent}"),
            "Integrer le resultat de l'agent".to_string(),
            "Resumer le resultat".to_string(),
        ]);
        self.completed_custom_tasks = 1;
        self.publish_custom_tasks(tx);
    }

    fn observe_tool_result(&mut self, call: &ToolCall, result: &ToolResult, tx: &TuiSender) {
        if !self.enabled || !result.success {
            return;
        }

        if let ToolCall::WriteFile { path, content } = call
            && is_tasks_file(path)
        {
            let tasks = parse_checklist_tasks(content);
            if !tasks.is_empty() {
                self.using_file_tasks = true;
                let _ = tx.send(TuiEvent::TasksUpdated { tasks });
                return;
            }
        }

        if self.using_file_tasks {
            return;
        }

        if matches!(call, ToolCall::DelegateToAgent { .. }) {
            self.mark_custom_progress(2, tx);
            return;
        }

        if tool_call_is_verification(call) {
            self.mark_default_progress(3, tx);
        } else if tool_call_is_action(call) {
            self.mark_default_progress(2, tx);
        }
    }

    fn finish(&mut self, tx: &TuiSender) {
        if !self.enabled || self.using_file_tasks {
            return;
        }

        if self.custom_tasks.is_some() {
            self.mark_custom_progress(usize::MAX, tx);
        } else {
            self.mark_default_progress(ASSISTANT_DEFAULT_TASKS.len(), tx);
        }
    }

    fn mark_default_progress(&mut self, completed_count: usize, tx: &TuiSender) {
        if !self.enabled || self.using_file_tasks || self.custom_tasks.is_some() {
            return;
        }

        let capped = completed_count.min(ASSISTANT_DEFAULT_TASKS.len());
        if capped <= self.completed_default_tasks {
            return;
        }

        self.completed_default_tasks = capped;
        publish_default_assistant_tasks(tx, self.completed_default_tasks);
    }

    fn mark_custom_progress(&mut self, completed_count: usize, tx: &TuiSender) {
        if !self.enabled || self.using_file_tasks {
            return;
        }

        let Some(tasks) = &self.custom_tasks else {
            return;
        };
        let capped = completed_count.min(tasks.len());
        if capped <= self.completed_custom_tasks {
            return;
        }

        self.completed_custom_tasks = capped;
        self.publish_custom_tasks(tx);
    }

    fn publish_custom_tasks(&self, tx: &TuiSender) {
        let Some(descriptions) = &self.custom_tasks else {
            return;
        };
        let tasks = descriptions
            .iter()
            .enumerate()
            .map(|(index, description)| Task {
                description: description.clone(),
                is_done: index < self.completed_custom_tasks,
            })
            .collect();
        let _ = tx.send(TuiEvent::TasksUpdated { tasks });
    }
}

fn publish_default_assistant_tasks(tx: &TuiSender, completed_count: usize) {
    let tasks = crate::workflows::build_phase_tasks(ASSISTANT_DEFAULT_TASKS, completed_count);
    let _ = tx.send(TuiEvent::TasksUpdated { tasks });
}

fn should_track_assistant_task(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    let action_words = [
        "implement",
        "build",
        "create",
        "fix",
        "debug",
        "refactor",
        "test",
        "add",
        "write",
        "update",
        "run",
        "find",
        "research",
        "fetch",
        "retrieve",
        "code",
        "implemente",
        "cree",
        "corrige",
        "ajoute",
        "modifie",
        "ecris",
        "execute",
        "teste",
        "cherche",
        "recherche",
        "retrouve",
        "recupere",
        "continue",
    ];
    let has_action = action_words.iter().any(|word| lower.contains(word));
    let looks_multi_step = lower.contains(" et ")
        || lower.contains(" puis ")
        || lower.contains(" ensuite ")
        || lower.contains("&&")
        || lower.lines().count() > 1
        || lower.len() > 120;

    has_action && looks_multi_step
}

fn calls_indicate_tracked_work(calls: &[ToolCall]) -> bool {
    calls.iter().any(tool_call_is_action)
}

fn tool_call_is_action(call: &ToolCall) -> bool {
    matches!(
        call,
        ToolCall::WriteFile { .. }
            | ToolCall::FetchUrl { .. }
            | ToolCall::RunCommand { .. }
            | ToolCall::DelegateToAgent { .. }
            | ToolCall::InjectDirective { .. }
    )
}

fn tool_call_is_verification(call: &ToolCall) -> bool {
    match call {
        ToolCall::RunCommand { command, args } => {
            let command = command.as_str();
            let args = args.to_ascii_lowercase();
            matches!(command, "cargo" | "go" | "npm" | "pip" | "docker")
                && (args.contains("test")
                    || args.contains("check")
                    || args.contains("clippy")
                    || args.contains("fmt")
                    || args.contains("build")
                    || args.contains("lint"))
        }
        _ => false,
    }
}

fn is_tasks_file(path: &str) -> bool {
    path.rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("TASKS.md"))
}

fn parse_checklist_tasks(content: &str) -> Vec<Task> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(desc) = line.strip_prefix("- [ ] ") {
                Some(Task {
                    description: desc.to_string(),
                    is_done: false,
                })
            } else {
                line.strip_prefix("- [x] ")
                    .or_else(|| line.strip_prefix("- [X] "))
                    .map(|desc| Task {
                        description: desc.to_string(),
                        is_done: true,
                    })
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// XML parser for <tool_call> blocks
// ---------------------------------------------------------------------------

/// Extract all `<tool_call>` blocks from an LLM response and parse them.
fn parse_tool_calls(text: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let mut remaining = text;

    // 1. Try standard <tool_call> wrappers first
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

    // 2. Fallback: Scan for any bare tool tags if no standard wrappers were found
    if calls.is_empty() {
        let known_tools = [
            "web_search",
            "write_file",
            "read_file",
            "list_files",
            "run_command",
            "fetch_url",
            "query_agent_status",
            "delegate_to_agent",
            "inject_directive",
        ];

        for tool in known_tools {
            let mut search_text = text;
            while let Some(content) = extract_tag(search_text, tool) {
                if let Some(call) = parse_json_call(tool, content) {
                    calls.push(call);
                }
                // Move past this tag to find others of the same type if necessary
                let open = format!("<{}>", tool);
                let close = format!("</{}>", tool);
                if let Some(start) = search_text.find(&open) {
                    let after_open = &search_text[start + open.len()..];
                    if let Some(end) = after_open.find(&close) {
                        search_text = &after_open[end + close.len()..];
                        continue;
                    }
                }
                break;
            }
        }
    }

    calls
}

fn extract_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    
    let start_idx = text.find(&open)?;
    let after_open = &text[start_idx + open.len()..];
    let end_idx = after_open.find(&close)?;
    
    Some(after_open[..end_idx].trim())
}

fn parse_single_call(block: &str) -> Option<ToolCall> {
    // Standard format: <name>tool_name</name> ... args
    if let Some(name) = extract_tag(block, "name") {
        return parse_named_call(name, block);
    }

    // Non-standard fallback: <tool_name>{json}</tool_name>
    let known_tools = [
        "web_search",
        "write_file",
        "read_file",
        "list_files",
        "run_command",
        "fetch_url",
        "query_agent_status",
        "delegate_to_agent",
        "inject_directive",
    ];

    for tool in known_tools {
        if let Some(content) = extract_tag(block, tool) {
            return parse_json_call(tool, content);
        }
    }

    None
}

fn parse_named_call(name: &str, block: &str) -> Option<ToolCall> {
    match name {
        "write_file" => {
            let path = extract_tag(block, "path")?.to_string();
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
        "fetch_url" => {
            let url = extract_tag(block, "url")?.to_string();
            Some(ToolCall::FetchUrl { url })
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

fn parse_json_call(name: &str, content: &str) -> Option<ToolCall> {
    // Attempt JSON parsing first
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        match name {
            "write_file" => return Some(ToolCall::WriteFile {
                path: v["path"].as_str()?.to_string(),
                content: v["content"].as_str()?.to_string(),
            }),
            "read_file" => return Some(ToolCall::ReadFile {
                path: v["path"].as_str()?.to_string(),
            }),
            "list_files" => return Some(ToolCall::ListFiles {
                path: v["path"].as_str().unwrap_or(".").to_string(),
            }),
            "run_command" => return Some(ToolCall::RunCommand {
                command: v["command"].as_str()?.to_string(),
                args: v["args"].as_str().unwrap_or("").to_string(),
            }),
            "fetch_url" => return Some(ToolCall::FetchUrl {
                url: v["url"].as_str()?.to_string(),
            }),
            "web_search" => return Some(ToolCall::WebSearch {
                query: v["query"].as_str()?.to_string(),
            }),
            "query_agent_status" => return Some(ToolCall::QueryAgentStatus {
                agent: v["agent"].as_str().map(|s| s.to_string()),
            }),
            "delegate_to_agent" => return Some(ToolCall::DelegateToAgent {
                agent: v["agent"].as_str()?.to_string(),
                instruction: v["instruction"].as_str()?.to_string(),
            }),
            "inject_directive" => return Some(ToolCall::InjectDirective {
                agent: v["agent"].as_str()?.to_string(),
                instruction: v["instruction"].as_str()?.to_string(),
            }),
            _ => {}
        }
    }

    // Fallback: If JSON parsing fails, treat content as raw text (especially useful for web_search)
    match name {
        "web_search" => Some(ToolCall::WebSearch { query: content.to_string() }),
        "fetch_url" => Some(ToolCall::FetchUrl { url: content.to_string() }),
        "read_file" => Some(ToolCall::ReadFile { path: content.to_string() }),
        "list_files" => Some(ToolCall::ListFiles { path: content.to_string() }),
        "run_command" => Some(ToolCall::RunCommand { command: content.to_string(), args: "".to_string() }),
        _ => None,
    }
}

pub(crate) fn strip_tool_calls_for_display(text: &str) -> String {
    let mut output = String::new();
    let mut remaining = text;

    // 1. Strip standard <tool_call>...</tool_call> blocks (including everything inside)
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

    // 2. Strip known tool tags and their content (fallback for non-standard or orphan blocks)
    let tool_tags = [
        ("web_search", "</web_search>"),
        ("write_file", "</write_file>"),
        ("read_file", "</read_file>"),
        ("list_files", "</list_files>"),
        ("run_command", "</run_command>"),
        ("fetch_url", "</fetch_url>"),
        ("query_agent_status", "</query_agent_status>"),
        ("delegate_to_agent", "</delegate_to_agent>"),
        ("inject_directive", "</inject_directive>"),
    ];

    let mut current = output;
    for (open_name, close_tag) in tool_tags {
        let open_tag = format!("<{}>", open_name);
        while let Some(start) = current.find(&open_tag) {
            let mut result = current[..start].to_string();
            let after_open = &current[start + open_tag.len()..];
            if let Some(end) = after_open.find(close_tag) {
                result.push_str(&after_open[end + close_tag.len()..]);
            } else {
                // Orphan open tag — strip until end of string
            }
            current = result;
        }
    }

    // 3. Final cleanup: strip any remaining orphan tags
    let orphan_tags = [
        "<tool_call>",
        "</tool_call>",
        "<name>",
        "</name>",
        "<path>",
        "</path>",
        "<content>",
        "</content>",
        "<command>",
        "</command>",
        "<args>",
        "</args>",
        "<url>",
        "</url>",
        "<query>",
        "</query>",
        "<agent>",
        "</agent>",
        "<instruction>",
        "</instruction>",
    ];

    for tag in orphan_tags {
        current = current.replace(tag, "");
    }

    current
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

fn validate_fetch_url(url: &str) -> Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url)?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        scheme => {
            anyhow::bail!("unsupported URL scheme '{scheme}'; only http and https are allowed")
        }
    }
}

async fn fetch_url_content(url: &str) -> ToolResult {
    let display = format!("fetch_url({url})");
    let parsed = match validate_fetch_url(url) {
        Ok(parsed) => parsed,
        Err(e) => {
            return ToolResult {
                call: display,
                output: format!("error: {e}"),
                success: false,
            };
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(format!("cortex/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return ToolResult {
                call: display,
                output: format!("error: failed to build HTTP client: {e}"),
                success: false,
            };
        }
    };

    match client.get(parsed).send().await {
        Ok(response) => {
            let status = response.status();
            let final_url = response.url().to_string();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            match response.text().await {
                Ok(body) => {
                    let text = normalize_fetched_text(&body, content_type.as_str());
                    ToolResult {
                        call: display,
                        output: format!(
                            "status: {status}\nurl: {final_url}\ncontent-type: {content_type}\n\n{}",
                            truncate_chars(&text, 60_000)
                        ),
                        success: status.is_success(),
                    }
                }
                Err(e) => ToolResult {
                    call: display,
                    output: format!("error: failed to read response body: {e}"),
                    success: false,
                },
            }
        }
        Err(e) => ToolResult {
            call: display,
            output: format!("error: request failed: {e}"),
            success: false,
        },
    }
}

fn normalize_fetched_text(body: &str, content_type: &str) -> String {
    if content_type.contains("html") {
        strip_html_for_model(body)
    } else {
        body.to_string()
    }
}

fn strip_html_for_model(html: &str) -> String {
    let mut output = String::with_capacity(html.len().min(60_000));
    let mut in_tag = false;
    let mut last_was_space = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                if !last_was_space {
                    output.push(' ');
                    last_was_space = true;
                }
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            _ if ch.is_whitespace() => {
                if !last_was_space {
                    output.push(' ');
                    last_was_space = true;
                }
            }
            _ => {
                output.push(ch);
                last_was_space = false;
            }
        }
    }

    decode_basic_html_entities(output.trim())
}

fn decode_basic_html_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}\n\n... (truncated)")
    } else {
        truncated
    }
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
                        agent: "cortex".to_string(),
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
        ToolCall::FetchUrl { url } => fetch_url_content(url).await,
        ToolCall::WebSearch { query } => {
            let display = format!("web_search({})", query);
            let (brave_enabled, has_key) = {
                let config = config.read().await;
                (config.tools.web_search_enabled, config.api_keys.web_search.is_some())
            };

            if brave_enabled && has_key {
                // Use Brave Search API when configured.
                let config_guard = config.read().await;
                let context = crate::tools::web_search::fetch_context(query, &config_guard).await;
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
            } else {
                // Fallback: DuckDuckGo Lite (no API key required).
                let context = crate::tools::web_search::search_without_key(query).await;
                let success = !context.trim().is_empty();
                ToolResult {
                    call: display,
                    output: if success {
                        context
                    } else {
                        format!(
                            "error: web search returned no results for '{}'. Try fetch_url with a direct URL, or gh for GitHub entities.",
                            query
                        )
                    },
                    success,
                }
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

            // Guard: cannot delegate to self.
            if agent == "cortex" || agent == "assistant" {
                return ToolResult {
                    call: display,
                    output: "error: cannot delegate to yourself. \
                             For research or lookup questions use web_search. \
                             For direct GitHub data use run_command with gh. \
                             delegate_to_agent is only for workflow agents: \
                             ceo, pm, tech_lead, developer, qa, devops."
                        .to_string(),
                    success: false,
                };
            }

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
                execution_mode: crate::workflows::ExecutionMode::Auto,
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
        ToolCall::FetchUrl { url } => format!("fetching URL {}", url),
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
    cancel: CancellationToken,
) -> Result<(String, Vec<rig::completion::Message>)> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let sandbox = FileSystem::new(&cwd);

    // Build a mutable conversation: system preamble is handled via `complete_chat`,
    // so we just accumulate user + assistant turns here.
    let mut conv_history = history;
    let mut current_message = message.to_string();
    let mut final_reply = String::new();
    let mut empty_retry_count: u8 = 0;
    let mut task_tracker = AssistantTaskTracker::new(message);
    task_tracker.maybe_publish_initial(tx);

    for iteration in 0..MAX_ITERATIONS {
        // ── Cancellation check ────────────────────────────────────────────────
        if cancel.is_cancelled() {
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "cortex".to_string(),
                    chunk: "  Aborted.".to_string(),
                },
            );
            final_reply = "Aborted.".to_string();
            break;
        }

        // ── LLM call ──────────────────────────────────────────────────────────
        send(
            tx,
            TuiEvent::AgentProgress {
                agent: "cortex".to_string(),
                message: if iteration == 0 {
                    "Thinking…".to_string()
                } else {
                    format!("Iteration {}…", iteration + 1)
                },
            },
        );

        let config_snapshot = config.read().await.clone();

        // Auto-inject web search context at the first iteration so even models
        // that cannot emit tool-call XML benefit from fresh web results.
        if iteration == 0 {
            let search_query: String = message.chars().take(200).collect();
            let web_context =
                crate::tools::web_search::fetch_context(&search_query, &config_snapshot).await;
            if !web_context.is_empty() {
                current_message = format!("{}\n{}", current_message, web_context);
            }
        }

        // Use the full preamble on the first pass; fall back to a minimal preamble
        // on retries so that small models with limited context windows can still
        // produce a plain-text answer.
        let preamble = if empty_retry_count == 0 {
            assistant_preamble(&config, &current_message).await
        } else {
            "You are Cortex, a helpful AI assistant. Answer the user's question concisely in plain text. Do not use XML tags or tool calls.".to_string()
        };

        let llm_fut = crate::providers::complete_chat_stream(
            model,
            &preamble,
            conv_history.clone(),
            &current_message,
            &config_snapshot,
            tx,
            "cortex",
        );
        let reply = tokio::select! {
            res = llm_fut => res?,
            () = cancel.cancelled() => {
                send(tx, TuiEvent::TokenChunk {
                    agent: "cortex".to_string(),
                    chunk: "  Aborted.".to_string(),
                });
                final_reply = "Aborted.".to_string();
                break;
            }
            () = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                send(tx, TuiEvent::TokenChunk {
                    agent: "cortex".to_string(),
                    chunk: "  Request timed out after 120s. Try a faster model.".to_string(),
                });
                final_reply = "Request timed out.".to_string();
                break;
            }
        };

        // ── Parse tool calls ─────────────────────────────────────────────────
        let calls = parse_tool_calls(&reply);
        if calls.is_empty() {
            let visible = strip_tool_calls(&reply);
            if visible.trim().is_empty() {
                if empty_retry_count < MAX_EMPTY_RETRIES {
                    conv_history.push(rig::completion::Message::user(&current_message));
                    conv_history.push(rig::completion::Message::assistant(&reply));
                    current_message = if empty_retry_count == 0 {
                        empty_final_retry_prompt(message, &current_message)
                    } else {
                        format!("Réponds simplement à : {}", message)
                    };
                    empty_retry_count += 1;
                    continue;
                }

                send(
                    tx,
                    TuiEvent::TokenChunk {
                        agent: "cortex".to_string(),
                        chunk: EMPTY_FINAL_REPLY.to_string(),
                    },
                );
                final_reply = EMPTY_FINAL_REPLY.to_string();
                break;
            }
            final_reply = visible;
            task_tracker.finish(tx);
            // Record this turn so the next prompt inherits the full context.
            conv_history.push(rig::completion::Message::user(message));
            conv_history.push(rig::completion::Message::assistant(&final_reply));
            break;
        }

        task_tracker.ensure_started_for_tools(&calls, tx);
        task_tracker.mark_understood(tx);

        let visible = strip_tool_calls(&reply);
        final_reply = visible;

        // ── Execute tools ─────────────────────────────────────────────────────
        let mut results_block = String::new();
        for call in &calls {
            if let ToolCall::DelegateToAgent { agent, .. } = call {
                task_tracker.begin_delegate_task(agent, tx);
            }

            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "cortex".to_string(),
                    chunk: format!("{}...", describe_tool(call)),
                },
            );

            let result =
                execute_tool(call, &sandbox, Arc::clone(&config), agent_bus.clone(), tx).await;
            task_tracker.observe_tool_result(call, &result, tx);

            let status_icon = if result.success { "✓" } else { "✗" };
            send(
                tx,
                TuiEvent::TokenChunk {
                    agent: "cortex".to_string(),
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
                agent: "cortex".to_string(),
                chunk: EMPTY_FINAL_REPLY.to_string(),
            },
        );
        final_reply = EMPTY_FINAL_REPLY.to_string();
    }

    task_tracker.finish(tx);

    Ok((final_reply, conv_history))
}

fn send(tx: &TuiSender, event: TuiEvent) {
    let _ = tx.send(event);
}

async fn assistant_preamble(config: &Arc<RwLock<Config>>, prompt: &str) -> String {
    let cfg = config.read().await;
    let mut preamble = PREAMBLE.to_string();
    if cfg.tools.skills_enabled {
        let skill_context = crate::skills::context_for_prompt(
            "cortex",
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
    fn parses_fetch_url_call() {
        let text = r#"
<tool_call>
<name>fetch_url</name>
<url>https://example.com/docs</url>
</tool_call>
"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        if let ToolCall::FetchUrl { url } = &calls[0] {
            assert_eq!(url, "https://example.com/docs");
        } else {
            panic!("Expected FetchUrl");
        }
    }

    #[test]
    fn fetch_url_validation_accepts_http_and_https_only() {
        assert!(validate_fetch_url("https://example.com").is_ok());
        assert!(validate_fetch_url("http://example.com").is_ok());
        assert!(validate_fetch_url("file:///etc/passwd").is_err());
        assert!(validate_fetch_url("ssh://example.com/repo").is_err());
    }

    #[test]
    fn html_fetch_normalization_strips_tags_and_decodes_basic_entities() {
        let text = normalize_fetched_text(
            "<html><body><h1>Title &amp; Docs</h1><p>Hello&nbsp;world</p></body></html>",
            "text/html; charset=utf-8",
        );

        assert!(text.contains("Title & Docs"));
        assert!(text.contains("Hello world"));
        assert!(!text.contains("<h1>"));
    }

    #[test]
    fn web_search_no_results_message_names_fallbacks() {
        // When DDG returns no results, the error message should mention direct fallbacks.
        let msg = format!(
            "error: web search returned no results for '{}'. Try fetch_url with a direct URL, or gh for GitHub entities.",
            "test query"
        );
        assert!(msg.contains("fetch_url"));
        assert!(msg.contains("gh"));
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

    #[test]
    fn assistant_task_tracking_ignores_simple_chat() {
        assert!(!should_track_assistant_task("c'est quoi rust ?"));
    }

    #[test]
    fn assistant_task_tracking_detects_multistep_work() {
        assert!(should_track_assistant_task(
            "implemente la feature et ajoute les tests"
        ));
    }

    #[test]
    fn assistant_task_tracking_detects_url_research_work() {
        assert!(should_track_assistant_task(
            "retrouve les infos depuis https://example.com et mets-les dans info.md"
        ));
    }

    #[test]
    fn parses_tasks_md_checklist_for_tui() {
        let tasks = parse_checklist_tasks(
            r#"
# Plan
- [x] Lire le code
- [ ] Ajouter la feature
- [X] Tester
"#,
        );

        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].description, "Lire le code");
        assert!(tasks[0].is_done);
        assert!(!tasks[1].is_done);
        assert!(tasks[2].is_done);
    }

    #[test]
    fn parses_bare_tool_tags_with_raw_text() {
        let text = r#"Check this: <web_search>enokdev developer</web_search>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            ToolCall::WebSearch { query } => assert_eq!(query, "enokdev developer"),
            _ => panic!("Expected WebSearch call with raw text"),
        }
    }

    #[test]
    fn parses_bare_tool_tags_without_wrapper() {
        let text = r#"Certainly! Here is the search: <web_search>{"query":"enokdev"}</web_search>"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            ToolCall::WebSearch { query } => assert_eq!(query, "enokdev"),
            _ => panic!("Expected WebSearch call"),
        }
    }

    #[test]
    fn parses_non_standard_json_xml_calls() {
        let text = r#"
<tool_call>
<web_search>{"query":"enokdev"}</web_search>
</tool_call>
"#;
        let calls = parse_tool_calls(text);
        assert_eq!(calls.len(), 1);
        match &calls[0] {
            ToolCall::WebSearch { query } => assert_eq!(query, "enokdev"),
            _ => panic!("Expected WebSearch call"),
        }
    }

    #[test]
    fn strip_tool_calls_handles_orphan_tags() {
        let text = "Result: <web_search>{\"query\":\"enokdev\"}</web_search></tool_call>";
        let visible = strip_tool_calls(text);
        assert_eq!(visible, "Result:");
    }

    #[test]
    fn assistant_delegate_task_publishes_agent_specific_tasks() {
        let (tx, mut rx) = crate::tui::events::channel();
        let mut tracker = AssistantTaskTracker::new("implemente la feature et ajoute les tests");

        tracker.maybe_publish_initial(&tx);
        tracker.begin_delegate_task("developer", &tx);

        let _ = rx.try_recv().expect("initial tasks event");
        let event = rx.try_recv().expect("delegate tasks event");
        match event {
            TuiEvent::TasksUpdated { tasks } => {
                assert_eq!(tasks.len(), 4);
                assert_eq!(tasks[1].description, "Deleguer la tache a developer");
                assert!(tasks[0].is_done);
                assert!(!tasks[1].is_done);
            }
            other => panic!("expected tasks event, got {other:?}"),
        }
    }

    #[test]
    fn tasks_file_detection_accepts_nested_tasks_md() {
        assert!(is_tasks_file("TASKS.md"));
        assert!(is_tasks_file("docs/TASKS.md"));
        assert!(is_tasks_file("docs/tasks.md"));
        assert!(!is_tasks_file("docs/tasks.txt"));
    }
}
