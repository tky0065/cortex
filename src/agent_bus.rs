/// Inter-agent communication hub.
///
/// The `AgentBus` provides two capabilities:
///   1. **Status registry** — every agent reports its state (Idle/Running/Done/Error)
///      and last output so the assistant can query them at any time.
///   2. **Directive inbox** — the assistant (or a slash command) can push an
///      `AgentDirective` that workflows drain between phases and log/act upon.
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{RwLock, mpsc};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Current lifecycle state of an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Done,
    Error(String),
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Done => write!(f, "done"),
            Self::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}

/// Snapshot of a single agent's state.
#[derive(Debug, Clone)]
pub struct AgentRecord {
    pub status: AgentStatus,
    /// Last textual output produced by the agent (e.g. the brief, specs.md content, …).
    pub output: Option<String>,
    /// Monotonic timestamp of the last update (for display / ordering).
    pub updated_at: Instant,
}

/// An instruction injected by the assistant or a slash command into a running workflow.
#[derive(Debug, Clone)]
pub struct AgentDirective {
    /// The agent this directive targets (e.g. "developer", "*" for all).
    pub target_agent: String,
    /// Free-form instruction in natural language.
    pub instruction: String,
}

// ---------------------------------------------------------------------------
// AgentBus
// ---------------------------------------------------------------------------

pub struct AgentBus {
    agents: RwLock<HashMap<String, AgentRecord>>,
    directive_tx: mpsc::UnboundedSender<AgentDirective>,
    directive_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<AgentDirective>>,
}

impl AgentBus {
    pub fn new() -> Arc<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            agents: RwLock::new(HashMap::new()),
            directive_tx: tx,
            directive_rx: tokio::sync::Mutex::new(rx),
        })
    }

    // ── Status registry ──────────────────────────────────────────────────────

    /// Update an agent's status and (optionally) its last output.
    pub async fn update_agent(&self, name: &str, status: AgentStatus, output: Option<String>) {
        let mut map = self.agents.write().await;
        let record = map.entry(name.to_string()).or_insert(AgentRecord {
            status: AgentStatus::Idle,
            output: None,
            updated_at: Instant::now(),
        });
        record.status = status;
        if output.is_some() {
            record.output = output;
        }
        record.updated_at = Instant::now();
    }

    /// Get a snapshot of a single agent's record.
    pub async fn get_status(&self, name: &str) -> Option<AgentRecord> {
        self.agents.read().await.get(name).cloned()
    }

    /// Get a snapshot of all agent records, sorted by name.
    pub async fn get_all_statuses(&self) -> Vec<(String, AgentRecord)> {
        let map = self.agents.read().await;
        let mut entries: Vec<(String, AgentRecord)> =
            map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        entries
    }

    // ── Directive channel ────────────────────────────────────────────────────

    /// Push a directive into the inbox.
    ///
    /// Returns `Err` only if the receiver side has been dropped (which should
    /// never happen while a workflow is running).
    pub fn send_directive(&self, directive: AgentDirective) -> anyhow::Result<()> {
        self.directive_tx
            .send(directive)
            .map_err(|e| anyhow::anyhow!("directive channel closed: {e}"))
    }

    /// Drain all currently pending directives (non-blocking).
    ///
    /// Workflows call this between phases to pick up any injected instructions.
    pub async fn drain_directives(&self) -> Vec<AgentDirective> {
        let mut rx = self.directive_rx.lock().await;
        let mut directives = Vec::new();
        while let Ok(d) = rx.try_recv() {
            directives.push(d);
        }
        directives
    }
}

impl Default for AgentBus {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            agents: RwLock::new(HashMap::new()),
            directive_tx: tx,
            directive_rx: tokio::sync::Mutex::new(rx),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_query_agent_status() {
        let bus = AgentBus::new();
        bus.update_agent("ceo", AgentStatus::Running, None).await;
        let rec = bus.get_status("ceo").await.expect("agent not found");
        assert_eq!(rec.status, AgentStatus::Running);
        assert!(rec.output.is_none());
    }

    #[tokio::test]
    async fn output_is_stored_on_done() {
        let bus = AgentBus::new();
        bus.update_agent("pm", AgentStatus::Done, Some("## Specs".to_string()))
            .await;
        let rec = bus.get_status("pm").await.expect("agent not found");
        assert_eq!(rec.status, AgentStatus::Done);
        assert_eq!(rec.output.as_deref(), Some("## Specs"));
    }

    #[tokio::test]
    async fn directive_round_trip() {
        let bus = AgentBus::new();
        bus.send_directive(AgentDirective {
            target_agent: "developer".to_string(),
            instruction: "Add tests for the auth module".to_string(),
        })
        .expect("send failed");

        let directives = bus.drain_directives().await;
        assert_eq!(directives.len(), 1);
        assert_eq!(directives[0].target_agent, "developer");
    }

    #[tokio::test]
    async fn drain_is_non_blocking_when_empty() {
        let bus = AgentBus::new();
        let directives = bus.drain_directives().await;
        assert!(directives.is_empty());
    }

    #[tokio::test]
    async fn get_all_statuses_returns_sorted_entries() {
        let bus = AgentBus::new();
        bus.update_agent("pm", AgentStatus::Done, None).await;
        bus.update_agent("ceo", AgentStatus::Idle, None).await;
        let all = bus.get_all_statuses().await;
        assert_eq!(all[0].0, "ceo");
        assert_eq!(all[1].0, "pm");
    }
}
