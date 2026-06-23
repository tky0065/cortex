//! Per-project persistence for free-form chat conversations.
//!
//! Chats are stored centrally under `~/.cortex/projects/<encoded-dir>/<id>.json`,
//! keyed by the project directory they were started in (like Claude Code's
//! `~/.claude/projects/<encoded-cwd>/`). This survives even if the project is
//! later moved or deleted. Each conversation is one JSON file.
//!
//! Turns are stored as display-ready `{prompt, response}` pairs. The rig message
//! history needed to *continue* a conversation is reconstructed from those turns
//! (see [`ChatSession::rig_history`]), so we don't depend on rig's serde support.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Maximum number of chats retained per project directory; older ones are pruned.
pub const MAX_CHATS_PER_PROJECT: usize = 100;

const SCHEMA_VERSION: u32 = 1;

/// A single prompt/response exchange in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub prompt: String,
    pub response: String,
}

/// A persisted session: a free-form chat or a workflow run, tied to a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub schema_version: u32,
    pub id: String,
    pub project_dir: PathBuf,
    /// `"chat"` for free-form chat, otherwise the workflow name (`"dev"`, …).
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Optional user-given name (reserved for a future `/rename`).
    #[serde(default)]
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub turns: Vec<ChatTurn>,
}

fn default_kind() -> String {
    "chat".to_string()
}

impl ChatSession {
    pub fn new(project_dir: PathBuf) -> Self {
        Self::with_kind(project_dir, "chat")
    }

    pub fn with_kind(project_dir: PathBuf, kind: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            project_dir,
            kind: kind.into(),
            name: None,
            created_at: now,
            updated_at: now,
            turns: Vec::new(),
        }
    }

    /// Short human label for the picker: the name, else the first prompt, truncated.
    pub fn summary(&self) -> String {
        if let Some(name) = self.name.as_ref().filter(|n| !n.trim().is_empty()) {
            return name.trim().to_string();
        }
        match self.turns.first() {
            Some(turn) => {
                let prompt = turn.prompt.replace('\n', " ");
                let prompt = prompt.trim();
                if prompt.is_empty() {
                    "(empty session)".to_string()
                } else if prompt.chars().count() > 55 {
                    let truncated: String = prompt.chars().take(55).collect();
                    format!("{truncated}…")
                } else {
                    prompt.to_string()
                }
            }
            None => "(empty session)".to_string(),
        }
    }

    /// Reconstruct the rig conversation history so the chat can be continued.
    pub fn rig_history(&self) -> Vec<rig::completion::Message> {
        let mut history = Vec::with_capacity(self.turns.len() * 2);
        for turn in &self.turns {
            history.push(rig::completion::Message::user(&turn.prompt));
            history.push(rig::completion::Message::assistant(&turn.response));
        }
        history
    }

    /// Persist this session under the default store root, then prune old chats.
    pub fn save(&self) -> Result<()> {
        let root = store_root().context("could not resolve home directory for chat store")?;
        self.save_in(&root)
    }

    pub fn save_in(&self, root: &Path) -> Result<()> {
        let dir = project_store_dir_in(root, &self.project_dir);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create chat dir: {}", dir.display()))?;
        let path = dir.join(format!("{}.json", self.id));
        let raw = serde_json::to_string_pretty(self).context("Failed to serialize chat session")?;
        std::fs::write(&path, raw)
            .with_context(|| format!("Failed to write chat session: {}", path.display()))?;
        prune_in(root, &self.project_dir);
        Ok(())
    }
}

/// Render a relative "time since last activity" label (e.g. `5m ago`, `2h ago`).
pub fn humanize_since(updated_at: DateTime<Utc>) -> String {
    let secs = (Utc::now() - updated_at).num_seconds().max(0);
    if secs < 45 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", (secs / 60).max(1))
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

/// Root of the per-project session store: `~/.cortex/projects`.
pub fn store_root() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cortex").join("projects"))
}

/// Encode an absolute project directory into a single safe folder name.
pub fn encode_project_dir(dir: &Path) -> String {
    let encoded: String = dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = encoded.trim_matches('-');
    if trimmed.is_empty() {
        "root".to_string()
    } else {
        trimmed.to_string()
    }
}

fn project_store_dir_in(root: &Path, project_dir: &Path) -> PathBuf {
    root.join(encode_project_dir(project_dir))
}

/// Load a single chat session by id from the default store.
pub fn load(project_dir: &Path, id: &str) -> Result<ChatSession> {
    let root = store_root().context("could not resolve home directory for chat store")?;
    load_in(&root, project_dir, id)
}

pub fn load_in(root: &Path, project_dir: &Path, id: &str) -> Result<ChatSession> {
    let path = project_store_dir_in(root, project_dir).join(format!("{id}.json"));
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read chat session: {}", path.display()))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse chat session: {}", path.display()))
}

/// List chats for a project, newest first, capped at [`MAX_CHATS_PER_PROJECT`].
pub fn list_for_project(project_dir: &Path) -> Vec<ChatSession> {
    match store_root() {
        Some(root) => list_in(&root, project_dir),
        None => Vec::new(),
    }
}

pub fn list_in(root: &Path, project_dir: &Path) -> Vec<ChatSession> {
    let dir = project_store_dir_in(root, project_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<ChatSession> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|raw| serde_json::from_str::<ChatSession>(&raw).ok())
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    sessions.truncate(MAX_CHATS_PER_PROJECT);
    sessions
}

/// Delete the oldest chats beyond [`MAX_CHATS_PER_PROJECT`] for a project.
fn prune_in(root: &Path, project_dir: &Path) {
    let dir = project_store_dir_in(root, project_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut sessions: Vec<(PathBuf, DateTime<Utc>)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|e| {
            let path = e.path();
            let raw = std::fs::read_to_string(&path).ok()?;
            let session: ChatSession = serde_json::from_str(&raw).ok()?;
            Some((path, session.updated_at))
        })
        .collect();
    if sessions.len() <= MAX_CHATS_PER_PROJECT {
        return;
    }
    // Oldest first, then drop everything past the cap.
    sessions.sort_by_key(|(_, updated)| *updated);
    let to_remove = sessions.len() - MAX_CHATS_PER_PROJECT;
    for (path, _) in sessions.into_iter().take(to_remove) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "cortex_chat_session_{}_{}_{}",
            tag,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn session_with(project: &Path, prompt: &str, response: &str) -> ChatSession {
        let mut s = ChatSession::new(project.to_path_buf());
        s.turns.push(ChatTurn {
            prompt: prompt.to_string(),
            response: response.to_string(),
        });
        s
    }

    #[test]
    fn save_then_load_round_trips() {
        let root = temp_root("roundtrip");
        let project = PathBuf::from("/Users/test/dev/cortex");
        let session = session_with(&project, "hello", "hi there");
        session.save_in(&root).unwrap();

        let loaded = load_in(&root, &project, &session.id).unwrap();
        assert_eq!(loaded.id, session.id);
        assert_eq!(loaded.project_dir, project);
        assert_eq!(loaded.turns, session.turns);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_sorts_newest_first_and_isolates_by_project() {
        let root = temp_root("list");
        let project_a = PathBuf::from("/Users/test/dev/a");
        let project_b = PathBuf::from("/Users/test/dev/b");

        let mut older = session_with(&project_a, "first", "r1");
        older.updated_at = Utc::now() - chrono::Duration::minutes(10);
        older.save_in(&root).unwrap();

        let newer = session_with(&project_a, "second", "r2");
        newer.save_in(&root).unwrap();

        session_with(&project_b, "other project", "r3")
            .save_in(&root)
            .unwrap();

        let listed = list_in(&root, &project_a);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, newer.id, "newest should be first");
        assert_eq!(listed[1].id, older.id);

        assert_eq!(list_in(&root, &project_b).len(), 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_keeps_only_the_cap() {
        let root = temp_root("prune");
        let project = PathBuf::from("/Users/test/dev/prune");
        for i in 0..(MAX_CHATS_PER_PROJECT + 1) {
            let mut s = session_with(&project, &format!("chat {i}"), "r");
            s.updated_at = Utc::now() + chrono::Duration::seconds(i as i64);
            s.save_in(&root).unwrap();
        }
        let listed = list_in(&root, &project);
        assert_eq!(listed.len(), MAX_CHATS_PER_PROJECT);
        // The very first (oldest) chat should have been pruned.
        assert!(listed.iter().all(|s| s.turns[0].prompt != "chat 0"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rig_history_emits_user_then_assistant_per_turn() {
        let project = PathBuf::from("/tmp/x");
        let mut s = ChatSession::new(project);
        s.turns.push(ChatTurn {
            prompt: "q1".into(),
            response: "a1".into(),
        });
        s.turns.push(ChatTurn {
            prompt: "q2".into(),
            response: "a2".into(),
        });
        let history = s.rig_history();
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn summary_uses_name_then_prompt_and_truncates() {
        let project = PathBuf::from("/tmp/x");
        assert_eq!(
            ChatSession::new(project.clone()).summary(),
            "(empty session)"
        );

        let s = session_with(&project, &"a".repeat(80), "r");
        let summary = s.summary();
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), 56); // 55 + ellipsis

        let mut named = session_with(&project, "first prompt", "r");
        named.name = Some("My session".to_string());
        assert_eq!(named.summary(), "My session");
    }

    #[test]
    fn workflow_session_round_trips_with_kind() {
        let root = temp_root("kind");
        let project = PathBuf::from("/Users/test/dev/cortex");
        let mut s = ChatSession::with_kind(project.clone(), "dev");
        s.turns.push(ChatTurn {
            prompt: "build an app".into(),
            response: "Completed — 3 files".into(),
        });
        s.save_in(&root).unwrap();

        let loaded = load_in(&root, &project, &s.id).unwrap();
        assert_eq!(loaded.kind, "dev");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn humanize_since_buckets() {
        assert_eq!(humanize_since(Utc::now()), "just now");
        assert_eq!(
            humanize_since(Utc::now() - chrono::Duration::minutes(5)),
            "5m ago"
        );
        assert_eq!(
            humanize_since(Utc::now() - chrono::Duration::hours(2)),
            "2h ago"
        );
        assert_eq!(
            humanize_since(Utc::now() - chrono::Duration::days(3)),
            "3d ago"
        );
    }
}
