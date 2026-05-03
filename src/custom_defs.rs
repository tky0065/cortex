use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct CustomAgentDef {
    pub name: String,
    pub description: String,
    pub model: String,
    #[allow(dead_code)]
    pub tools: Vec<String>,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowStep {
    pub role: String,
    pub agent: String,
}

#[derive(Debug, Clone)]
pub struct CustomWorkflowDef {
    pub name: String,
    pub description: String,
    pub agents: Vec<WorkflowStep>,
    #[allow(dead_code)]
    pub body: String,
}

/// Strips YAML frontmatter from a static prompt string and returns only the body.
/// If the string has no frontmatter, returns it unchanged.
pub fn prompt_body(raw: &'static str) -> &'static str {
    match split_frontmatter(raw) {
        Ok((_, body)) => body,
        Err(_) => raw,
    }
}

fn split_frontmatter(content: &str) -> Result<(&str, &str)> {
    let content = content.trim_start();
    if !content.starts_with("---") {
        bail!("missing opening '---' frontmatter delimiter");
    }
    let after_open = &content[3..];

    // Accept either a proper \n--- closing delimiter or a top-level \n## markdown
    // heading as the body separator. AI-generated files often place the closing ---
    // at the end of the document rather than right after the YAML keys.
    let dash_pos = after_open.find("\n---");
    let head_pos = after_open.find("\n##");

    let (close_pos, body_offset) = match (dash_pos, head_pos) {
        (Some(d), Some(h)) if h < d => (h, 1), // ## comes before --- : use ## as split
        (Some(d), _) => (d, 4),                 // standard \n--- delimiter
        (None, Some(h)) => (h, 1),              // no ---, fall back to ##
        (None, None) => bail!("missing closing '---' frontmatter delimiter"),
    };

    let yaml = after_open[..close_pos].trim();
    let body = after_open[close_pos + body_offset..].trim_start_matches('\n');
    Ok((yaml, body))
}

pub fn parse_agent_def(content: &str) -> Result<CustomAgentDef> {
    let (yaml, body) = split_frontmatter(content)?;

    #[derive(Deserialize)]
    struct Frontmatter {
        name: String,
        description: String,
        model: String,
        #[serde(default)]
        tools: Vec<String>,
    }

    // First try strict parse (tools as YAML list).
    // If that fails, retry with tools as a plain string and split on commas —
    // this tolerates AI-generated files that write `tools: Read, Write` instead
    // of the correct `tools: [Read, Write]`.
    let fm: Frontmatter = serde_yaml::from_str(yaml).or_else(|_| {
        #[derive(Deserialize)]
        struct FallbackFm {
            name: String,
            description: String,
            model: String,
            #[serde(default)]
            tools: String,
        }
        let fb: FallbackFm =
            serde_yaml::from_str(yaml).context("failed to parse agent frontmatter")?;
        Ok::<Frontmatter, anyhow::Error>(Frontmatter {
            name: fb.name,
            description: fb.description,
            model: fb.model,
            tools: fb
                .tools
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        })
    })?;

    Ok(CustomAgentDef {
        name: fm.name,
        description: fm.description,
        model: fm.model,
        tools: fm.tools,
        system_prompt: body.to_string(),
    })
}

pub fn parse_workflow_def(content: &str) -> Result<CustomWorkflowDef> {
    let (yaml, body) = split_frontmatter(content)?;

    #[derive(Deserialize)]
    struct Frontmatter {
        #[serde(default)]
        name: String,
        description: String,
        agents: Vec<WorkflowStep>,
    }

    let fm: Frontmatter =
        serde_yaml::from_str(yaml).context("failed to parse workflow frontmatter")?;

    Ok(CustomWorkflowDef {
        name: fm.name,
        description: fm.description,
        agents: fm.agents,
        body: body.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agent_def_valid() {
        let content = "---\nname: designer\ndescription: UI designer\nmodel: ollama/qwen2.5:32b\ntools: [filesystem]\n---\nYou are a designer.\n";
        let def = parse_agent_def(content).unwrap();
        assert_eq!(def.name, "designer");
        assert_eq!(def.model, "ollama/qwen2.5:32b");
        assert_eq!(def.tools, vec!["filesystem"]);
        assert!(def.system_prompt.contains("designer"));
    }

    #[test]
    fn parse_workflow_def_valid() {
        let content = "---\nname: sprint\ndescription: Design sprint\nagents:\n  - role: researcher\n    agent: researcher\n  - role: designer\n    agent: designer\n---\nA sprint workflow.\n";
        let def = parse_workflow_def(content).unwrap();
        assert_eq!(def.name, "sprint");
        assert_eq!(def.agents.len(), 2);
        assert_eq!(def.agents[0].role, "researcher");
        assert_eq!(def.agents[1].agent, "designer");
    }

    #[test]
    fn parse_agent_def_missing_delimiter() {
        let content = "name: designer\nmodel: ollama/foo\n";
        assert!(parse_agent_def(content).is_err());
    }
}
