# Custom Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add strict-but-pragmatic validation for Cortex custom agents and workflows, with CLI/REPL reporting and pre-execution blocking for invalid custom workflows.

**Architecture:** Add a focused `src/custom_validation.rs` module that discovers local/global custom definition files, parses them with existing `custom_defs` helpers, emits structured diagnostics, and formats human-readable reports. Reuse the validator from `main.rs`, `repl.rs`, and `workflows::get_workflow()` so command output and runtime blocking share one source of truth.

**Tech Stack:** Rust, clap, tokio, anyhow, serde_yaml, existing Cortex `AgentLoader`, `CustomAgentDef`, `CustomWorkflowDef`, `TuiEvent`, and Cargo tests.

---

## File Structure

- Create `src/custom_validation.rs`: validation types, discovery helpers, rules, report formatting, and unit tests.
- Modify `src/main.rs`: register `mod custom_validation;`, add `Validate` command, print report, exit non-zero on errors.
- Modify `src/workflows/mod.rs`: validate named custom workflow before returning `CustomWorkflow`.
- Modify `src/workflows/custom.rs`: remove normal missing-agent fallback; return an error if an agent is missing defensively.
- Modify `src/repl.rs`: add `/validate` help text and handler that emits the validator report to logs.
- Modify `README.md`: document `cortex validate`, `/validate`, and pre-execution validation.
- Modify `LACUNES.md`: mark lacune 8 as complete after code/tests/docs pass.

---

### Task 1: Add Validation Core Types And Report Formatting

**Files:**
- Create: `src/custom_validation.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create the module shell with failing report-format tests**

Add `mod custom_validation;` near the other module declarations in `src/main.rs`.

Create `src/custom_validation.rs` with the initial types and tests:

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDiagnostic {
    pub severity: ValidationSeverity,
    pub path: PathBuf,
    pub target: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub diagnostics: Vec<ValidationDiagnostic>,
}

impl ValidationReport {
    pub fn push(&mut self, diagnostic: ValidationDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == ValidationSeverity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == ValidationSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == ValidationSeverity::Warning)
            .count()
    }

    pub fn format_human(&self) -> String {
        let title = if self.has_errors() {
            "Custom definition validation failed"
        } else if self.warning_count() > 0 {
            "Custom definition validation passed with warnings"
        } else {
            "Custom definition validation passed"
        };

        let mut out = String::from(title);
        out.push_str("\n\n");

        for diagnostic in &self.diagnostics {
            let severity = match diagnostic.severity {
                ValidationSeverity::Error => "ERROR",
                ValidationSeverity::Warning => "WARNING",
            };
            out.push_str(&format!(
                "{} {} [{}] {}\n  {}\n\n",
                severity,
                diagnostic.path.display(),
                diagnostic.target,
                diagnostic.code,
                diagnostic.message
            ));
        }

        out.push_str(&format!(
            "{} diagnostics: {} errors, {} warnings",
            self.diagnostics.len(),
            self.error_count(),
            self.warning_count()
        ));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_formats_clean_success() {
        let report = ValidationReport::default();
        assert_eq!(
            report.format_human(),
            "Custom definition validation passed\n\n0 diagnostics: 0 errors, 0 warnings"
        );
    }

    #[test]
    fn report_formats_errors_and_warnings() {
        let mut report = ValidationReport::default();
        report.push(ValidationDiagnostic {
            severity: ValidationSeverity::Error,
            path: PathBuf::from(".cortex/workflows/outreach.md"),
            target: "workflow:outreach".to_string(),
            code: "missing-agent",
            message: "step 'writer' references missing agent 'cold_email_writer'".to_string(),
        });
        report.push(ValidationDiagnostic {
            severity: ValidationSeverity::Warning,
            path: PathBuf::from(".cortex/agents/sender.md"),
            target: "agent:sender".to_string(),
            code: "sensitive-tool",
            message: "custom agent uses email; verify dry-run/send behavior before running"
                .to_string(),
        });

        let formatted = report.format_human();
        assert!(formatted.contains("Custom definition validation failed"));
        assert!(formatted.contains("ERROR .cortex/workflows/outreach.md [workflow:outreach] missing-agent"));
        assert!(formatted.contains("WARNING .cortex/agents/sender.md [agent:sender] sensitive-tool"));
        assert!(formatted.contains("2 diagnostics: 1 errors, 1 warnings"));
    }
}
```

- [ ] **Step 2: Run the focused tests**

Run: `cargo test custom_validation::tests::report_formats -- --nocapture`

Expected: PASS for both report formatting tests.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs src/custom_validation.rs
git commit -m "feat: add custom validation report types"
```

---

### Task 2: Implement Agent Validation Rules

**Files:**
- Modify: `src/custom_validation.rs`

- [ ] **Step 1: Add failing tests for agent rules**

Append these tests inside `#[cfg(test)] mod tests`:

```rust
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("cortex-validation-{name}-{suffix}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_file(path: &std::path::Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn valid_agent_has_no_diagnostics() {
    let root = temp_root("valid-agent");
    let path = root.join(".cortex/agents/writer.md");
    write_file(
        &path,
        "---\nname: writer\ndescription: Writes crisp copy\nmodel: ollama/qwen2.5:32b\ntools: [web_search]\n---\nYou write useful copy.\n",
    );

    let report = validate_agent_file(&path);
    assert_eq!(report.diagnostics, Vec::new());
}

#[test]
fn agent_with_unknown_tool_is_error() {
    let root = temp_root("unknown-tool");
    let path = root.join(".cortex/agents/writer.md");
    write_file(
        &path,
        "---\nname: writer\ndescription: Writes copy\nmodel: ollama/qwen2.5:32b\ntools: [shell]\n---\nYou write.\n",
    );

    let report = validate_agent_file(&path);
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "unknown-tool"));
}

#[test]
fn agent_with_sensitive_tool_is_warning() {
    let root = temp_root("sensitive-tool");
    let path = root.join(".cortex/agents/sender.md");
    write_file(
        &path,
        "---\nname: sender\ndescription: Sends emails carefully\nmodel: ollama/qwen2.5:32b\ntools: [email]\n---\nYou prepare outreach.\n",
    );

    let report = validate_agent_file(&path);
    assert!(!report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "sensitive-tool"));
}

#[test]
fn agent_with_empty_body_is_error() {
    let root = temp_root("empty-body");
    let path = root.join(".cortex/agents/empty.md");
    write_file(
        &path,
        "---\nname: empty\ndescription: Empty prompt\nmodel: ollama/qwen2.5:32b\ntools: []\n---\n",
    );

    let report = validate_agent_file(&path);
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "empty-prompt"));
}

#[test]
fn agent_with_invalid_yaml_is_error() {
    let root = temp_root("bad-yaml");
    let path = root.join(".cortex/agents/bad.md");
    write_file(&path, "---\nname: [bad\n---\nPrompt\n");

    let report = validate_agent_file(&path);
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "parse-error"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test custom_validation::tests::agent -- --nocapture`

Expected: FAIL because `validate_agent_file` is not implemented.

- [ ] **Step 3: Implement agent validation**

Add these helpers above the test module:

```rust
use anyhow::Context;

const KNOWN_TOOLS: &[&str] = &["filesystem", "terminal", "web_search", "email"];
const SENSITIVE_TOOLS: &[&str] = &["terminal", "email"];
const LONG_PROMPT_CHARS: usize = 24_000;

pub fn validate_agent_file(path: &std::path::Path) -> ValidationReport {
    let mut report = ValidationReport::default();
    let content = match std::fs::read_to_string(path)
        .with_context(|| format!("cannot read agent file: {}", path.display()))
    {
        Ok(content) => content,
        Err(e) => {
            push_error(&mut report, path, "agent:<unknown>", "read-error", e.to_string());
            return report;
        }
    };

    let def = match crate::custom_defs::parse_agent_def(&content) {
        Ok(def) => def,
        Err(e) => {
            push_error(
                &mut report,
                path,
                "agent:<unknown>",
                "parse-error",
                format!("invalid agent definition: {e}"),
            );
            return report;
        }
    };

    let target = format!("agent:{}", display_name(&def.name));
    validate_name(&mut report, path, &target, "agent", &def.name);
    require_nonempty(&mut report, path, &target, "missing-name", "name", &def.name);
    require_nonempty(
        &mut report,
        path,
        &target,
        "missing-description",
        "description",
        &def.description,
    );
    require_nonempty(&mut report, path, &target, "missing-model", "model", &def.model);

    if def.system_prompt.trim().is_empty() {
        push_error(
            &mut report,
            path,
            &target,
            "empty-prompt",
            "agent prompt body is empty",
        );
    }

    if def.description.trim().len() < 12 {
        push_warning(
            &mut report,
            path,
            &target,
            "short-description",
            "description is very short",
        );
    }

    if def.system_prompt.len() > LONG_PROMPT_CHARS {
        push_warning(
            &mut report,
            path,
            &target,
            "long-prompt",
            format!("prompt body is {} chars", def.system_prompt.len()),
        );
    }

    if !def.model.contains('/') {
        push_warning(
            &mut report,
            path,
            &target,
            "model-without-provider",
            format!(
                "model '{}' has no provider prefix; Cortex will route through the active provider",
                def.model
            ),
        );
    }

    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if !def.name.is_empty() && stem != def.name {
            push_warning(
                &mut report,
                path,
                &target,
                "filename-name-mismatch",
                format!("filename stem '{stem}' differs from declared name '{}'", def.name),
            );
        }
    }

    for tool in &def.tools {
        let normalized = tool.trim();
        if !KNOWN_TOOLS.iter().any(|known| known.eq_ignore_ascii_case(normalized)) {
            push_error(
                &mut report,
                path,
                &target,
                "unknown-tool",
                format!("unknown tool '{normalized}'"),
            );
        } else if SENSITIVE_TOOLS
            .iter()
            .any(|sensitive| sensitive.eq_ignore_ascii_case(normalized))
        {
            push_warning(
                &mut report,
                path,
                &target,
                "sensitive-tool",
                format!("custom agent uses {normalized}; verify behavior before running"),
            );
        }
    }

    report
}

fn push_error(
    report: &mut ValidationReport,
    path: &std::path::Path,
    target: impl Into<String>,
    code: &'static str,
    message: impl Into<String>,
) {
    report.push(ValidationDiagnostic {
        severity: ValidationSeverity::Error,
        path: path.to_path_buf(),
        target: target.into(),
        code,
        message: message.into(),
    });
}

fn push_warning(
    report: &mut ValidationReport,
    path: &std::path::Path,
    target: impl Into<String>,
    code: &'static str,
    message: impl Into<String>,
) {
    report.push(ValidationDiagnostic {
        severity: ValidationSeverity::Warning,
        path: path.to_path_buf(),
        target: target.into(),
        code,
        message: message.into(),
    });
}

fn require_nonempty(
    report: &mut ValidationReport,
    path: &std::path::Path,
    target: &str,
    code: &'static str,
    field: &str,
    value: &str,
) {
    if value.trim().is_empty() {
        push_error(report, path, target, code, format!("required field '{field}' is empty"));
    }
}

fn validate_name(
    report: &mut ValidationReport,
    path: &std::path::Path,
    target: &str,
    kind: &str,
    name: &str,
) {
    if name.trim().is_empty() {
        return;
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        push_error(
            report,
            path,
            target,
            "invalid-name",
            format!("{kind} name '{name}' must match ^[a-zA-Z0-9_-]+$"),
        );
    }
}

fn display_name(name: &str) -> &str {
    if name.trim().is_empty() { "<unknown>" } else { name }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test custom_validation::tests::agent -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/custom_validation.rs
git commit -m "feat: validate custom agent definitions"
```

---

### Task 3: Implement Workflow Validation And Discovery

**Files:**
- Modify: `src/custom_validation.rs`

- [ ] **Step 1: Add failing workflow validation tests**

Append these tests:

```rust
#[test]
fn workflow_with_missing_agent_is_error() {
    let root = temp_root("missing-agent");
    let workflow = root.join(".cortex/workflows/outreach.md");
    write_file(
        &workflow,
        "---\nname: outreach\ndescription: Outreach pipeline\nagents:\n  - role: writer\n    agent: cold_email_writer\n---\nPipeline.\n",
    );

    let report = validate_workflow_file(&workflow, Some(&root));
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "missing-agent"));
}

#[test]
fn workflow_with_duplicate_roles_is_error() {
    let root = temp_root("duplicate-role");
    let agent = root.join(".cortex/agents/writer.md");
    write_file(
        &agent,
        "---\nname: writer\ndescription: Writes copy\nmodel: ollama/qwen2.5:32b\ntools: []\n---\nYou write.\n",
    );
    let workflow = root.join(".cortex/workflows/outreach.md");
    write_file(
        &workflow,
        "---\nname: outreach\ndescription: Outreach pipeline\nagents:\n  - role: writer\n    agent: writer\n  - role: writer\n    agent: writer\n---\nPipeline.\n",
    );

    let report = validate_workflow_file(&workflow, Some(&root));
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "duplicate-role"));
}

#[test]
fn workflow_with_builtin_name_is_error() {
    let root = temp_root("builtin-name");
    let workflow = root.join(".cortex/workflows/dev.md");
    write_file(
        &workflow,
        "---\nname: dev\ndescription: Collides\nagents: []\n---\nPipeline.\n",
    );

    let report = validate_workflow_file(&workflow, Some(&root));
    assert!(report.has_errors());
    assert!(report.diagnostics.iter().any(|d| d.code == "builtin-workflow-collision"));
}

#[test]
fn workflow_with_existing_agent_has_no_errors() {
    let root = temp_root("existing-agent");
    write_file(
        &root.join(".cortex/agents/writer.md"),
        "---\nname: writer\ndescription: Writes copy\nmodel: ollama/qwen2.5:32b\ntools: []\n---\nYou write.\n",
    );
    write_file(
        &root.join(".cortex/workflows/outreach.md"),
        "---\nname: outreach\ndescription: Outreach pipeline\nagents:\n  - role: writer\n    agent: writer\n---\nPipeline.\n",
    );

    let report = validate_workflow_file(&root.join(".cortex/workflows/outreach.md"), Some(&root));
    assert!(!report.has_errors(), "{}", report.format_human());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test custom_validation::tests::workflow -- --nocapture`

Expected: FAIL because workflow functions are not implemented.

- [ ] **Step 3: Implement workflow validation and discovery**

Add imports and functions:

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MANY_WORKFLOW_STEPS: usize = 8;

pub fn validate_workflow_file(path: &Path, project_root: Option<&Path>) -> ValidationReport {
    let mut report = ValidationReport::default();
    let content = match std::fs::read_to_string(path)
        .with_context(|| format!("cannot read workflow file: {}", path.display()))
    {
        Ok(content) => content,
        Err(e) => {
            push_error(&mut report, path, "workflow:<unknown>", "read-error", e.to_string());
            return report;
        }
    };

    let def = match crate::custom_defs::parse_workflow_def(&content) {
        Ok(def) => def,
        Err(e) => {
            push_error(
                &mut report,
                path,
                "workflow:<unknown>",
                "parse-error",
                format!("invalid workflow definition: {e}"),
            );
            return report;
        }
    };

    let target = format!("workflow:{}", display_name(&def.name));
    validate_name(&mut report, path, &target, "workflow", &def.name);
    require_nonempty(&mut report, path, &target, "missing-name", "name", &def.name);
    require_nonempty(
        &mut report,
        path,
        &target,
        "missing-description",
        "description",
        &def.description,
    );

    if crate::workflows::available_workflows()
        .iter()
        .any(|workflow| workflow.name == def.name)
    {
        push_error(
            &mut report,
            path,
            &target,
            "builtin-workflow-collision",
            format!("custom workflow '{}' collides with a built-in workflow", def.name),
        );
    }

    if def.agents.is_empty() {
        push_error(
            &mut report,
            path,
            &target,
            "missing-agents",
            "workflow must contain at least one agent step",
        );
    }

    if def.body.trim().is_empty() {
        push_warning(
            &mut report,
            path,
            &target,
            "empty-workflow-body",
            "workflow body is empty",
        );
    }

    if def.agents.len() > MANY_WORKFLOW_STEPS {
        push_warning(
            &mut report,
            path,
            &target,
            "many-steps",
            format!("workflow has {} steps", def.agents.len()),
        );
    }

    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
        if !def.name.is_empty() && stem != def.name {
            push_warning(
                &mut report,
                path,
                &target,
                "filename-name-mismatch",
                format!("filename stem '{stem}' differs from declared name '{}'", def.name),
            );
        }
    }

    let mut roles = HashSet::new();
    for step in &def.agents {
        if step.role.trim().is_empty() {
            push_error(
                &mut report,
                path,
                &target,
                "missing-role",
                "workflow step has an empty role",
            );
        } else if !roles.insert(step.role.clone()) {
            push_error(
                &mut report,
                path,
                &target,
                "duplicate-role",
                format!("workflow role '{}' appears more than once", step.role),
            );
        }

        if step.agent.trim().is_empty() {
            push_error(
                &mut report,
                path,
                &target,
                "missing-step-agent",
                format!("step '{}' has an empty agent", step.role),
            );
        } else if !agent_exists(&step.agent, project_root) {
            push_error(
                &mut report,
                path,
                &target,
                "missing-agent",
                format!(
                    "step '{}' references missing agent '{}'",
                    step.role, step.agent
                ),
            );
        }
    }

    report
}

pub fn validate_all(project_root: Option<&Path>) -> ValidationReport {
    let mut report = ValidationReport::default();
    for path in discovered_agent_files(project_root) {
        report.diagnostics.extend(validate_agent_file(&path).diagnostics);
    }
    for path in discovered_workflow_files(project_root) {
        report
            .diagnostics
            .extend(validate_workflow_file(&path, project_root).diagnostics);
    }
    report
}

fn agent_exists(name: &str, project_root: Option<&Path>) -> bool {
    agent_path(name, project_root).is_some()
}

pub fn agent_path(name: &str, project_root: Option<&Path>) -> Option<PathBuf> {
    for dir in agent_dirs(project_root) {
        let candidate = dir.join(format!("{name}.md"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn workflow_path(name: &str, project_root: Option<&Path>) -> Option<PathBuf> {
    for dir in workflow_dirs(project_root) {
        let candidate = dir.join(format!("{name}.md"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn validate_named_workflow(name: &str, project_root: Option<&Path>) -> ValidationReport {
    match workflow_path(name, project_root) {
        Some(path) => validate_workflow_file(&path, project_root),
        None => {
            let mut report = ValidationReport::default();
            push_error(
                &mut report,
                Path::new(name),
                format!("workflow:{name}"),
                "missing-workflow",
                format!("custom workflow '{name}' was not found"),
            );
            report
        }
    }
}

fn agent_dirs(project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = project_root {
        dirs.push(root.join(".cortex").join("agents"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".cortex").join("agents"));
    }
    dirs
}

fn workflow_dirs(project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = project_root {
        dirs.push(root.join(".cortex").join("workflows"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".cortex").join("workflows"));
    }
    dirs
}

fn discovered_agent_files(project_root: Option<&Path>) -> Vec<PathBuf> {
    discovered_md_files(agent_dirs(project_root))
}

fn discovered_workflow_files(project_root: Option<&Path>) -> Vec<PathBuf> {
    discovered_md_files(workflow_dirs(project_root))
}

fn discovered_md_files(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if seen.insert(stem.to_string()) {
                paths.push(path);
            }
        }
    }
    paths
}
```

If duplicate imports conflict with Task 2, consolidate them at the top of `src/custom_validation.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test custom_validation::tests -- --nocapture`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/custom_validation.rs
git commit -m "feat: validate custom workflow definitions"
```

---

### Task 4: Block Invalid Custom Workflow Execution

**Files:**
- Modify: `src/workflows/mod.rs`
- Modify: `src/workflows/custom.rs`
- Modify: `src/custom_validation.rs`

- [ ] **Step 1: Add failing runtime validation test**

In `src/workflows/mod.rs`, add this test inside the existing `#[cfg(test)] mod tests`:

```rust
#[test]
fn custom_workflow_with_missing_agent_is_rejected() {
    let root = std::env::temp_dir().join(format!(
        "cortex-workflow-invalid-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(root.join(".cortex/workflows")).unwrap();
    std::fs::write(
        root.join(".cortex/workflows/outreach.md"),
        "---\nname: outreach\ndescription: Outreach pipeline\nagents:\n  - role: writer\n    agent: missing_writer\n---\nPipeline.\n",
    )
    .unwrap();

    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&root).unwrap();
    let err = match get_workflow("outreach") {
        Ok(_) => {
            std::env::set_current_dir(previous).unwrap();
            panic!("invalid custom workflow should fail");
        }
        Err(e) => e.to_string(),
    };
    std::env::set_current_dir(previous).unwrap();

    assert!(err.contains("Custom definition validation failed"));
    assert!(err.contains("missing-agent"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test workflows::tests::custom_workflow_with_missing_agent_is_rejected -- --nocapture`

Expected: FAIL because `get_workflow()` still accepts the workflow.

- [ ] **Step 3: Validate before constructing `CustomWorkflow`**

In `src/workflows/mod.rs`, update the custom branch of `get_workflow()` so `Ok(Some(def))` validates first:

```rust
Ok(Some(def)) => {
    let report = crate::custom_validation::validate_named_workflow(
        custom_name,
        project_root.as_deref(),
    );
    if report.has_errors() {
        anyhow::bail!("{}", report.format_human());
    }
    Ok(Box::new(custom::CustomWorkflow { def }))
}
```

- [ ] **Step 4: Replace missing-agent fallback with defensive error**

In `src/workflows/custom.rs`, replace the fallback `None => { ... CustomAgentDef { ... } }` block with:

```rust
None => {
    anyhow::bail!(
        "custom workflow '{}' references missing agent '{}'; run `cortex validate` for details",
        self.def.name,
        step.agent
    );
}
```

Then remove the now-unused `CustomAgentDef` import from the top of `src/workflows/custom.rs`.

- [ ] **Step 5: Run focused tests**

Run: `cargo test workflows::tests::custom_workflow_with_missing_agent_is_rejected custom_validation::tests -- --nocapture`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/workflows/mod.rs src/workflows/custom.rs src/custom_validation.rs
git commit -m "feat: block invalid custom workflows"
```

---

### Task 5: Add `cortex validate` CLI Command

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add the clap command**

Add this variant to `enum Commands`:

```rust
/// Validate custom agents and workflows in the current project and user config
Validate,
```

- [ ] **Step 2: Implement the command handler**

In `main()`, add a match arm:

```rust
Some(Commands::Validate) => {
    let project_root = std::env::current_dir().ok();
    let report = custom_validation::validate_all(project_root.as_deref());
    println!("{}", report.format_human());
    if report.has_errors() {
        std::process::exit(1);
    }
}
```

- [ ] **Step 3: Run CLI help check**

Run: `cargo run -- validate`

Expected: command runs and prints one of:

```text
Custom definition validation passed
```

or a diagnostics report for existing local/global custom files. If existing user files cause errors, that is acceptable for this manual check because the command must surface them.

- [ ] **Step 4: Run compile check**

Run: `cargo check`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add custom validation cli"
```

---

### Task 6: Add `/validate` REPL Command

**Files:**
- Modify: `src/repl.rs`
- Modify: `README.md`

- [ ] **Step 1: Add help text**

In the `/help` output in `src/repl.rs`, add:

```rust
"  /validate                    — validate custom agents and workflows",
```

Place it near `/workflow list` and `/agent list`.

- [ ] **Step 2: Add command handler**

In `handle_command()`, add a match arm before `"/agent"`:

```rust
"/validate" => {
    let project_root = std::env::current_dir().ok();
    let report = crate::custom_validation::validate_all(project_root.as_deref());
    for line in report.format_human().lines() {
        send(
            tx,
            TuiEvent::TokenChunk {
                agent: "validate".to_string(),
                chunk: format!("  {line}"),
            },
        );
    }
}
```

- [ ] **Step 3: Update README command table**

In `README.md`, add a row in the REPL command table:

```markdown
| `/validate` | Validate custom agents and workflows |
```

Also add a one-shot example near CLI examples:

```bash
# Validate custom agent/workflow definitions
cortex validate
```

- [ ] **Step 4: Run compile check**

Run: `cargo check`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/repl.rs README.md
git commit -m "feat: add validate repl command"
```

---

### Task 7: Update Custom Workflow Documentation And Lacune Status

**Files:**
- Modify: `README.md`
- Modify: `LACUNES.md`

- [ ] **Step 1: Update README custom workflow text**

In the “What's new in 0.2.0” custom workflow bullet list, replace the fallback bullet with:

```markdown
- **Custom validation** — Run `cortex validate` or `/validate` to check custom agents and workflows. Cortex also validates a custom workflow before execution and blocks critical errors like missing agents, invalid YAML, unknown tools, or built-in workflow name collisions.
```

If another section still says missing agents fall back to generic agents, replace it with:

```markdown
Custom workflows must reference existing custom agents. Run `cortex validate` if a workflow fails to start; the report points to the file, step, and missing agent.
```

- [ ] **Step 2: Update `LACUNES.md` lacune 8**

Change lacune 8 to:

```markdown
### 8. Custom agents et workflows: validation trop critique pour rester permissive
**Statut:** Terminé
**Preuve:** Couvert par `src/custom_validation.rs`, `cortex validate`, `/validate`, validation pré-exécution des workflows custom, blocage des agents manquants/outils inconnus/YAML invalide, et tests Rust dédiés.
```

Keep the existing constat/importance/action text below it unless implementation makes a small wording update necessary.

- [ ] **Step 3: Add tracking entry**

At the end of `Suivi des lots`, add:

```markdown
- 2026-05-19 — Lot validation custom terminé: validation structurée agents/workflows custom, commandes `cortex validate` et `/validate`, blocage pré-exécution des workflows invalides. Lacune terminée: 8.
```

- [ ] **Step 4: Run docs grep**

Run: `rg -n "fallback|generic fallback|agent manquant|missing agent" README.md LACUNES.md src/workflows/custom.rs`

Expected: no README claim says missing custom agents normally fall back during workflow execution. Defensive code text in `src/workflows/custom.rs` may mention missing agents as an error.

- [ ] **Step 5: Commit**

```bash
git add README.md LACUNES.md
git commit -m "docs: document custom validation"
```

---

### Task 8: Final Verification

**Files:**
- Verify all changed files.

- [ ] **Step 1: Format**

Run: `cargo fmt`

Expected: no errors.

- [ ] **Step 2: Run tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 3: Run check**

Run: `cargo check`

Expected: PASS.

- [ ] **Step 4: Inspect status**

Run: `git status --short`

Expected: only intentional tracked changes remain, or a clean tracked worktree after all commits. Ignore pre-existing untracked `.DS_Store`, `.claude/`, and `.idea/` unless the user explicitly asks to clean them.

- [ ] **Step 5: Final commit if formatting changed files**

If `cargo fmt` changed files after previous commits:

```bash
git add src/custom_validation.rs src/main.rs src/workflows/mod.rs src/workflows/custom.rs src/repl.rs
git commit -m "style: format custom validation changes"
```

If no files changed, do not create an empty commit.
