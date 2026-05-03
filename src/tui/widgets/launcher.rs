use crate::agent_loader::AgentLoader;
use crate::tui::theme::THEME;
use crate::workflows::AVAILABLE_WORKFLOWS;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub struct LauncherData {
    pub builtin_workflows: Vec<(String, String)>,
    pub custom_workflows: Vec<(String, String)>,
    pub custom_agents: Vec<(String, String)>,
}

impl LauncherData {
    pub fn load() -> Self {
        let project_root = std::env::current_dir().ok();
        let builtin_workflows = AVAILABLE_WORKFLOWS
            .iter()
            .map(|w| (w.name.to_string(), w.description.to_string()))
            .collect();
        let custom_workflows = AgentLoader::list_workflows(project_root.as_deref())
            .into_iter()
            .map(|w| (w.name, w.description))
            .collect();
        let custom_agents = AgentLoader::list_agents(project_root.as_deref())
            .into_iter()
            .map(|a| (a.name.clone(), format!("{}  ({})", a.description, a.model)))
            .collect();
        Self {
            builtin_workflows,
            custom_workflows,
            custom_agents,
        }
    }

    pub fn all_workflows(&self) -> impl Iterator<Item = (&str, bool)> {
        self.builtin_workflows
            .iter()
            .map(|(n, _)| (n.as_str(), false))
            .chain(
                self.custom_workflows
                    .iter()
                    .map(|(n, _)| (n.as_str(), true)),
            )
    }
}

/// Renders the agents panel idle state: workflow list + optional custom agents + hint.
pub struct LauncherWidget<'a> {
    pub data: &'a LauncherData,
}

impl LauncherWidget<'_> {
    pub fn render(self, frame: &mut Frame, area: Rect) {
        let has_agents = !self.data.custom_agents.is_empty();

        let wf_count = self.data.builtin_workflows.len() + self.data.custom_workflows.len();
        let wf_height = (wf_count as u16 + 2).max(3);
        let agent_height = if has_agents {
            (self.data.custom_agents.len() as u16 + 2).max(3)
        } else {
            0
        };

        let constraints: Vec<Constraint> = if has_agents {
            vec![
                Constraint::Length(wf_height),
                Constraint::Length(agent_height),
                Constraint::Length(1),
                Constraint::Min(0),
            ]
        } else {
            vec![
                Constraint::Length(wf_height),
                Constraint::Length(1),
                Constraint::Min(0),
            ]
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // --- Workflows section ---
        let mut wf_lines: Vec<Line> = Vec::new();
        for (name, desc) in &self.data.builtin_workflows {
            wf_lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<18}", name),
                    Style::default().fg(THEME.primary),
                ),
                Span::styled(desc.clone(), Style::default().fg(THEME.text)),
            ]));
        }
        for (name, desc) in &self.data.custom_workflows {
            wf_lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<16}", name),
                    Style::default().fg(THEME.success),
                ),
                Span::styled(
                    " ★  ",
                    Style::default()
                        .fg(THEME.success)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(desc.clone(), Style::default().fg(THEME.text)),
            ]));
        }

        let wf_block = Block::default()
            .title(Span::styled(" Workflows ", THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.border_style());
        frame.render_widget(Paragraph::new(wf_lines).block(wf_block), chunks[0]);

        // --- Custom Agents section ---
        if has_agents {
            let agent_lines: Vec<Line> = self
                .data
                .custom_agents
                .iter()
                .map(|(name, desc)| {
                    Line::from(vec![
                        Span::styled(
                            format!("  {:<18}", name),
                            Style::default().fg(THEME.secondary),
                        ),
                        Span::styled(desc.clone(), Style::default().fg(THEME.muted)),
                    ])
                })
                .collect();

            let agent_block = Block::default()
                .title(Span::styled(" Custom Agents ", THEME.title_style()))
                .borders(Borders::ALL)
                .border_style(THEME.border_style());
            frame.render_widget(Paragraph::new(agent_lines).block(agent_block), chunks[1]);
        }

        // --- Hint line ---
        let hint_idx = if has_agents { 2 } else { 1 };
        if hint_idx < chunks.len() {
            let hint = Paragraph::new(Line::from(vec![
                Span::styled(
                    "  /start ",
                    Style::default()
                        .fg(THEME.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "<workflow>  to launch  ·  ",
                    Style::default().fg(THEME.muted),
                ),
                Span::styled(
                    "/workflow create ",
                    Style::default()
                        .fg(THEME.primary)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("<name>  to add", Style::default().fg(THEME.muted)),
            ]));
            frame.render_widget(hint, chunks[hint_idx]);
        }
    }
}

/// Renders the pipeline bar idle state: compact list of all workflow names.
pub struct IdlePipelineWidget<'a> {
    pub data: &'a LauncherData,
}

impl IdlePipelineWidget<'_> {
    pub fn render(self, frame: &mut Frame, area: Rect) {
        let mut spans: Vec<Span> = vec![Span::raw("  ")];

        for (name, is_custom) in self.data.all_workflows() {
            if is_custom {
                spans.push(Span::styled(
                    format!("◇ {} ★  ", name),
                    Style::default().fg(THEME.success),
                ));
            } else {
                spans.push(Span::styled(
                    format!("◇ {}  ", name),
                    Style::default().fg(THEME.muted),
                ));
            }
        }

        let block = Block::default()
            .title(Span::styled(" Pipeline ", THEME.title_style()))
            .borders(Borders::ALL)
            .border_style(THEME.border_style());

        frame.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
    }
}
