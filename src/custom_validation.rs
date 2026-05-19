// Temporary while validation core types are staged before CLI/runtime validation wiring.
// Remove once later custom validation tasks call this module from production paths.
#![allow(dead_code)]

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
}
