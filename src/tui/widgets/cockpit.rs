use crate::run_report::{FileRunRecord, RunReport};
use crate::tui::theme::THEME;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
};

#[derive(Debug)]
pub struct CockpitPanel<'a> {
    pub files: &'a [String],
    pub git_hash: &'a Option<String>,
    pub active_tab: &'a CockpitTabDisplay,
    pub duration_secs: u64,
    pub report: &'a Option<RunReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CockpitTabDisplay {
    Summary,
    Files,
    Agents,
    Timeline,
}

impl CockpitTabDisplay {
    fn all() -> [Self; 4] {
        [Self::Summary, Self::Files, Self::Agents, Self::Timeline]
    }

    fn label(&self) -> &str {
        match self {
            Self::Summary => " Summary ",
            Self::Files => " Files ",
            Self::Agents => " Agents ",
            Self::Timeline => " Timeline ",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Self::Summary => 0,
            Self::Files => 1,
            Self::Agents => 2,
            Self::Timeline => 3,
        }
    }

    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Self::Summary,
            1 => Self::Files,
            2 => Self::Agents,
            _ => Self::Timeline,
        }
    }
}

fn duration_str(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    }
}

impl<'a> CockpitPanel<'a> {
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(" Cockpit ", THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.border_style());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(inner);

        let active_tab = self.active_tab;
        let all_tabs = CockpitTabDisplay::all();
        let tab_titles: Vec<Line> = all_tabs
            .iter()
            .map(|t| {
                let selected = t == active_tab;
                let style = if selected {
                    Style::default()
                        .fg(THEME.primary)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(THEME.muted)
                };
                Line::from(Span::styled(t.label(), style))
            })
            .collect();

        let tabs = Tabs::new(tab_titles)
            .select(active_tab.index())
            .highlight_style(
                Style::default()
                    .fg(THEME.primary)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, chunks[0]);

        match self.active_tab {
            CockpitTabDisplay::Summary => self.render_summary(frame, chunks[1]),
            CockpitTabDisplay::Files => self.render_files(frame, chunks[1]),
            CockpitTabDisplay::Agents => self.render_agents(frame, chunks[1]),
            CockpitTabDisplay::Timeline => self.render_timeline(frame, chunks[1]),
        }
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().fg(THEME.muted)),
            Span::styled(
                "✓ Complete",
                Style::default()
                    .fg(THEME.success)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Duration: ", Style::default().fg(THEME.muted)),
            Span::styled(
                duration_str(self.duration_secs),
                Style::default().fg(THEME.text),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Files: ", Style::default().fg(THEME.muted)),
            Span::styled(
                format!("{}", self.files.len()),
                Style::default().fg(THEME.text),
            ),
        ]));

        if let Some(report) = self.report {
            lines.push(Line::from(vec![
                Span::styled("Workflow: ", Style::default().fg(THEME.muted)),
                Span::styled(&report.workflow, Style::default().fg(THEME.primary)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Provider: ", Style::default().fg(THEME.muted)),
                Span::styled(&report.provider, Style::default().fg(THEME.secondary)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Agents: ", Style::default().fg(THEME.muted)),
                Span::styled(
                    format!("{}", report.agents.len()),
                    Style::default().fg(THEME.text),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Tool calls: ", Style::default().fg(THEME.muted)),
                Span::styled(
                    format!("{}", report.metrics.tool_call_count),
                    Style::default().fg(THEME.text),
                ),
            ]));
            if let Some(tokens) = report.metrics.tokens_total {
                lines.push(Line::from(vec![
                    Span::styled("Tokens: ", Style::default().fg(THEME.muted)),
                    Span::styled(format!("{}", tokens), Style::default().fg(THEME.warning)),
                ]));
            }
            if let Some(cost) = report.metrics.estimated_cost_usd {
                lines.push(Line::from(vec![
                    Span::styled("Estimated cost: ", Style::default().fg(THEME.muted)),
                    Span::styled(format!("${:.4}", cost), Style::default().fg(THEME.warning)),
                ]));
            }
        } else {
            lines.push(Line::from(Span::styled(
                "Report data unavailable",
                Style::default().fg(THEME.muted),
            )));
        }

        if let Some(hash) = self.git_hash {
            lines.push(Line::from(""));
            if !hash.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("Git: ", Style::default().fg(THEME.muted)),
                    Span::styled(hash.as_str(), Style::default().fg(THEME.secondary)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::NONE)
                .style(Style::default().fg(THEME.text)),
        );
        frame.render_widget(paragraph, area);
    }

    fn render_files(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        if !self.files.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("{} files created:", self.files.len()),
                Style::default()
                    .fg(THEME.warning)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            if let Some(report) = self.report {
                let mut by_agent: std::collections::BTreeMap<&str, Vec<&FileRunRecord>> =
                    std::collections::BTreeMap::new();
                for f in &report.files {
                    by_agent.entry(f.agent.as_str()).or_default().push(f);
                }
                for (agent, agent_files) in &by_agent {
                    lines.push(Line::from(Span::styled(
                        format!(" {}:", agent),
                        Style::default()
                            .fg(THEME.secondary)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for f in agent_files {
                        lines.push(Line::from(vec![
                            Span::styled("   ", Style::default()),
                            Span::styled(
                                f.operation.to_uppercase(),
                                Style::default().fg(THEME.primary),
                            ),
                            Span::styled(" ", Style::default()),
                            Span::styled(&f.path, Style::default().fg(THEME.text)),
                            Span::styled(
                                format!(" ({} B)", f.bytes),
                                Style::default().fg(THEME.muted),
                            ),
                        ]));
                    }
                }
            } else {
                for f in self.files {
                    lines.push(Line::from(vec![
                        Span::styled("  📄 ", Style::default()),
                        Span::styled(f.clone(), Style::default().fg(THEME.text)),
                    ]));
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "No files recorded",
                Style::default().fg(THEME.muted),
            )));
        }

        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::NONE)
                    .style(Style::default().fg(THEME.text)),
            ),
            area,
        );
    }

    fn render_agents(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        if let Some(report) = self.report {
            if report.agents.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No agent data available",
                    Style::default().fg(THEME.muted),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{:20}", "Agent"),
                        Style::default()
                            .fg(THEME.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:10}", "Status"),
                        Style::default()
                            .fg(THEME.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:12}", "Duration"),
                        Style::default()
                            .fg(THEME.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Errors",
                        Style::default()
                            .fg(THEME.primary)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    "-".repeat(60),
                    Style::default().fg(THEME.muted),
                )));

                for agent in &report.agents {
                    let status_style = match agent.status {
                        crate::run_report::AgentRunStatus::Done => {
                            Style::default().fg(THEME.success)
                        }
                        crate::run_report::AgentRunStatus::Error => {
                            Style::default().fg(THEME.error)
                        }
                        crate::run_report::AgentRunStatus::Interrupted => {
                            Style::default().fg(THEME.warning)
                        }
                        _ => Style::default().fg(THEME.muted),
                    };
                    let status_str = format!("{:?}", agent.status);
                    let dur = agent
                        .duration_ms
                        .map(|ms| duration_str(ms / 1000))
                        .unwrap_or_else(|| "-".to_string());
                    let errs = if agent.errors.is_empty() {
                        "0".to_string()
                    } else {
                        format!("{}", agent.errors.len())
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{:20}", agent.agent),
                            Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(format!("{:10}", status_str), status_style),
                        Span::styled(format!("{:12}", dur), Style::default().fg(THEME.text)),
                        Span::styled(errs, Style::default().fg(THEME.error)),
                    ]));

                    if let Some(model) = &agent.model {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {:>18}", "model:"),
                                Style::default().fg(THEME.muted),
                            ),
                            Span::styled(
                                format!(" {}", model),
                                Style::default().fg(THEME.secondary),
                            ),
                        ]));
                    }

                    for err in &agent.errors {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {:>18}", "error:"),
                                Style::default().fg(THEME.error),
                            ),
                            Span::styled(format!(" {}", err), Style::default().fg(THEME.text)),
                        ]));
                    }
                }
            }
        } else {
            lines.push(Line::from(Span::styled(
                "Agent details unavailable — install the run report",
                Style::default().fg(THEME.muted),
            )));
        }

        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::NONE)
                    .style(Style::default().fg(THEME.text)),
            ),
            area,
        );
    }

    fn render_timeline(&self, frame: &mut Frame, area: Rect) {
        let mut lines: Vec<Line> = Vec::new();

        if let Some(report) = self.report {
            if report.timeline.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No timeline events recorded",
                    Style::default().fg(THEME.muted),
                )));
            } else {
                for event in &report.timeline {
                    let ts_ms = event.timestamp_unix_ms;
                    let secs = ts_ms / 1000;
                    let ts_str = format!(
                        "{:02}:{:02}:{:02}",
                        (secs / 3600) % 24,
                        (secs / 60) % 60,
                        secs % 60,
                    );

                    let (symbol, color) = match event.event_type.as_str() {
                        "workflow_started" => ("▶", THEME.primary),
                        "workflow_completed" => ("✓", THEME.success),
                        "agent_started" => ("◈", THEME.warning),
                        "agent_completed" => ("◆", THEME.success),
                        "agent_progress" => ("·", THEME.muted),
                        "file_written" => ("📄", THEME.text),
                        "tool_call" => ("⚙", THEME.secondary),
                        _ => ("•", THEME.muted),
                    };

                    let agent_prefix = event
                        .agent
                        .as_deref()
                        .map(|a| format!("[{}] ", a))
                        .unwrap_or_default();
                    let msg = event.message.as_deref().unwrap_or(&event.event_type);

                    lines.push(Line::from(vec![
                        Span::styled(format!("{} ", ts_str), Style::default().fg(THEME.muted)),
                        Span::styled(format!("{} ", symbol), Style::default().fg(color)),
                        Span::styled(
                            agent_prefix,
                            Style::default()
                                .fg(THEME.secondary)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(msg, Style::default().fg(THEME.text)),
                    ]));
                }
            }
        } else {
            for f in self.files {
                lines.push(Line::from(vec![
                    Span::styled("📄 ", Style::default()),
                    Span::styled(f.clone(), Style::default().fg(THEME.text)),
                ]));
            }
            if self.files.is_empty() {
                lines.push(Line::from(Span::styled(
                    "No timeline data available",
                    Style::default().fg(THEME.muted),
                )));
            }
        }

        frame.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::NONE)
                    .style(Style::default().fg(THEME.text)),
            ),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_report::{
        AgentRunRecord, AgentRunStatus, FileRunRecord, RunMetrics, RunReport, RunStatus,
        RunTimelineEvent, ToolRunRecord,
    };
    use ratatui::{Terminal, backend::TestBackend};

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn empty_cockpit() -> CockpitPanel<'static> {
        CockpitPanel {
            files: &[],
            git_hash: &None,
            active_tab: &CockpitTabDisplay::Summary,
            duration_secs: 0,
            report: &None,
        }
    }

    fn populated_report() -> RunReport {
        RunReport {
            schema_version: 2,
            run_id: "test-123".to_string(),
            cortex_version: "0.2.3".to_string(),
            workflow: "dev".to_string(),
            prompt: "build a todo app".to_string(),
            provider: "ollama".to_string(),
            started_at_unix_ms: 1000000,
            finished_at_unix_ms: Some(1125000),
            status: RunStatus::Success,
            timeline: vec![
                RunTimelineEvent {
                    timestamp_unix_ms: 1000000,
                    event_type: "workflow_started".to_string(),
                    agent: None,
                    phase: None,
                    message: Some("started".to_string()),
                    path: None,
                    tool: None,
                },
                RunTimelineEvent {
                    timestamp_unix_ms: 1125000,
                    event_type: "workflow_completed".to_string(),
                    agent: None,
                    phase: None,
                    message: Some("completed".to_string()),
                    path: None,
                    tool: None,
                },
            ],
            agents: vec![AgentRunRecord {
                agent: "CEO".to_string(),
                model: Some("qwen2.5-coder:32b".to_string()),
                status: AgentRunStatus::Done,
                started_at_unix_ms: Some(1001000),
                finished_at_unix_ms: Some(1010000),
                duration_ms: Some(9000),
                token_chunks: 42,
                output_chars: 1200,
                last_progress: Some("done".to_string()),
                errors: vec![],
            }],
            tools: vec![ToolRunRecord {
                agent: "CEO".to_string(),
                tool: "filesystem".to_string(),
                label: "read".to_string(),
                timestamp_unix_ms: 1002000,
                status: "ok".to_string(),
            }],
            files: vec![FileRunRecord {
                agent: "Developer".to_string(),
                path: "src/main.rs".to_string(),
                operation: "write".to_string(),
                bytes: 2048,
                sha256: "abc123".to_string(),
                timestamp_unix_ms: 1050000,
            }],
            metrics: RunMetrics {
                duration_ms: Some(125000),
                tokens_total: Some(15000),
                token_chunks_total: 100,
                output_chars_total: 5000,
                agent_count: 1,
                file_count: 1,
                tool_call_count: 5,
                max_tokens_per_run: 100000,
                max_estimated_cost_usd: 0.01,
                budget_status: crate::budget::BudgetStatus::WithinBudget,
                budget_exceeded_reason: None,
                cost_status: crate::run_report::CostStatus::Estimated,
                estimated_cost_usd: Some(0.0025),
                cost_notes: "estimated".to_string(),
            },
            failure: None,
        }
    }

    #[test]
    fn renders_empty_data() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                empty_cockpit().render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_with_report() {
        let mut terminal = make_terminal();
        let report = populated_report();
        let cockpit = CockpitPanel {
            files: &["src/main.rs".to_string()],
            git_hash: &Some("abc123".to_string()),
            active_tab: &CockpitTabDisplay::Summary,
            duration_secs: 125,
            report: &Some(report),
        };
        terminal
            .draw(|f| {
                let area = f.area();
                cockpit.render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_all_tabs() {
        let mut terminal = make_terminal();
        let report = populated_report();
        for tab in &[
            CockpitTabDisplay::Summary,
            CockpitTabDisplay::Files,
            CockpitTabDisplay::Agents,
            CockpitTabDisplay::Timeline,
        ] {
            let cockpit = CockpitPanel {
                files: &["src/main.rs".to_string()],
                git_hash: &None,
                active_tab: tab,
                duration_secs: 125,
                report: &Some(report.clone()),
            };
            terminal
                .draw(|f| {
                    let area = f.area();
                    cockpit.render(f, area);
                })
                .unwrap();
        }
    }

    #[test]
    fn tab_navigation_logic() {
        assert_eq!(CockpitTabDisplay::Summary.index(), 0);
        assert_eq!(CockpitTabDisplay::Files.index(), 1);
        assert_eq!(CockpitTabDisplay::Agents.index(), 2);
        assert_eq!(CockpitTabDisplay::Timeline.index(), 3);

        assert_eq!(CockpitTabDisplay::from_index(0), CockpitTabDisplay::Summary);
        assert_eq!(CockpitTabDisplay::from_index(1), CockpitTabDisplay::Files);
        assert_eq!(CockpitTabDisplay::from_index(2), CockpitTabDisplay::Agents);
        assert_eq!(
            CockpitTabDisplay::from_index(3),
            CockpitTabDisplay::Timeline
        );
        assert_eq!(
            CockpitTabDisplay::from_index(4),
            CockpitTabDisplay::Timeline
        );
    }
}
