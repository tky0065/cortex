use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

const REGISTRY_FILE: &str = "registry.json";
const SKILL_FILE: &str = "SKILL.md";
const SKILLS_CLI_TIMEOUT_SECS: u64 = 45;
static PUBLIC_CATALOG_CACHE: OnceLock<Vec<RemoteSkill>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    Global,
    Project,
}

impl SkillScope {
    fn as_str(self) -> &'static str {
        match self {
            SkillScope::Global => "global",
            SkillScope::Project => "project",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillsCliCommand {
    pub program: String,
    pub args: Vec<String>,
    pub display: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub name: String,
    pub description: String,
    pub source: String,
    pub enabled: bool,
    pub scope: SkillScope,
    pub path: PathBuf,
    pub installed_at: String,
}

#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSkill {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub installs: u64,
    #[serde(default)]
    pub source_type: String,
    #[serde(default)]
    pub install_url: Option<String>,
    #[serde(default)]
    pub is_duplicate: bool,
}

#[derive(Debug, Deserialize)]
struct SkillListResponse {
    data: Vec<RemoteSkill>,
}

#[derive(Debug, Deserialize)]
struct SkillSearchResponse {
    data: Vec<RemoteSkill>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmbeddedSkill {
    source: String,
    skill_id: String,
    name: String,
    #[serde(default)]
    installs: u64,
}

#[derive(Debug, Deserialize)]
struct SkillDetailResponse {
    files: Option<Vec<RemoteSkillFile>>,
}

#[derive(Debug, Deserialize)]
struct RemoteSkillFile {
    path: String,
    contents: String,
}

#[derive(Debug, Deserialize)]
struct GitHubTreeResponse {
    tree: Vec<GitHubTreeItem>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubTreeItem {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub scope: Option<SkillScope>,
    pub skill: Option<String>,
    pub all: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CreateOptions {
    pub scope: Option<SkillScope>,
    pub description: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SkillRegistry {
    #[serde(default)]
    skills: Vec<SkillRecord>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    metadata: SkillMetadata,
}

#[derive(Debug, Default, Deserialize)]
struct SkillMetadata {
    #[serde(default)]
    tags: Vec<String>,
}

pub fn run_command(args: &[String]) -> Result<Vec<String>> {
    if args.is_empty() {
        return Ok(help_lines());
    }

    match args[0].as_str() {
        "list" | "ls" => list_command(),
        "show" => {
            let name = args.get(1).context("usage: skill show <name>")?;
            show(name)
        }
        "add" | "install" => add_command(&args[1..]),
        "enable" | "activate" => set_enabled_command(&args[1..], true),
        "disable" | "deactivate" => set_enabled_command(&args[1..], false),
        "remove" | "delete" | "rm" | "supprimer" => remove_command(&args[1..]),
        "create" | "new" => create_command(&args[1..]),
        "help" | "--help" | "-h" => Ok(help_lines()),
        other => bail!("unknown skill command '{other}'. Try: skill help"),
    }
}

pub fn parse_command_line(input: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote: Option<char> = None;

    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            None if ch == '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            None => current.push(ch),
        }
    }

    if quote.is_some() {
        bail!("unterminated quoted argument");
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

pub async fn add_from_source(source: &str, options: AddOptions) -> Result<Vec<SkillRecord>> {
    let scope = options.scope.unwrap_or_else(default_scope);
    let candidates = if Path::new(source).exists() {
        find_skill_dirs(Path::new(source))?
    } else if let Some(candidates) =
        try_import_github_source(source, options.skill.as_deref()).await?
    {
        candidates
    } else {
        import_with_skills_cli(source, options.skill.as_deref()).await?
    };

    let selected = select_candidates(candidates, options.skill.as_deref(), options.all)?;
    let mut records = Vec::new();
    for skill_dir in selected {
        let definition = load_definition(&skill_dir)?;
        validate_skill_name(&definition.name)?;
        install_skill(&definition, source, scope, options.enabled)?;
        records.push(record_for(&definition, source, scope, options.enabled)?);
    }
    Ok(records)
}

pub async fn fetch_catalog(view: &str, page: u32, per_page: u32) -> Result<Vec<RemoteSkill>> {
    let client = reqwest::Client::new();
    let request = client.get("https://skills.sh/api/v1/skills").query(&[
        ("view", view.to_string()),
        ("page", page.to_string()),
        ("per_page", per_page.min(500).to_string()),
    ]);

    if let Ok(response) = send_skills_request(request).await
        && response.status().is_success()
    {
        let parsed = response
            .json::<SkillListResponse>()
            .await
            .context("failed to parse skills.sh catalog")?;
        return Ok(parsed.data);
    }

    let mut fallback = fetch_public_catalog().await?;
    let start = (page as usize).saturating_mul(per_page as usize);
    let end = start.saturating_add(per_page as usize).min(fallback.len());
    if start >= fallback.len() {
        return Ok(Vec::new());
    }
    Ok(fallback.drain(start..end).collect())
}

pub async fn search_remote_skills(query: &str) -> Result<Vec<RemoteSkill>> {
    if query.trim().chars().count() < 2 {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    let request = client
        .get("https://skills.sh/api/v1/skills/search")
        .query(&[("q", query.trim()), ("limit", "100")]);

    if let Ok(response) = send_skills_request(request).await
        && response.status().is_success()
    {
        let parsed = response
            .json::<SkillSearchResponse>()
            .await
            .context("failed to parse skills.sh search results")?;
        return Ok(parsed.data);
    }

    let query = query.trim().to_ascii_lowercase();
    let terms = query
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    let results = fetch_public_catalog()
        .await?
        .into_iter()
        .filter(|skill| {
            let haystack = format!("{} {}", skill.name, skill.source).to_ascii_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .take(100)
        .collect();
    Ok(results)
}

pub async fn install_remote_skill(
    remote: &RemoteSkill,
    scope: Option<SkillScope>,
) -> Result<SkillRecord> {
    let scope = scope.unwrap_or_else(default_scope);
    if github_repo_for_remote(remote).is_some()
        && let Ok(record) = install_github_remote_skill(remote, scope).await
    {
        return Ok(record);
    }

    if let Ok(detail) = fetch_remote_detail(&remote.id).await
        && let Some(files) = detail.files
        && !files.is_empty()
    {
        let skill_dir = materialize_remote_files(&remote.id, &files)?;
        let candidates = find_skill_dirs(&skill_dir)?;
        let selected = select_candidates(candidates, None, false)?;
        let definition = load_definition(&selected[0])?;
        let source = format!("skills.sh:{}", remote.id);
        install_skill(&definition, &source, scope, true)?;
        return record_for(&definition, &source, scope, true);
    }

    let install_url = remote
        .install_url
        .as_deref()
        .context("skills.sh did not provide files or an install URL for this skill")?;
    let records = add_from_source(
        install_url,
        AddOptions {
            scope: Some(scope),
            skill: Some(remote.slug.clone()),
            all: false,
            enabled: true,
        },
    )
    .await?;
    records
        .into_iter()
        .next()
        .context("remote skill installation produced no records")
}

pub fn remote_install_display(remote: &RemoteSkill) -> String {
    if let Some((owner, repo)) = github_repo_for_remote(remote) {
        return format!("GitHub {owner}/{repo} --skill {}", remote.slug);
    }
    let source = remote.install_url.as_deref().unwrap_or(&remote.source);
    skills_cli_command(source, Some(&remote.slug)).display
}

pub fn skills_cli_command(source: &str, skill: Option<&str>) -> SkillsCliCommand {
    let mut args = vec![
        "--yes".to_string(),
        "skills".to_string(),
        "add".to_string(),
        source.to_string(),
    ];
    if let Some(skill) = skill {
        args.push("--skill".to_string());
        args.push(skill.to_string());
    }
    let display = format!("npx {}", args.join(" "));
    SkillsCliCommand {
        program: "npx".to_string(),
        args,
        display,
    }
}

pub fn create(name: &str, options: CreateOptions) -> Result<SkillRecord> {
    validate_skill_name(name)?;
    let scope = options.scope.unwrap_or_else(default_scope);
    let root = skills_root(scope)?;
    let skill_dir = root.join(name);
    if skill_dir.exists() {
        bail!("skill '{name}' already exists in {} scope", scope.as_str());
    }
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;

    let description = if options.description.trim().is_empty() {
        format!("Custom Cortex skill for {name}.")
    } else {
        options.description.trim().to_string()
    };
    let content = format!(
        "---\nname: {name}\ndescription: \"{}\"\nmetadata:\n  version: \"0.1.0\"\n---\n\n# {name}\n\n## When to Use\n\nUse this skill when the task needs this capability.\n\n## Instructions\n\n- Replace this section with concrete steps.\n- Keep instructions actionable and specific.\n",
        escape_yaml_string(&description)
    );
    fs::write(skill_dir.join(SKILL_FILE), content)
        .with_context(|| format!("failed to write {}", skill_dir.join(SKILL_FILE).display()))?;

    let definition = load_definition(&skill_dir)?;
    install_registry_record(&definition, "created", scope, true)?;
    record_for(&definition, "created", scope, true)
}

pub fn set_enabled(name: &str, scope: Option<SkillScope>, enabled: bool) -> Result<SkillRecord> {
    validate_skill_name(name)?;
    let scopes = target_scopes(scope);
    for candidate_scope in scopes {
        let root = skills_root(candidate_scope)?;
        let mut registry = load_registry(&root)?;
        if let Some(record) = registry.skills.iter_mut().find(|s| s.name == name) {
            record.enabled = enabled;
            let updated = record.clone();
            save_registry(&root, &registry)?;
            return Ok(updated);
        }
    }
    bail!("skill '{name}' was not found")
}

pub fn remove(name: &str, scope: Option<SkillScope>) -> Result<SkillRecord> {
    validate_skill_name(name)?;
    let scopes = target_scopes(scope);
    for candidate_scope in scopes {
        let root = skills_root(candidate_scope)?;
        let mut registry = load_registry(&root)?;
        if let Some(idx) = registry.skills.iter().position(|s| s.name == name) {
            let record = registry.skills.remove(idx);
            save_registry(&root, &registry)?;
            if record.path.exists() {
                fs::remove_dir_all(&record.path)
                    .with_context(|| format!("failed to remove {}", record.path.display()))?;
            }
            return Ok(record);
        }
    }
    bail!("skill '{name}' was not found")
}

pub fn list() -> Result<Vec<SkillRecord>> {
    let mut records = Vec::new();
    records.extend(load_scope_records(SkillScope::Global)?);
    if project_root().is_some() {
        records.extend(load_scope_records(SkillScope::Project)?);
    }

    let mut by_name: HashMap<String, SkillRecord> = HashMap::new();
    for record in records {
        match by_name.get(&record.name) {
            Some(existing) if existing.scope == SkillScope::Project => {}
            _ => {
                by_name.insert(record.name.clone(), record);
            }
        }
    }

    let mut values = by_name.into_values().collect::<Vec<_>>();
    values.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(values)
}

pub fn show(name: &str) -> Result<Vec<String>> {
    let record = list()?
        .into_iter()
        .find(|record| record.name == name)
        .with_context(|| format!("skill '{name}' was not found"))?;
    let definition = load_definition(&record.path)?;
    Ok(vec![
        format!("name: {}", record.name),
        format!("description: {}", record.description),
        format!("enabled: {}", record.enabled),
        format!("scope: {}", record.scope.as_str()),
        format!("source: {}", record.source),
        format!("path: {}", record.path.display()),
        String::new(),
        definition.content,
    ])
}

pub fn context_for_prompt(agent_name: &str, prompt: &str, max_chars: usize) -> Result<String> {
    let records = list()?
        .into_iter()
        .filter(|record| record.enabled)
        .collect::<Vec<_>>();
    if records.is_empty() || max_chars == 0 {
        return Ok(String::new());
    }

    let mut definitions = Vec::new();
    for record in &records {
        if let Ok(definition) = load_definition(&record.path) {
            definitions.push((record, definition));
        }
    }
    if definitions.is_empty() {
        return Ok(String::new());
    }

    let mut output = String::from("\n\n## Active Cortex Skills\n");
    output.push_str("Use these reusable skills when they are relevant to the task. A compact catalog follows:\n");
    for (record, definition) in &definitions {
        output.push_str(&format!(
            "- {} [{}]: {}\n",
            record.name,
            record.scope.as_str(),
            first_non_empty(&definition.description, &record.description)
        ));
    }

    let mut scored = definitions
        .into_iter()
        .map(|(record, definition)| {
            let score = relevance_score(&definition, agent_name, prompt);
            (score, record.clone(), definition)
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    let mut remaining = max_chars.saturating_sub(output.chars().count());
    if remaining == 0 {
        return Ok(take_chars(&output, max_chars));
    }

    let relevant = scored
        .into_iter()
        .filter(|(score, _, _)| *score > 0)
        .take(3)
        .collect::<Vec<_>>();
    if !relevant.is_empty() {
        output.push_str("\nRelevant skill instructions:\n");
    }
    for (_, record, definition) in relevant {
        let block = format!("\n### {}\n{}\n", record.name, definition.content);
        let block_len = block.chars().count();
        if block_len > remaining {
            output.push_str(&take_chars(&block, remaining));
            break;
        }
        remaining -= block_len;
        output.push_str(&block);
    }

    Ok(output)
}

fn add_command(args: &[String]) -> Result<Vec<String>> {
    let parsed = parse_add_args(args)?;
    let runtime = tokio::runtime::Handle::try_current()
        .context("skill add must run inside an async runtime")?;
    let records = runtime.block_on(add_from_source(&parsed.0, parsed.1))?;
    Ok(records
        .into_iter()
        .map(|record| {
            format!(
                "added {} [{}] enabled={}",
                record.name,
                record.scope.as_str(),
                record.enabled
            )
        })
        .collect())
}

pub async fn add_command_async(args: &[String]) -> Result<Vec<String>> {
    let parsed = parse_add_args(args)?;
    let records = add_from_source(&parsed.0, parsed.1).await?;
    Ok(records
        .into_iter()
        .map(|record| {
            format!(
                "added {} [{}] enabled={}",
                record.name,
                record.scope.as_str(),
                record.enabled
            )
        })
        .collect())
}

pub async fn run_command_async(args: &[String]) -> Result<Vec<String>> {
    if args.first().map(String::as_str) == Some("add")
        || args.first().map(String::as_str) == Some("install")
    {
        return add_command_async(&args[1..]).await;
    }
    run_command(args)
}

fn list_command() -> Result<Vec<String>> {
    let records = list()?;
    if records.is_empty() {
        return Ok(vec!["no skills installed".to_string()]);
    }
    Ok(records
        .into_iter()
        .map(|record| {
            format!(
                "{} [{}] {} - {}",
                record.name,
                record.scope.as_str(),
                if record.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                record.description
            )
        })
        .collect())
}

fn set_enabled_command(args: &[String], enabled: bool) -> Result<Vec<String>> {
    let (name, scope) = parse_name_scope(args, "skill enable <name> [--global|--project]")?;
    let record = set_enabled(&name, scope, enabled)?;
    Ok(vec![format!(
        "{} {} [{}]",
        if enabled { "enabled" } else { "disabled" },
        record.name,
        record.scope.as_str()
    )])
}

fn remove_command(args: &[String]) -> Result<Vec<String>> {
    let (name, scope) = parse_name_scope(args, "skill remove <name> [--global|--project]")?;
    let record = remove(&name, scope)?;
    Ok(vec![format!(
        "removed {} [{}]",
        record.name,
        record.scope.as_str()
    )])
}

fn create_command(args: &[String]) -> Result<Vec<String>> {
    if args.is_empty() {
        bail!("usage: skill create <name> --description <text> [--global|--project]");
    }
    let name = args[0].clone();
    let mut scope = None;
    let mut description = String::new();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--global" => scope = Some(SkillScope::Global),
            "--project" => scope = Some(SkillScope::Project),
            "--description" | "-d" => {
                i += 1;
                description = args
                    .get(i)
                    .context("--description requires a value")?
                    .to_string();
            }
            other => bail!("unknown option '{other}'"),
        }
        i += 1;
    }
    let record = create(&name, CreateOptions { scope, description })?;
    Ok(vec![format!(
        "created {} [{}] at {}",
        record.name,
        record.scope.as_str(),
        record.path.display()
    )])
}

fn parse_add_args(args: &[String]) -> Result<(String, AddOptions)> {
    if args.is_empty() {
        bail!(
            "usage: skill add <owner/repo|path|url> [--skill <name>] [--global|--project] [--all]"
        );
    }
    let source = args[0].clone();
    let mut options = AddOptions {
        enabled: true,
        ..AddOptions::default()
    };
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--global" => options.scope = Some(SkillScope::Global),
            "--project" => options.scope = Some(SkillScope::Project),
            "--skill" => {
                i += 1;
                options.skill = Some(args.get(i).context("--skill requires a value")?.to_string());
            }
            "--all" => options.all = true,
            "--disabled" => options.enabled = false,
            other => bail!("unknown option '{other}'"),
        }
        i += 1;
    }
    Ok((source, options))
}

fn parse_name_scope(args: &[String], usage: &str) -> Result<(String, Option<SkillScope>)> {
    if args.is_empty() {
        bail!("usage: {usage}");
    }
    let name = args[0].clone();
    let mut scope = None;
    for arg in &args[1..] {
        match arg.as_str() {
            "--global" => scope = Some(SkillScope::Global),
            "--project" => scope = Some(SkillScope::Project),
            other => bail!("unknown option '{other}'"),
        }
    }
    Ok((name, scope))
}

fn install_skill(
    definition: &SkillDefinition,
    source: &str,
    scope: SkillScope,
    enabled: bool,
) -> Result<()> {
    let root = skills_root(scope)?;
    let dest = root.join(&definition.name);
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .with_context(|| format!("failed to replace {}", dest.display()))?;
    }
    copy_dir_all(definition.path.parent().unwrap_or(&definition.path), &dest)?;
    install_registry_record(definition, source, scope, enabled)
}

fn install_registry_record(
    definition: &SkillDefinition,
    source: &str,
    scope: SkillScope,
    enabled: bool,
) -> Result<()> {
    let root = skills_root(scope)?;
    fs::create_dir_all(&root).with_context(|| format!("failed to create {}", root.display()))?;
    let mut registry = load_registry(&root)?;
    registry
        .skills
        .retain(|record| record.name != definition.name);
    registry
        .skills
        .push(record_for(definition, source, scope, enabled)?);
    registry.skills.sort_by(|a, b| a.name.cmp(&b.name));
    save_registry(&root, &registry)
}

fn record_for(
    definition: &SkillDefinition,
    source: &str,
    scope: SkillScope,
    enabled: bool,
) -> Result<SkillRecord> {
    Ok(SkillRecord {
        name: definition.name.clone(),
        description: definition.description.clone(),
        source: source.to_string(),
        enabled,
        scope,
        path: skills_root(scope)?.join(&definition.name),
        installed_at: Utc::now().to_rfc3339(),
    })
}

fn load_scope_records(scope: SkillScope) -> Result<Vec<SkillRecord>> {
    let root = skills_root(scope)?;
    let mut registry = load_registry(&root)?;
    let mut known = registry
        .skills
        .iter()
        .map(|record| record.name.clone())
        .collect::<HashSet<_>>();

    if root.exists() {
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            if !path.is_dir() || !path.join(SKILL_FILE).exists() {
                continue;
            }
            let definition = load_definition(&path)?;
            if known.insert(definition.name.clone()) {
                registry
                    .skills
                    .push(record_for(&definition, "manual", scope, true)?);
            }
        }
    }

    registry
        .skills
        .retain(|record| record.path.join(SKILL_FILE).exists());
    Ok(registry.skills)
}

fn load_registry(root: &Path) -> Result<SkillRegistry> {
    let path = root.join(REGISTRY_FILE);
    if !path.exists() {
        return Ok(SkillRegistry::default());
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_registry(root: &Path, registry: &SkillRegistry) -> Result<()> {
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;
    let path = root.join(REGISTRY_FILE);
    let raw = serde_json::to_string_pretty(registry)?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
}

fn load_definition(skill_dir: &Path) -> Result<SkillDefinition> {
    let path = if skill_dir.ends_with(SKILL_FILE) {
        skill_dir.to_path_buf()
    } else {
        skill_dir.join(SKILL_FILE)
    };
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let (frontmatter, body) = split_frontmatter(&content)?;
    let metadata: SkillFrontmatter =
        serde_yaml::from_str(frontmatter).context("failed to parse SKILL.md frontmatter")?;
    validate_skill_name(&metadata.name)?;
    if metadata.description.len() > 500 {
        bail!(
            "skill '{}' description exceeds 500 characters",
            metadata.name
        );
    }
    Ok(SkillDefinition {
        name: metadata.name,
        description: metadata.description,
        tags: metadata.metadata.tags,
        path,
        content: body.trim().to_string(),
    })
}

fn split_frontmatter(content: &str) -> Result<(&str, &str)> {
    let content = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"))
        .context("SKILL.md must start with YAML frontmatter")?;
    let marker = if let Some(idx) = content.find("\n---\n") {
        (idx, 5)
    } else if let Some(idx) = content.find("\r\n---\r\n") {
        (idx, 7)
    } else {
        bail!("SKILL.md frontmatter must be closed with ---");
    };
    Ok((&content[..marker.0], &content[marker.0 + marker.1..]))
}

fn find_skill_dirs(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() && path.file_name().and_then(|name| name.to_str()) == Some(SKILL_FILE) {
        return Ok(vec![
            path.parent()
                .context("SKILL.md path has no parent directory")?
                .to_path_buf(),
        ]);
    }
    if path.join(SKILL_FILE).exists() {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut dirs = Vec::new();
    if path.is_dir() {
        find_skill_dirs_recursive(path, &mut dirs)?;
    }
    Ok(dirs)
}

fn find_skill_dirs_recursive(path: &Path, dirs: &mut Vec<PathBuf>) -> Result<()> {
    if path.join(SKILL_FILE).exists() {
        dirs.push(path.to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let child = entry?.path();
        if child.is_dir() {
            if child
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, ".git" | "node_modules" | "target"))
            {
                continue;
            }
            find_skill_dirs_recursive(&child, dirs)?;
        }
    }
    Ok(())
}

async fn try_import_github_source(
    source: &str,
    skill: Option<&str>,
) -> Result<Option<Vec<PathBuf>>> {
    let Some(skill) = skill else {
        return Ok(None);
    };
    if parse_github_repo(source).is_none() {
        return Ok(None);
    }

    let skill_dir = materialize_github_skill_source(source, skill).await?;
    Ok(Some(find_skill_dirs(&skill_dir)?))
}

async fn install_github_remote_skill(
    remote: &RemoteSkill,
    scope: SkillScope,
) -> Result<SkillRecord> {
    let skill_dir = materialize_github_skill_source(
        remote.install_url.as_deref().unwrap_or(&remote.source),
        &remote.slug,
    )
    .await?;
    let candidates = find_skill_dirs(&skill_dir)?;
    let selected = select_candidates(candidates, None, false)?;
    let definition = load_definition(&selected[0])?;
    let source = format!("skills.sh:{}", remote.id);
    install_skill(&definition, &source, scope, true)?;
    record_for(&definition, &source, scope, true)
}

async fn materialize_github_skill_source(source: &str, skill: &str) -> Result<PathBuf> {
    let (owner, repo) = parse_github_repo(source)
        .with_context(|| format!("not a GitHub skill source: {source}"))?;
    let client = reqwest::Client::builder()
        .user_agent("cortex-skill-installer")
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitHub client")?;
    let (branch, tree) = fetch_github_tree(&client, &owner, &repo).await?;
    let remote_dir = select_github_skill_dir(&tree, skill)
        .with_context(|| format!("skill '{skill}' was not found in {owner}/{repo}"))?;
    let root = std::env::temp_dir().join(format!("cortex-github-skill-{}", uuid::Uuid::new_v4()));
    let local_dir = root.join(skill);
    fs::create_dir_all(&local_dir)
        .with_context(|| format!("failed to create {}", local_dir.display()))?;

    let prefix = if remote_dir.is_empty() {
        String::new()
    } else {
        format!("{remote_dir}/")
    };
    for item in tree.iter().filter(|item| {
        item.kind == "blob"
            && (remote_dir.is_empty()
                || item.path == remote_dir
                || item.path.starts_with(prefix.as_str()))
    }) {
        let rel = if prefix.is_empty() {
            item.path.as_str()
        } else {
            item.path
                .strip_prefix(prefix.as_str())
                .unwrap_or(item.path.as_str())
        };
        let rel = safe_relative_path(rel)?;
        let path = local_dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let raw_url = github_raw_url(&owner, &repo, &branch, &item.path);
        let bytes = client
            .get(raw_url)
            .send()
            .await
            .with_context(|| format!("failed to download {}", item.path))?
            .error_for_status()
            .with_context(|| format!("GitHub raw request failed for {}", item.path))?
            .bytes()
            .await
            .with_context(|| format!("failed to read {}", item.path))?;
        fs::write(&path, &bytes).with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(local_dir)
}

async fn fetch_github_tree(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
) -> Result<(String, Vec<GitHubTreeItem>)> {
    let mut last_error = None;
    for branch in ["main", "master"] {
        let url =
            format!("https://api.github.com/repos/{owner}/{repo}/git/trees/{branch}?recursive=1");
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let parsed = response
                    .json::<GitHubTreeResponse>()
                    .await
                    .with_context(|| format!("failed to parse GitHub tree for {owner}/{repo}"))?;
                return Ok((branch.to_string(), parsed.tree));
            }
            Ok(response) => {
                last_error = Some(format!("GitHub returned {}", response.status()));
            }
            Err(error) => {
                last_error = Some(error.to_string());
            }
        }
    }
    bail!(
        "failed to fetch GitHub tree for {owner}/{repo}: {}",
        last_error.unwrap_or_else(|| "unknown error".to_string())
    )
}

fn select_github_skill_dir(tree: &[GitHubTreeItem], skill: &str) -> Option<String> {
    let mut candidates = tree
        .iter()
        .filter(|item| item.kind == "blob" && item.path.ends_with(SKILL_FILE))
        .filter_map(|item| {
            let dir = item.path.strip_suffix(&format!("/{SKILL_FILE}"))?;
            let basename = dir.rsplit('/').next().unwrap_or(dir);
            (basename == skill || dir == skill || dir.ends_with(&format!("/{skill}")))
                .then(|| dir.to_string())
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        a.matches('/')
            .count()
            .cmp(&b.matches('/').count())
            .then(a.cmp(b))
    });
    candidates.into_iter().next()
}

fn github_repo_for_remote(remote: &RemoteSkill) -> Option<(String, String)> {
    remote
        .install_url
        .as_deref()
        .and_then(parse_github_repo)
        .or_else(|| parse_github_repo(&remote.source))
}

fn parse_github_repo(source: &str) -> Option<(String, String)> {
    let source = source.trim().trim_end_matches(".git");
    let rest = source
        .strip_prefix("https://github.com/")
        .or_else(|| source.strip_prefix("http://github.com/"))
        .or_else(|| source.strip_prefix("git@github.com:"))
        .unwrap_or(source);
    if rest.contains("://") || rest.contains(char::is_whitespace) {
        return None;
    }

    let parts = rest
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 || !is_repo_part(parts[0]) || !is_repo_part(parts[1]) {
        return None;
    }
    Some((
        parts[0].to_string(),
        parts[1].trim_end_matches(".git").to_string(),
    ))
}

fn is_repo_part(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn github_raw_url(owner: &str, repo: &str, branch: &str, path: &str) -> String {
    format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{path}")
}

async fn import_with_skills_cli(source: &str, skill: Option<&str>) -> Result<Vec<PathBuf>> {
    let temp = std::env::temp_dir().join(format!("cortex-skills-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp).with_context(|| format!("failed to create {}", temp.display()))?;

    let cli = skills_cli_command(source, skill);
    let mut cmd = Command::new(&cli.program);
    cmd.args(&cli.args);
    cmd.current_dir(&temp)
        .env("CI", "1")
        .env("npm_config_yes", "true")
        .env("DISABLE_TELEMETRY", "1")
        .env("SKILLS_NO_TELEMETRY", "1");

    let output = timeout(Duration::from_secs(SKILLS_CLI_TIMEOUT_SECS), cmd.output())
        .await
        .with_context(|| {
            format!(
                "`{}` timed out after {}s; try a local path or retry when npm/git is responsive",
                cli.display,
                SKILLS_CLI_TIMEOUT_SECS
            )
        })?
        .with_context(|| {
            "failed to run `npx skills add`; install Node.js/npm or add the skill from a local path"
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        bail!("{} failed:\n{stdout}\n{stderr}", cli.display);
    }

    let dirs = find_skill_dirs(&temp)?;
    if dirs.is_empty() {
        bail!("npx skills add succeeded but no SKILL.md files were found");
    }
    Ok(dirs)
}

async fn fetch_remote_detail(id: &str) -> Result<SkillDetailResponse> {
    let client = reqwest::Client::new();
    send_skills_request(client.get(format!("https://skills.sh/api/v1/skills/{id}")))
        .await
        .with_context(|| format!("failed to fetch skills.sh skill detail for {id}"))?
        .error_for_status()
        .with_context(|| format!("skills.sh skill detail request failed for {id}"))?
        .json::<SkillDetailResponse>()
        .await
        .with_context(|| format!("failed to parse skills.sh skill detail for {id}"))
}

async fn send_skills_request(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, reqwest::Error> {
    if let Ok(key) = std::env::var("SKILLS_SH_API_KEY")
        && !key.trim().is_empty()
    {
        return request.bearer_auth(key).send().await;
    }
    request.send().await
}

async fn fetch_public_catalog() -> Result<Vec<RemoteSkill>> {
    if let Some(cached) = PUBLIC_CATALOG_CACHE.get() {
        return Ok(cached.clone());
    }

    let html = reqwest::Client::new()
        .get("https://skills.sh")
        .send()
        .await
        .context("failed to fetch public skills.sh page")?
        .error_for_status()
        .context("public skills.sh page request failed")?
        .text()
        .await
        .context("failed to read public skills.sh page")?;
    let parsed = parse_public_catalog(&html)?;
    let _ = PUBLIC_CATALOG_CACHE.set(parsed.clone());
    Ok(parsed)
}

fn parse_public_catalog(html: &str) -> Result<Vec<RemoteSkill>> {
    let normalized = html.replace("\\\"", "\"");
    let mut skills = Vec::new();
    let mut cursor = 0usize;

    while let Some(relative_start) = normalized[cursor..].find("{\"source\":\"") {
        let start = cursor + relative_start;
        let Some(relative_end) = normalized[start..].find('}') else {
            break;
        };
        let end = start + relative_end + 1;
        let object = &normalized[start..end];
        cursor = end;

        let Ok(embedded) = serde_json::from_str::<EmbeddedSkill>(object) else {
            continue;
        };
        if embedded.source.is_empty() || embedded.skill_id.is_empty() {
            continue;
        }

        let source_type = if embedded.source.contains('.') && !embedded.source.contains('/') {
            "well-known"
        } else {
            "github"
        };
        let install_url = if source_type == "github" {
            Some(format!("https://github.com/{}", embedded.source))
        } else {
            Some(format!("https://{}", embedded.source))
        };
        skills.push(RemoteSkill {
            id: format!("{}/{}", embedded.source, embedded.skill_id),
            slug: embedded.skill_id.clone(),
            name: embedded.name,
            source: embedded.source,
            installs: embedded.installs,
            source_type: source_type.to_string(),
            install_url,
            is_duplicate: false,
        });
    }

    if skills.is_empty() {
        bail!("could not find public skills in skills.sh page");
    }

    let mut seen = HashSet::new();
    skills.retain(|skill| seen.insert(skill.id.clone()));
    Ok(skills)
}

fn materialize_remote_files(id: &str, files: &[RemoteSkillFile]) -> Result<PathBuf> {
    let root = std::env::temp_dir().join(format!("cortex-remote-skill-{}", uuid::Uuid::new_v4()));
    let skill_dir = root.join(sanitize_remote_id(id));
    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;

    for file in files {
        let rel = safe_relative_path(&file.path)?;
        let path = skill_dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, &file.contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(skill_dir)
}

fn safe_relative_path(path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        bail!("remote skill file path must be relative: {path}");
    }
    let mut out = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => bail!("remote skill file path is unsafe: {path}"),
        }
    }
    if out.as_os_str().is_empty() {
        bail!("remote skill file path is empty");
    }
    Ok(out)
}

fn sanitize_remote_id(id: &str) -> String {
    id.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn select_candidates(
    candidates: Vec<PathBuf>,
    skill: Option<&str>,
    all: bool,
) -> Result<Vec<PathBuf>> {
    if candidates.is_empty() {
        bail!("no SKILL.md files found");
    }
    if all {
        return Ok(candidates);
    }
    if let Some(name) = skill {
        for candidate in candidates {
            let definition = load_definition(&candidate)?;
            if definition.name == name {
                return Ok(vec![candidate]);
            }
        }
        bail!("skill '{name}' was not found in source");
    }
    if candidates.len() == 1 {
        return Ok(candidates);
    }
    let names = candidates
        .iter()
        .filter_map(|candidate| load_definition(candidate).ok().map(|d| d.name))
        .collect::<Vec<_>>()
        .join(", ");
    bail!("source contains multiple skills ({names}); use --skill <name> or --all")
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &dest)
                .with_context(|| format!("failed to copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn validate_skill_name(name: &str) -> Result<()> {
    let len = name.len();
    if !(2..=64).contains(&len) {
        bail!("skill name must be 2-64 characters");
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    let last = name.chars().last().unwrap();
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        bail!("skill name must start and end with an alphanumeric character");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-'))
    {
        bail!(
            "skill name may contain only lowercase letters, numbers, dots, underscores, and hyphens"
        );
    }
    Ok(())
}

fn target_scopes(scope: Option<SkillScope>) -> Vec<SkillScope> {
    match scope {
        Some(scope) => vec![scope],
        None => vec![SkillScope::Project, SkillScope::Global],
    }
}

fn default_scope() -> SkillScope {
    if project_root().is_some() {
        SkillScope::Project
    } else {
        SkillScope::Global
    }
}

fn skills_root(scope: SkillScope) -> Result<PathBuf> {
    match scope {
        SkillScope::Global => {
            let home = dirs::home_dir().context("could not determine home directory")?;
            Ok(home.join(".cortex").join("skills"))
        }
        SkillScope::Project => {
            let root = project_root().unwrap_or(std::env::current_dir()?);
            Ok(root.join(".cortex").join("skills"))
        }
    }
}

fn project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() || dir.join("Cargo.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn relevance_score(definition: &SkillDefinition, agent_name: &str, prompt: &str) -> u32 {
    let haystack = format!("{} {}", agent_name, prompt).to_ascii_lowercase();
    let mut score = 0;
    if haystack.contains(&definition.name) {
        score += 10;
    }
    for tag in &definition.tags {
        if !tag.is_empty() && haystack.contains(&tag.to_ascii_lowercase()) {
            score += 4;
        }
    }
    for word in definition
        .description
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| word.len() >= 4)
    {
        if haystack.contains(&word.to_ascii_lowercase()) {
            score += 1;
        }
    }
    if prompt.contains(&format!("@skill:{}", definition.name))
        || prompt.contains(&format!("/{}", definition.name))
    {
        score += 20;
    }
    score
}

fn first_non_empty<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.trim().is_empty() { b } else { a }
}

fn escape_yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn take_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn help_lines() -> Vec<String> {
    vec![
        "skill list".to_string(),
        "skill add <owner/repo|path|url> [--skill <name>] [--global|--project] [--all]".to_string(),
        "skill enable <name> [--global|--project]".to_string(),
        "skill disable <name> [--global|--project]".to_string(),
        "skill remove <name> [--global|--project]".to_string(),
        "skill create <name> --description <text> [--global|--project]".to_string(),
        "skill show <name>".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_args() {
        let args = parse_command_line("create my-skill --description \"hello world\"").unwrap();
        assert_eq!(
            args,
            vec!["create", "my-skill", "--description", "hello world"]
        );
    }

    #[test]
    fn validates_skill_names() {
        assert!(validate_skill_name("my-skill_1").is_ok());
        assert!(validate_skill_name("A-bad").is_err());
        assert!(validate_skill_name("-bad").is_err());
    }

    #[test]
    fn parses_skill_frontmatter() {
        let content = "---\nname: test-skill\ndescription: Test skill\nmetadata:\n  tags: [rust, testing]\n---\n\n# Test\n\nDo work.";
        let (frontmatter, body) = split_frontmatter(content).unwrap();
        let parsed: SkillFrontmatter = serde_yaml::from_str(frontmatter).unwrap();
        assert_eq!(parsed.name, "test-skill");
        assert_eq!(parsed.metadata.tags, vec!["rust", "testing"]);
        assert!(body.contains("Do work."));
    }

    #[test]
    fn parses_skills_sh_list_response() {
        let raw = r#"{
            "data": [{
                "id": "openai/skills/openai-docs",
                "slug": "openai-docs",
                "name": "OpenAI Docs",
                "source": "openai/skills",
                "installs": 1100,
                "sourceType": "github",
                "installUrl": "https://github.com/openai/skills",
                "isDuplicate": false
            }]
        }"#;
        let parsed: SkillListResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.data[0].id, "openai/skills/openai-docs");
        assert_eq!(
            parsed.data[0].install_url.as_deref(),
            Some("https://github.com/openai/skills")
        );
    }

    #[test]
    fn parses_public_skills_catalog_from_html() {
        let html = r#"<script>self.__next_f.push([1,"[{\"source\":\"callstackincubator/agent-skills\",\"skillId\":\"react-native-best-practices\",\"name\":\"react-native-best-practices\",\"installs\":11289}]"])</script>"#;
        let parsed = parse_public_catalog(html).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed[0].id,
            "callstackincubator/agent-skills/react-native-best-practices"
        );
        assert_eq!(
            parsed[0].install_url.as_deref(),
            Some("https://github.com/callstackincubator/agent-skills")
        );
    }

    #[test]
    fn builds_noninteractive_skills_cli_command() {
        let command =
            skills_cli_command("https://github.com/vercel-labs/skills", Some("find-skills"));
        assert_eq!(command.program, "npx");
        assert_eq!(
            command.args,
            vec![
                "--yes",
                "skills",
                "add",
                "https://github.com/vercel-labs/skills",
                "--skill",
                "find-skills"
            ]
        );
        assert_eq!(
            command.display,
            "npx --yes skills add https://github.com/vercel-labs/skills --skill find-skills"
        );
    }

    #[test]
    fn parses_github_repo_sources() {
        assert_eq!(
            parse_github_repo("https://github.com/vercel-labs/skills"),
            Some(("vercel-labs".to_string(), "skills".to_string()))
        );
        assert_eq!(
            parse_github_repo("git@github.com:vercel-labs/skills.git"),
            Some(("vercel-labs".to_string(), "skills".to_string()))
        );
        assert_eq!(
            parse_github_repo("vercel-labs/skills"),
            Some(("vercel-labs".to_string(), "skills".to_string()))
        );
        assert_eq!(parse_github_repo("https://example.com/org/repo"), None);
    }

    #[test]
    fn selects_nested_github_skill_dir() {
        let tree = vec![
            GitHubTreeItem {
                path: "README.md".to_string(),
                kind: "blob".to_string(),
            },
            GitHubTreeItem {
                path: "skills/find-skills/SKILL.md".to_string(),
                kind: "blob".to_string(),
            },
            GitHubTreeItem {
                path: "skills/find-skills/references/api.md".to_string(),
                kind: "blob".to_string(),
            },
        ];
        assert_eq!(
            select_github_skill_dir(&tree, "find-skills").as_deref(),
            Some("skills/find-skills")
        );
    }

    #[test]
    fn describes_github_remote_install_without_npx() {
        let remote = RemoteSkill {
            id: "vercel-labs/skills/find-skills".to_string(),
            slug: "find-skills".to_string(),
            name: "find-skills".to_string(),
            source: "vercel-labs/skills".to_string(),
            installs: 0,
            source_type: "github".to_string(),
            install_url: Some("https://github.com/vercel-labs/skills".to_string()),
            is_duplicate: false,
        };
        assert_eq!(
            remote_install_display(&remote),
            "GitHub vercel-labs/skills --skill find-skills"
        );
    }

    #[test]
    fn rejects_unsafe_remote_file_paths() {
        assert!(safe_relative_path("SKILL.md").is_ok());
        assert!(safe_relative_path("references/api.md").is_ok());
        assert!(safe_relative_path("../SKILL.md").is_err());
        assert!(safe_relative_path("/tmp/SKILL.md").is_err());
    }

    #[test]
    fn scores_explicit_skill_mentions_highest() {
        let definition = SkillDefinition {
            name: "api-docs".to_string(),
            description: "Generate API docs".to_string(),
            tags: vec!["documentation".to_string()],
            path: PathBuf::from("SKILL.md"),
            content: String::new(),
        };
        assert!(relevance_score(&definition, "developer", "use @skill:api-docs") >= 20);
    }
}
