use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::custom_defs::{CustomAgentDef, CustomWorkflowDef, parse_agent_def, parse_workflow_def};

pub struct AgentLoader;

impl AgentLoader {
    /// Search directories in priority order: local `.cortex/agents/` before global `~/.cortex/agents/`.
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

    /// Load a single agent by name. Local directory takes priority over global.
    pub fn load_agent(name: &str, project_root: Option<&Path>) -> Result<Option<CustomAgentDef>> {
        for dir in Self::agent_dirs(project_root) {
            let candidate = dir.join(format!("{}.md", name));
            if candidate.exists() {
                let content = std::fs::read_to_string(&candidate)
                    .with_context(|| format!("cannot read agent file: {}", candidate.display()))?;
                let def = parse_agent_def(&content)
                    .with_context(|| format!("invalid agent def: {}", candidate.display()))?;
                return Ok(Some(def));
            }
        }
        Ok(None)
    }

    /// Load a single workflow definition by name.
    pub fn load_workflow(
        name: &str,
        project_root: Option<&Path>,
    ) -> Result<Option<CustomWorkflowDef>> {
        for dir in Self::workflow_dirs(project_root) {
            let candidate = dir.join(format!("{}.md", name));
            if candidate.exists() {
                let content = std::fs::read_to_string(&candidate).with_context(|| {
                    format!("cannot read workflow file: {}", candidate.display())
                })?;
                let def = parse_workflow_def(&content)
                    .with_context(|| format!("invalid workflow def: {}", candidate.display()))?;
                return Ok(Some(def));
            }
        }
        Ok(None)
    }

    /// All discovered agent definitions; local shadows global on name collision.
    pub fn list_agents(project_root: Option<&Path>) -> Vec<CustomAgentDef> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut agents = Vec::new();
        for dir in Self::agent_dirs(project_root) {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if seen.contains(stem) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    match parse_agent_def(&content) {
                        Ok(def) => {
                            seen.insert(stem.to_string());
                            agents.push(def);
                        }
                        Err(e) => {
                            eprintln!("[cortex] skipping agent '{}': {}", path.display(), e);
                        }
                    }
                }
            }
        }
        agents
    }

    /// All discovered workflow definitions; local shadows global on name collision.
    pub fn list_workflows(project_root: Option<&Path>) -> Vec<CustomWorkflowDef> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut workflows = Vec::new();
        for dir in Self::workflow_dirs(project_root) {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                if seen.contains(stem) {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(mut def) = parse_workflow_def(&content) {
                        if def.name.is_empty() {
                            def.name = stem.to_string();
                        }
                        seen.insert(stem.to_string());
                        workflows.push(def);
                    }
                }
            }
        }
        workflows
    }
}
