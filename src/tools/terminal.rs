#![allow(dead_code)]

use anyhow::{Result, bail};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

const ALLOWED_COMMANDS: &[&str] = &[
    "cargo", "go", "npm", "pip", "git", "docker", "gh", "curl", "jq", "rg",
];
const DEFAULT_TIMEOUT_SECS: u64 = 120;

fn is_command_allowed(command: &str) -> bool {
    ALLOWED_COMMANDS.contains(&command)
}

#[derive(Debug)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

pub async fn run(
    command: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout_secs: Option<u64>,
) -> Result<CommandOutput> {
    if !is_command_allowed(command) {
        bail!(
            "'{}' is not in the command allowlist: {:?}",
            command,
            ALLOWED_COMMANDS
        );
    }

    let secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);

    let task = async {
        let mut cmd = Command::new(command);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let output = cmd.output().await?;
        let exit_code = output.status.code().unwrap_or(-1);
        Ok::<CommandOutput, anyhow::Error>(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            success: output.status.success(),
            exit_code,
        })
    };

    timeout(Duration::from_secs(secs), task)
        .await
        .map_err(|_| anyhow::anyhow!("command '{}' timed out after {}s", command, secs))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_unlisted_command() {
        assert!(run("sh", &["-c", "echo hi"], None, None).await.is_err());
        assert!(run("bash", &[], None, None).await.is_err());
        assert!(run("python3", &[], None, None).await.is_err());
    }

    #[tokio::test]
    async fn listed_command_runs() {
        // `git --version` should succeed on any dev machine
        let out = run("git", &["--version"], None, None).await.unwrap();
        assert!(out.success);
        assert!(out.stdout.contains("git"));
    }

    #[tokio::test]
    async fn research_cli_commands_are_allowlisted() {
        for command in ["gh", "curl", "jq", "rg"] {
            assert!(
                is_command_allowed(command),
                "{command} should be allowlisted"
            );
        }
    }
}
