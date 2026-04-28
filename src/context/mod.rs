#![allow(dead_code)]

pub mod compressor;

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Files each agent role is allowed to read as input context.
pub fn files_for_agent(role: &str, project_root: &Path) -> Vec<PathBuf> {
    match role {
        "pm"        => vec![project_root.join("specs.md")],
        "tech_lead" => vec![project_root.join("specs.md")],
        "developer" => vec![
            project_root.join("specs.md"),
            project_root.join("architecture.md"),
        ],
        "qa"        => vec![project_root.join("architecture.md")],
        "devops"    => vec![project_root.join("architecture.md")],
        _           => vec![],
    }
}

/// Reads and concatenates context files for an agent, capped at `max_chars`.
/// Files that don't exist yet are silently skipped.
pub fn load_context(role: &str, project_root: &Path, max_chars: usize) -> Result<String> {
    let mut accumulated = String::new();

    for path in files_for_agent(role, project_root) {
        if !path.exists() {
            continue;
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let section = format!("\n\n## {} ##\n{}", name, content);

        let remaining = max_chars.saturating_sub(accumulated.len());
        if remaining == 0 {
            break;
        }
        if section.len() <= remaining {
            accumulated.push_str(&section);
        } else {
            let safe_len = section
                .char_indices()
                .take_while(|(i, _)| *i < remaining)
                .last()
                .map(|(i, _)| i + 1)
                .unwrap_or(0);
            accumulated.push_str(&section[..safe_len]);
            accumulated.push_str("\n[truncated]");
            break;
        }
    }

    Ok(accumulated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceo_has_no_context() {
        assert!(files_for_agent("ceo", Path::new("/tmp")).is_empty());
    }

    #[test]
    fn nonexistent_files_skipped() {
        let ctx = load_context("developer", Path::new("/nonexistent_cortex_dir_xyz"), 8192)
            .unwrap();
        assert!(ctx.is_empty());
    }

    #[test]
    fn developer_has_specs_and_arch() {
        let files = files_for_agent("developer", Path::new("/proj"));
        assert_eq!(files.len(), 2);
    }
}
