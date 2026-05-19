// Temporary while validation core types are staged before CLI/runtime validation wiring.
// Remove once later custom validation tasks call this module from production paths.
#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::custom_defs::{CustomAgentDef, canonical_tool_name, parse_agent_def};

const SENSITIVE_TOOLS: &[&str] = &["terminal", "email"];

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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
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
            .any(|diagnostic| diagnostic.severity == ValidationSeverity::Error)
    }

    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == ValidationSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == ValidationSeverity::Warning)
            .count()
    }

    pub fn format_human(&self) -> String {
        let title = if self.has_errors() {
            "Custom definition validation failed"
        } else {
            "Custom definition validation passed"
        };
        let summary = format!(
            "{} diagnostics: {} errors, {} warnings",
            self.diagnostics.len(),
            self.error_count(),
            self.warning_count()
        );

        if self.diagnostics.is_empty() {
            return format!("{title}\n\n{summary}");
        }

        let mut output = String::from(title);
        output.push_str("\n\n");
        for diagnostic in &self.diagnostics {
            output.push_str(&format!(
                "{} {} {} [{}]: {}\n",
                diagnostic.severity.as_str(),
                diagnostic.path.display(),
                diagnostic.target,
                diagnostic.code,
                diagnostic.message
            ));
        }
        output.push('\n');
        output.push_str(&summary);
        output
    }
}

impl ValidationSeverity {
    fn as_str(self) -> &'static str {
        match self {
            ValidationSeverity::Error => "ERROR",
            ValidationSeverity::Warning => "WARNING",
        }
    }
}

pub fn validate_agent_file(path: &Path) -> ValidationReport {
    let mut report = ValidationReport::default();

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            push_error(
                &mut report,
                path,
                &display_name(path),
                "read-error",
                format!("cannot read agent file: {error}"),
            );
            return report;
        }
    };

    let agent = match parse_agent_def(&content) {
        Ok(agent) => agent,
        Err(error) => {
            push_missing_frontmatter_fields(&mut report, path, &content);
            push_error(
                &mut report,
                path,
                &display_name(path),
                "parse-error",
                format!("cannot parse agent definition: {error}"),
            );
            return report;
        }
    };

    validate_agent(path, &agent, &mut report);
    report
}

fn push_missing_frontmatter_fields(report: &mut ValidationReport, path: &Path, content: &str) {
    let Some(yaml) = frontmatter_yaml(content) else {
        return;
    };

    let Ok(frontmatter) = serde_yaml::from_str::<serde_yaml::Mapping>(yaml) else {
        return;
    };

    for (field, code) in [
        ("name", "missing-name"),
        ("description", "missing-description"),
        ("model", "missing-model"),
    ] {
        if !frontmatter.contains_key(serde_yaml::Value::String(field.to_string())) {
            push_error(
                report,
                path,
                &display_name(path),
                code,
                format!("agent {field} must not be empty"),
            );
        }
    }
}

fn frontmatter_yaml(content: &str) -> Option<&str> {
    let content = content.trim_start();
    let after_open = content.strip_prefix("---")?;
    let dash_pos = after_open.find("\n---");
    let head_pos = after_open.find("\n##");

    let close_pos = match (dash_pos, head_pos) {
        (Some(dash), Some(head)) => dash.min(head),
        (Some(dash), None) => dash,
        (None, Some(head)) => head,
        (None, None) => return None,
    };

    Some(after_open[..close_pos].trim())
}

fn validate_agent(path: &Path, agent: &CustomAgentDef, report: &mut ValidationReport) {
    let target = if agent.name.trim().is_empty() {
        display_name(path)
    } else {
        agent.name.clone()
    };

    require_nonempty(report, path, &target, "name", &agent.name, "missing-name");
    require_nonempty(
        report,
        path,
        &target,
        "description",
        &agent.description,
        "missing-description",
    );
    require_nonempty(
        report,
        path,
        &target,
        "model",
        &agent.model,
        "missing-model",
    );
    validate_name(report, path, &target, &agent.name);

    if agent.system_prompt.trim().is_empty() {
        push_error(
            report,
            path,
            &target,
            "empty-prompt",
            "agent prompt body must not be empty".to_string(),
        );
    }

    if !agent.description.trim().is_empty() && agent.description.trim().chars().count() < 12 {
        push_warning(
            report,
            path,
            &target,
            "short-description",
            "agent description should be at least 12 characters".to_string(),
        );
    }

    if agent.system_prompt.chars().count() > 24_000 {
        push_warning(
            report,
            path,
            &target,
            "long-prompt",
            "agent prompt is longer than 24000 characters".to_string(),
        );
    }

    if !agent.model.trim().is_empty() && !agent.model.contains('/') {
        push_warning(
            report,
            path,
            &target,
            "model-without-provider",
            "agent model should include a provider prefix".to_string(),
        );
    }

    if path.file_stem().and_then(|stem| stem.to_str()) != Some(agent.name.as_str()) {
        push_warning(
            report,
            path,
            &target,
            "filename-name-mismatch",
            "agent filename stem should match declared name".to_string(),
        );
    }

    for tool in &agent.tools {
        let Some(canonical_tool) = canonical_tool_name(tool) else {
            push_error(
                report,
                path,
                &target,
                "unknown-tool",
                format!("agent references unknown tool '{tool}'"),
            );
            continue;
        };

        if SENSITIVE_TOOLS.contains(&canonical_tool) {
            push_warning(
                report,
                path,
                &target,
                "sensitive-tool",
                format!("agent uses sensitive tool '{tool}'"),
            );
        }
    }
}

fn push_error(
    report: &mut ValidationReport,
    path: &Path,
    target: &str,
    code: &'static str,
    message: String,
) {
    report.push(ValidationDiagnostic {
        severity: ValidationSeverity::Error,
        path: path.to_path_buf(),
        target: target.to_string(),
        code,
        message,
    });
}

fn push_warning(
    report: &mut ValidationReport,
    path: &Path,
    target: &str,
    code: &'static str,
    message: String,
) {
    report.push(ValidationDiagnostic {
        severity: ValidationSeverity::Warning,
        path: path.to_path_buf(),
        target: target.to_string(),
        code,
        message,
    });
}

fn require_nonempty(
    report: &mut ValidationReport,
    path: &Path,
    target: &str,
    field: &str,
    value: &str,
    code: &'static str,
) {
    if value.trim().is_empty() {
        push_error(
            report,
            path,
            target,
            code,
            format!("agent {field} must not be empty"),
        );
    }
}

fn validate_name(report: &mut ValidationReport, path: &Path, target: &str, name: &str) {
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
            "agent name may only contain ASCII letters, digits, '_' and '-'".to_string(),
        );
    }
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static TEST_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

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
            path: PathBuf::from("custom.toml"),
            target: "workflow.dev".to_string(),
            code: "missing_agent",
            message: "references an unknown agent".to_string(),
        });
        report.push(ValidationDiagnostic {
            severity: ValidationSeverity::Warning,
            path: PathBuf::from("custom.toml"),
            target: "workflow.marketing".to_string(),
            code: "unused_prompt",
            message: "prompt is not referenced".to_string(),
        });

        let formatted = report.format_human();

        assert!(formatted.contains("Custom definition validation failed"));
        assert!(formatted.contains(
            "ERROR custom.toml workflow.dev [missing_agent]: references an unknown agent"
        ));
        assert!(formatted.contains(
            "WARNING custom.toml workflow.marketing [unused_prompt]: prompt is not referenced"
        ));
        assert!(formatted.contains("2 diagnostics: 1 errors, 1 warnings"));
    }

    mod agent {
        use super::*;

        fn write_agent_file(test_name: &str, name: &str, content: &str) -> PathBuf {
            let nonce = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "cortex-custom-validation-agent-{}-{test_name}-{nonce}",
                std::process::id(),
            ));
            fs::create_dir_all(&dir).expect("create temp dir");
            let path = dir.join(name);
            fs::write(&path, content).expect("write temp agent file");
            path
        }

        fn diagnostic_codes(report: &ValidationReport) -> Vec<&'static str> {
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code)
                .collect()
        }

        fn valid_agent_content() -> &'static str {
            "---\nname: designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [filesystem, web_search]\n---\nYou are a designer.\n"
        }

        fn assert_diagnostic(report: &ValidationReport, code: &str, severity: ValidationSeverity) {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code && diagnostic.severity == severity),
                "expected {severity:?} diagnostic with code {code}, got {:?}",
                report.diagnostics
            );
        }

        #[test]
        fn valid_agent_has_no_diagnostics() {
            let path = write_agent_file(
                "valid_agent_has_no_diagnostics",
                "designer.md",
                valid_agent_content(),
            );

            let report = validate_agent_file(&path);

            assert_eq!(report.diagnostics, Vec::new());
        }

        #[test]
        fn agent_with_unknown_tool_is_error() {
            let path = write_agent_file(
                "agent_with_unknown_tool_is_error",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [filesystem, browser]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_diagnostic(&report, "unknown-tool", ValidationSeverity::Error);
            assert!(report.has_errors());
        }

        #[test]
        fn agent_with_generated_tool_aliases_has_no_unknown_tool_errors() {
            let path = write_agent_file(
                "agent_with_generated_tool_aliases_has_no_unknown_tool_errors",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [Read, Write, Edit, Glob, Grep, WebFetch, WebSearch]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert!(
                report
                    .diagnostics
                    .iter()
                    .all(|diagnostic| diagnostic.code != "unknown-tool"),
                "unexpected unknown tool diagnostic in {:?}",
                report.diagnostics
            );
        }

        #[test]
        fn agent_with_bash_alias_warns_as_sensitive_tool() {
            let path = write_agent_file(
                "agent_with_bash_alias_warns_as_sensitive_tool",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [Bash]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_eq!(report.error_count(), 0);
            assert_diagnostic(&report, "sensitive-tool", ValidationSeverity::Warning);
        }

        #[test]
        fn agent_with_sensitive_tool_is_warning() {
            let path = write_agent_file(
                "agent_with_sensitive_tool_is_warning",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [terminal, email]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_eq!(report.error_count(), 0);
            assert_diagnostic(&report, "sensitive-tool", ValidationSeverity::Warning);
        }

        #[test]
        fn agent_with_empty_body_is_error() {
            let path = write_agent_file(
                "agent_with_empty_body_is_error",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [filesystem]\n---\n\n",
            );

            let report = validate_agent_file(&path);

            assert_diagnostic(&report, "empty-prompt", ValidationSeverity::Error);
        }

        #[test]
        fn agent_with_omitted_name_is_error() {
            let path = write_agent_file(
                "agent_with_omitted_name_is_error",
                "designer.md",
                "---\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [filesystem]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_diagnostic(&report, "missing-name", ValidationSeverity::Error);
        }

        #[test]
        fn agent_with_omitted_description_is_error() {
            let path = write_agent_file(
                "agent_with_omitted_description_is_error",
                "designer.md",
                "---\nname: designer\nmodel: ollama/qwen2.5:32b\ntools: [filesystem]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_diagnostic(&report, "missing-description", ValidationSeverity::Error);
        }

        #[test]
        fn agent_with_omitted_model_is_error() {
            let path = write_agent_file(
                "agent_with_omitted_model_is_error",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\ntools: [filesystem]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_diagnostic(&report, "missing-model", ValidationSeverity::Error);
        }

        #[test]
        fn agent_with_heading_separator_and_omitted_field_reports_missing_code() {
            let path = write_agent_file(
                "agent_with_heading_separator_and_omitted_field_reports_missing_code",
                "designer.md",
                "---\nname: designer\ndescription: Creates practical interface designs\ntools: [filesystem]\n## Agent\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_diagnostic(&report, "missing-model", ValidationSeverity::Error);
        }

        #[test]
        fn agent_with_invalid_yaml_is_error() {
            let path = write_agent_file(
                "agent_with_invalid_yaml_is_error",
                "designer.md",
                "---\nname: [designer\ndescription: Creates practical interface designs\nmodel: ollama/qwen2.5:32b\ntools: [filesystem]\n---\nYou are a designer.\n",
            );

            let report = validate_agent_file(&path);

            assert_eq!(diagnostic_codes(&report), vec!["parse-error"]);
            assert_diagnostic(&report, "parse-error", ValidationSeverity::Error);
        }
    }
}
