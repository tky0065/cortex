use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::FutureExt;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::tui::events::{TuiEvent, TuiSender, channel};
use crate::workflows::RunOptions;

const START_MARKER: &str = "<!-- cortex:init:start -->";
const END_MARKER: &str = "<!-- cortex:init:end -->";
const MAX_FILES: usize = 600;
const MAX_MANIFEST_CHARS: usize = 24_000;
const MAX_PROMPT_CHARS: usize = 60_000;

#[derive(Debug, Clone)]
pub struct InitOutcome {
    pub path: PathBuf,
    pub created: bool,
    pub files_scanned: usize,
    pub detected_stack: Vec<String>,
    pub used_fallback: bool,
}

#[derive(Debug, Clone)]
struct ProjectScan {
    root: PathBuf,
    files_scanned: usize,
    tree: Vec<String>,
    manifests: BTreeMap<String, String>,
    languages: BTreeMap<String, usize>,
    stack: BTreeSet<String>,
    commands: BTreeSet<String>,
}

pub async fn init_current_project(
    config: Arc<Config>,
    tx: Option<TuiSender>,
    force: bool,
) -> Result<InitOutcome> {
    let root = std::env::current_dir().context("failed to resolve current directory")?;
    let tx = tx.unwrap_or_else(|| channel().0);

    send(
        &tx,
        TuiEvent::AgentStarted {
            agent: "init".to_string(),
        },
    );
    send(
        &tx,
        TuiEvent::AgentProgress {
            agent: "init".to_string(),
            message: format!("Scanning project at {}", root.display()),
        },
    );

    let scan = scan_project(&root)?;
    send(
        &tx,
        TuiEvent::AgentProgress {
            agent: "init".to_string(),
            message: format!("Scanned {} files", scan.files_scanned),
        },
    );

    let previous_panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let generated_result =
        std::panic::AssertUnwindSafe(generate_agents_md(&scan, config, tx.clone()))
            .catch_unwind()
            .await;
    std::panic::set_hook(previous_panic_hook);
    let (generated, used_fallback) = match generated_result {
        Ok(Ok(raw_generated)) if is_valid_generated_context(&raw_generated) => {
            (raw_generated, false)
        }
        Ok(Ok(_)) => {
            send(
                &tx,
                TuiEvent::TokenChunk {
                    agent: "init".to_string(),
                    chunk: "LLM returned empty or unusable output; wrote deterministic AGENTS.md instead.".to_string(),
                },
            );
            (deterministic_agents_md(&scan), true)
        }
        Ok(Err(error)) => {
            send(
                &tx,
                TuiEvent::TokenChunk {
                    agent: "init".to_string(),
                    chunk: format!(
                        "LLM generation failed ({error}); wrote deterministic AGENTS.md instead."
                    ),
                },
            );
            (deterministic_agents_md(&scan), true)
        }
        Err(_) => {
            send(
                &tx,
                TuiEvent::TokenChunk {
                    agent: "init".to_string(),
                    chunk: "LLM generation panicked; wrote deterministic AGENTS.md instead."
                        .to_string(),
                },
            );
            (deterministic_agents_md(&scan), true)
        }
    };
    let path = root.join("AGENTS.md");
    let created = !path.exists();
    let existing = fs::read_to_string(&path).ok();
    let merged = merge_agents_md(existing.as_deref(), &generated, force);
    fs::write(&path, merged).with_context(|| format!("failed to write {}", path.display()))?;

    send(
        &tx,
        TuiEvent::TokenChunk {
            agent: "init".to_string(),
            chunk: format!(
                "{} AGENTS.md at {}",
                if created { "Created" } else { "Updated" },
                path.display()
            ),
        },
    );
    send(
        &tx,
        TuiEvent::AgentDone {
            agent: "init".to_string(),
        },
    );

    Ok(InitOutcome {
        path,
        created,
        files_scanned: scan.files_scanned,
        detected_stack: scan.stack.into_iter().collect(),
        used_fallback,
    })
}

pub fn load_agents_context(max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let Ok(root) = std::env::current_dir() else {
        return String::new();
    };
    let path = root.join("AGENTS.md");
    let Ok(content) = fs::read_to_string(path) else {
        return String::new();
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let context = format!(
        "\n\n## Project Context from AGENTS.md\nUse these project-specific instructions before planning or changing files.\n\n{}\n",
        trimmed
    );
    take_chars(&context, max_chars)
}

fn scan_project(root: &Path) -> Result<ProjectScan> {
    let mut scan = ProjectScan {
        root: root.to_path_buf(),
        files_scanned: 0,
        tree: Vec::new(),
        manifests: BTreeMap::new(),
        languages: BTreeMap::new(),
        stack: BTreeSet::new(),
        commands: BTreeSet::new(),
    };
    visit_dir(root, root, 0, &mut scan)?;
    infer_stack_and_commands(&mut scan);
    Ok(scan)
}

fn visit_dir(root: &Path, dir: &Path, depth: usize, scan: &mut ProjectScan) -> Result<()> {
    if scan.files_scanned >= MAX_FILES {
        return Ok(());
    }

    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if scan.files_scanned >= MAX_FILES {
            break;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if should_skip(&name, &path) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        if path.is_dir() {
            if depth <= 3 {
                scan.tree.push(format!("{}/", rel));
            }
            visit_dir(root, &path, depth + 1, scan)?;
            continue;
        }

        if !path.is_file() {
            continue;
        }

        scan.files_scanned += 1;
        scan.tree.push(rel.clone());
        if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
            *scan.languages.entry(ext.to_ascii_lowercase()).or_insert(0) += 1;
        }
        if is_manifest(&name) || rel == "README.md" || rel.starts_with("docs/") {
            if let Ok(content) = fs::read_to_string(&path) {
                let remaining = MAX_MANIFEST_CHARS.saturating_sub(
                    scan.manifests
                        .values()
                        .map(|value| value.len())
                        .sum::<usize>(),
                );
                if remaining > 0 {
                    scan.manifests.insert(rel, take_chars(&content, remaining));
                }
            }
        }
    }

    Ok(())
}

fn should_skip(name: &str, path: &Path) -> bool {
    const SKIP_NAMES: &[&str] = &[
        ".git",
        ".agents",
        ".claude",
        ".codex",
        ".cortex",
        ".hg",
        ".svn",
        ".env",
        ".env.local",
        ".env.production",
        "target",
        "node_modules",
        "dist",
        "build",
        ".next",
        ".cache",
        "coverage",
        "vendor",
        "cortex-output",
    ];
    if SKIP_NAMES.contains(&name) {
        return true;
    }
    if name.ends_with(".lock") && name != "Cargo.lock" {
        return true;
    }
    if looks_sensitive(name) {
        return true;
    }
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
                    | "ico"
                    | "pdf"
                    | "zip"
                    | "gz"
                    | "tar"
                    | "bin"
                    | "exe"
                    | "dylib"
                    | "so"
                    | "dll"
            )
        })
        .unwrap_or(false)
}

fn looks_sensitive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
}

fn is_manifest(name: &str) -> bool {
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "pom.xml"
            | "build.gradle"
            | "settings.gradle"
            | "Makefile"
            | "justfile"
            | "docker-compose.yml"
            | "Dockerfile"
            | "README.md"
            | "AGENTS.md"
            | "CLAUDE.md"
    )
}

fn infer_stack_and_commands(scan: &mut ProjectScan) {
    if has_manifest(scan, "Cargo.toml") {
        scan.stack.insert("Rust".to_string());
        scan.commands.insert("cargo check".to_string());
        scan.commands.insert("cargo test".to_string());
        scan.commands
            .insert("cargo clippy -- -D warnings".to_string());
        scan.commands.insert("cargo fmt".to_string());
    }
    if has_manifest(scan, "package.json") {
        scan.stack.insert("Node.js".to_string());
        scan.commands.insert("npm test".to_string());
        scan.commands.insert("npm run build".to_string());
    }
    if has_manifest(scan, "pyproject.toml") {
        scan.stack.insert("Python".to_string());
        scan.commands.insert("pytest".to_string());
    }
    if has_manifest(scan, "go.mod") {
        scan.stack.insert("Go".to_string());
        scan.commands.insert("go test ./...".to_string());
    }

    for (ext, count) in &scan.languages {
        if *count == 0 {
            continue;
        }
        match ext.as_str() {
            "rs" => {
                scan.stack.insert("Rust".to_string());
            }
            "ts" | "tsx" => {
                scan.stack.insert("TypeScript".to_string());
            }
            "js" | "jsx" => {
                scan.stack.insert("JavaScript".to_string());
            }
            "py" => {
                scan.stack.insert("Python".to_string());
            }
            "go" => {
                scan.stack.insert("Go".to_string());
            }
            _ => {}
        }
    }
}

fn has_manifest(scan: &ProjectScan, name: &str) -> bool {
    scan.manifests
        .keys()
        .any(|path| path == name || path.ends_with(&format!("/{name}")))
}

async fn generate_agents_md(
    scan: &ProjectScan,
    config: Arc<Config>,
    tx: TuiSender,
) -> Result<String> {
    let model = crate::providers::model_for_role("assistant", &config)?.to_string();
    let prompt = build_generation_prompt(scan);
    let (resume_tx, resume_rx) = tokio::sync::mpsc::channel::<()>(1);
    let (answer_tx, answer_rx) = tokio::sync::mpsc::channel::<String>(1);
    let options = RunOptions {
        auto: true,
        config,
        tx,
        project_dir: scan.root.clone(),
        cancel: CancellationToken::new(),
        resume_tx: Arc::new(resume_tx),
        resume_rx: Arc::new(tokio::sync::Mutex::new(resume_rx)),
        answer_tx: Arc::new(answer_tx),
        answer_rx: Arc::new(tokio::sync::Mutex::new(answer_rx)),
        verbose: false,
        agent_bus: None,
    };
    crate::providers::complete(&model, GENERATOR_PREAMBLE, &prompt, &options, "init").await
}

const GENERATOR_PREAMBLE: &str = "\
You generate AGENTS.md files for coding agents.\n\
Write concise, durable project instructions based only on the supplied scan.\n\
Do not include secrets. Do not invent commands or architecture not supported by evidence.\n\
Use Markdown. Return only the content that belongs inside the Cortex-generated section.\n\
";

fn build_generation_prompt(scan: &ProjectScan) -> String {
    let mut prompt = String::new();
    prompt.push_str("Create a project context guide for coding agents.\n\n");
    prompt.push_str("Required sections:\n");
    prompt.push_str("- Project Overview\n- Stack & Tooling\n- Architecture Map\n- Commands\n- Coding Conventions\n- Agent Instructions\n- Known Constraints\n\n");
    prompt.push_str("Detected stack:\n");
    for item in &scan.stack {
        prompt.push_str(&format!("- {item}\n"));
    }
    prompt.push_str("\nSuggested commands:\n");
    for command in &scan.commands {
        prompt.push_str(&format!("- `{command}`\n"));
    }
    prompt.push_str("\nFile tree sample:\n");
    for line in scan.tree.iter().take(220) {
        prompt.push_str(&format!("- {line}\n"));
    }
    prompt.push_str("\nManifest and documentation excerpts:\n");
    for (path, content) in &scan.manifests {
        prompt.push_str(&format!("\n## {path}\n```text\n{}\n```\n", content.trim()));
    }
    take_chars(&prompt, MAX_PROMPT_CHARS)
}

fn is_valid_generated_context(generated: &str) -> bool {
    let trimmed = generated.trim();
    if trimmed.chars().count() < 80 {
        return false;
    }
    let without_markers = trimmed
        .replace(START_MARKER, "")
        .replace(END_MARKER, "")
        .trim()
        .to_string();
    if without_markers.chars().count() < 80 {
        return false;
    }
    [
        "Project Overview",
        "Stack",
        "Architecture",
        "Commands",
        "Agent Instructions",
    ]
    .iter()
    .any(|section| without_markers.contains(section))
}

fn deterministic_agents_md(scan: &ProjectScan) -> String {
    let project_name = scan
        .root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("this project");
    let stack = list_or_unknown(scan.stack.iter().map(String::as_str));
    let commands = list_or_default(
        scan.commands.iter().map(String::as_str),
        "- No standard command was detected. Inspect the project manifests before running checks.",
        true,
    );
    let manifests = list_or_default(
        scan.manifests.keys().map(String::as_str),
        "- No common manifest or README file was detected.",
        false,
    );
    let tree = list_or_default(
        scan.tree.iter().take(80).map(String::as_str),
        "- No project files were detected by the scanner.",
        false,
    );

    format!(
        "\
## Project Overview

This AGENTS.md was generated by `cortex init` from a deterministic project scan because the model returned unusable output. The project root is `{project_name}`.

## Stack & Tooling

Detected stack: {stack}.

Detected manifests and documentation:
{manifests}

## Architecture Map

Scanned {files_scanned} files, excluding generated artifacts, dependencies, VCS metadata, binary files, and likely secrets.

{tree}

## Commands

Run the most relevant checks before finishing changes:
{commands}

## Coding Conventions

- Follow the conventions already present in the existing files.
- Prefer small, focused changes over broad rewrites.
- Keep generated artifacts, dependency folders, caches, and build outputs out of source edits unless explicitly requested.

## Agent Instructions

- Read this file before planning or changing code in this project.
- Inspect the relevant source, manifest, and documentation files before editing.
- Preserve manual content outside the Cortex-managed section.
- Do not read, print, copy, or summarize secrets from `.env`, key, token, credential, or certificate files.

## Known Constraints

- The scanner uses a bounded project sample, so verify assumptions against the actual files before making large changes.
- If the detected commands are incomplete, derive the correct checks from the project manifests.
",
        files_scanned = scan.files_scanned
    )
}

fn list_or_unknown<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let values = items
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        "unknown".to_string()
    } else {
        values.join(", ")
    }
}

fn list_or_default<'a>(
    items: impl Iterator<Item = &'a str>,
    default_line: &str,
    code: bool,
) -> String {
    let values = items
        .filter(|item| !item.trim().is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return default_line.to_string();
    }
    values
        .into_iter()
        .map(|item| {
            if code {
                format!("- `{item}`")
            } else {
                format!("- {item}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn merge_agents_md(existing: Option<&str>, generated: &str, _force: bool) -> String {
    let generated = generated.trim();
    let block = format!("{START_MARKER}\n{generated}\n{END_MARKER}");
    let Some(existing) = existing else {
        return format!("# AGENTS.md\n\n{block}\n");
    };
    if let (Some(start), Some(end)) = (existing.find(START_MARKER), existing.find(END_MARKER)) {
        let end = end + END_MARKER.len();
        let mut out = String::new();
        out.push_str(existing[..start].trim_end());
        out.push_str("\n\n");
        out.push_str(&block);
        out.push('\n');
        let tail = existing[end..].trim_start();
        if !tail.is_empty() {
            out.push('\n');
            out.push_str(tail);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        return out;
    }
    let mut out = existing.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(&block);
    out.push('\n');
    out
}

fn take_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn send(tx: &TuiSender, event: TuiEvent) {
    let _ = tx.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scan() -> ProjectScan {
        ProjectScan {
            root: PathBuf::from("sample-app"),
            files_scanned: 4,
            tree: vec![
                "Cargo.toml".to_string(),
                "src/".to_string(),
                "src/main.rs".to_string(),
                "README.md".to_string(),
            ],
            manifests: BTreeMap::from([(
                "Cargo.toml".to_string(),
                "[package]\nname = \"sample-app\"".to_string(),
            )]),
            languages: BTreeMap::from([("rs".to_string(), 1)]),
            stack: BTreeSet::from(["Rust".to_string()]),
            commands: BTreeSet::from(["cargo check".to_string(), "cargo test".to_string()]),
        }
    }

    #[test]
    fn merge_creates_marked_file_when_missing() {
        let merged = merge_agents_md(None, "## Project Overview\nTest", false);
        assert!(merged.contains("# AGENTS.md"));
        assert!(merged.contains(START_MARKER));
        assert!(merged.contains("## Project Overview"));
        assert!(merged.contains(END_MARKER));
    }

    #[test]
    fn merge_replaces_only_marked_section() {
        let existing =
            format!("manual before\n\n{START_MARKER}\nold\n{END_MARKER}\n\nmanual after\n");
        let merged = merge_agents_md(Some(&existing), "new", false);
        assert!(merged.contains("manual before"));
        assert!(merged.contains("manual after"));
        assert!(merged.contains("new"));
        assert!(!merged.contains("\nold\n"));
    }

    #[test]
    fn merge_appends_when_no_markers_exist() {
        let merged = merge_agents_md(Some("# Manual\nKeep this"), "generated", false);
        assert!(merged.starts_with("# Manual\nKeep this"));
        assert!(merged.contains(START_MARKER));
        assert!(merged.contains("generated"));
    }

    #[test]
    fn rejects_empty_generated_context() {
        assert!(!is_valid_generated_context(""));
        assert!(!is_valid_generated_context("   \n"));
        assert!(!is_valid_generated_context(&format!(
            "{START_MARKER}\n\n{END_MARKER}"
        )));
    }

    #[test]
    fn accepts_useful_generated_context() {
        let generated = "## Project Overview\nThis project has enough useful context for agents.\n\n## Commands\nRun `cargo check` and `cargo test` before finishing changes.";
        assert!(is_valid_generated_context(generated));
    }

    #[test]
    fn deterministic_fallback_contains_required_sections() {
        let fallback = deterministic_agents_md(&sample_scan());
        for section in [
            "## Project Overview",
            "## Stack & Tooling",
            "## Architecture Map",
            "## Commands",
            "## Coding Conventions",
            "## Agent Instructions",
            "## Known Constraints",
        ] {
            assert!(fallback.contains(section), "missing section {section}");
        }
        assert!(fallback.contains("Rust"));
        assert!(fallback.contains("cargo check"));
    }

    #[test]
    fn sensitive_files_are_skipped() {
        assert!(should_skip(".env", Path::new(".env")));
        assert!(should_skip(".claude", Path::new(".claude")));
        assert!(should_skip(".cortex", Path::new(".cortex")));
        assert!(should_skip("api.secret", Path::new("api.secret")));
        assert!(should_skip("private.pem", Path::new("private.pem")));
    }

    #[test]
    fn detects_nested_manifest_names() {
        let mut scan = sample_scan();
        scan.manifests.clear();
        scan.manifests
            .insert("services/api/Cargo.toml".to_string(), String::new());
        assert!(has_manifest(&scan, "Cargo.toml"));
        assert!(!has_manifest(&scan, "package.json"));
    }
}
