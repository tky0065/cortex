use crate::{auth, providers::registry, tui::theme::THEME, workflows};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use tui_input::Input;

/// All slash commands with their short descriptions.
const COMMANDS: &[(&str, &str)] = &[
    ("/start", "Launch a workflow"),
    ("/run", "Alias for /start"),
    ("/resume", "Resume an interrupted workflow"),
    ("/init", "Generate or update AGENTS.md"),
    ("/status", "Show current workflow status"),
    ("/abort", "Cancel the running workflow"),
    ("/continue", "Resume an interactive pause"),
    ("/config", "Print active configuration"),
    ("/model", "Switch model"),
    ("/provider", "Switch provider"),
    ("/connect", "Connect provider auth"),
    (
        "/apikey",
        "Set an API key (openrouter/groq/together/web_search)",
    ),
    (
        "/websearch",
        "Toggle web search for all agents (enable|disable)",
    ),
    ("/skill", "Browse and manage skills"),
    ("/update", "Check for or install Cortex updates"),
    ("/focus", "Focus logs by agent"),
    ("/clear", "Clear visible logs"),
    ("/logs", "Toggle log panel"),
    ("/help", "Show all commands"),
    ("/agents", "List all agent statuses from the bus"),
    (
        "/agent",
        "Send a directive to a running agent: /agent <name> \"<msg>\"",
    ),
    ("/quit", "Exit cortex"),
    ("/exit", "Exit cortex"),
];

const MODEL_ROLES: &[(&str, &str)] = &[
    ("ceo", "CEO role model"),
    ("pm", "Product manager role model"),
    ("tech_lead", "Tech lead role model"),
    ("developer", "Developer role model"),
    ("qa", "QA role model"),
    ("devops", "DevOps role model"),
    ("cortex", "Cortex agent model"),
    ("all", "Set every role model"),
];

const WEBSEARCH_ACTIONS: &[(&str, &str)] = &[
    ("enable", "Enable web search context"),
    ("disable", "Disable web search context"),
];

const UPDATE_ACTIONS: &[(&str, &str)] = &[
    ("check", "Check for updates"),
    ("install", "Install the latest update"),
];

const SKILL_ACTIONS: &[(&str, &str)] = &[
    ("list", "List installed skills"),
    ("show", "Show an installed skill"),
    ("add", "Add a skill from a source"),
    ("install", "Alias for add"),
    ("enable", "Enable an installed skill"),
    ("disable", "Disable an installed skill"),
    ("remove", "Remove an installed skill"),
    ("create", "Create a local skill"),
    ("help", "Show skill help"),
];

const FOCUS_TARGETS: &[(&str, &str)] = &[
    ("all", "Show logs for all agents"),
    ("off", "Clear the active log filter"),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaletteItem {
    pub value: String,
    pub description: String,
    pub replacement: String,
}

impl PaletteItem {
    fn new(value: impl Into<String>, description: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            replacement: quote_arg_if_needed(&value),
            value,
            description: description.into(),
        }
    }

    fn with_replacement(
        value: impl Into<String>,
        description: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            value: value.into(),
            description: description.into(),
            replacement: replacement.into(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PaletteContext {
    pub providers: Vec<(String, String)>,
    pub models: Vec<String>,
    pub agents: Vec<String>,
    pub resume_sessions: Vec<ResumeSuggestion>,
    pub skills: Vec<(String, String)>,
    pub project_paths: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumeSuggestion {
    pub label: String,
    pub description: String,
    pub path: String,
}

pub struct InputBar {
    pub input: Input,
    /// Past commands, oldest first.
    history: Vec<String>,
    /// Current position while navigating history (None = live draft).
    history_idx: Option<usize>,
    /// Text that was in the input before Up was first pressed.
    saved_draft: String,
    // Legacy fields kept for test compatibility
    /// @deprecated — use palette_idx instead
    pub completion_prefix: Option<String>,
    /// @deprecated — use palette_idx instead
    pub completion_idx: Option<usize>,
    /// Index of the currently highlighted row in the command palette.
    palette_idx: usize,
}

impl InputBar {
    pub fn new() -> Self {
        Self {
            input: Input::default(),
            history: Vec::new(),
            history_idx: None,
            saved_draft: String::new(),
            completion_prefix: None,
            completion_idx: None,
            palette_idx: 0,
        }
    }

    // -----------------------------------------------------------------------
    // Command palette
    // -----------------------------------------------------------------------

    /// Returns true when a command or argument palette should be visible.
    pub fn palette_open(&self, context: &PaletteContext) -> bool {
        let v = self.input.value();
        self.mention_replacement(context).is_some()
            || self.argument_replacement(context).is_some()
            || (v.starts_with('/') && !v.contains(' '))
    }

    /// Commands or arguments that match what the user has typed so far.
    pub fn palette_matches(&self, context: &PaletteContext) -> Vec<PaletteItem> {
        if let Some(mention) = self.mention_replacement(context) {
            return mention.matches;
        }

        if let Some(arg) = self.argument_replacement(context) {
            return arg.matches;
        }

        let prefix = self.input.value();
        if !prefix.starts_with('/') {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .map(|(cmd, desc)| PaletteItem::new(*cmd, *desc))
            .collect()
    }

    /// Move the palette cursor up.
    pub fn palette_up(&mut self) {
        if self.palette_idx > 0 {
            self.palette_idx -= 1;
        }
    }

    /// Move the palette cursor down.
    pub fn palette_down(&mut self, context: &PaletteContext) {
        let len = self.palette_matches(context).len();
        if len > 0 && self.palette_idx + 1 < len {
            self.palette_idx += 1;
        }
    }

    /// Select the currently highlighted palette item — replaces the input value.
    /// Returns the selected command string (e.g. "/start"), or None if palette is empty.
    pub fn palette_select(&mut self, context: &PaletteContext) -> Option<String> {
        if let Some(mention) = self.mention_replacement(context) {
            if mention.matches.is_empty() {
                return None;
            }
            let idx = self.palette_idx.min(mention.matches.len() - 1);
            let item = &mention.matches[idx];
            let mut value = String::new();
            value.push_str(&mention.before);
            value.push_str(&item.replacement);
            value.push_str(&mention.after);
            if mention.append_space && !value.ends_with(' ') {
                value.push(' ');
            }
            self.input = Input::new(value.clone());
            self.palette_idx = 0;
            return Some(value);
        }

        if let Some(arg) = self.argument_replacement(context) {
            if arg.matches.is_empty() {
                return None;
            }
            let idx = self.palette_idx.min(arg.matches.len() - 1);
            let item = &arg.matches[idx];
            let mut value = String::new();
            value.push_str(&arg.before);
            value.push_str(&item.replacement);
            value.push_str(&arg.after);
            if arg.append_space && !value.ends_with(' ') {
                value.push(' ');
            }
            self.input = Input::new(value.clone());
            self.palette_idx = 0;
            return Some(value);
        }

        let matches = self.palette_matches(context);
        if matches.is_empty() {
            return None;
        }
        let idx = self.palette_idx.min(matches.len() - 1);
        let item = &matches[idx].value;

        // Commands that take arguments get a trailing space; others are complete.
        let value = if REQUIRES_ARGS.contains(&item.as_str()) {
            format!("{} ", item)
        } else {
            item.to_string()
        };
        self.input = Input::new(value.clone());
        self.palette_idx = 0;
        Some(value)
    }

    fn mention_replacement(&self, context: &PaletteContext) -> Option<ArgumentPalette> {
        let value = self.input.value();
        let cursor = self.input.cursor().min(value.len());
        let before_cursor = &value[..cursor];
        let token_start = before_cursor
            .char_indices()
            .rev()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx + ch.len_utf8()))
            .unwrap_or(0);
        let token = &value[token_start..cursor];
        let trigger = token.chars().next()?;
        if !matches!(trigger, '@' | '$') {
            return None;
        }
        if token[trigger.len_utf8()..].contains('@') || token[trigger.len_utf8()..].contains('$') {
            return None;
        }

        let prefix = &token[trigger.len_utf8()..];
        let matches = match trigger {
            '@' => context
                .project_paths
                .iter()
                .filter(|(path, _)| mention_matches(path, prefix))
                .map(|(path, description)| {
                    PaletteItem::with_replacement(
                        format!("@{path}"),
                        description,
                        format!("@{path}"),
                    )
                })
                .collect(),
            '$' => context
                .skills
                .iter()
                .filter(|(name, _)| mention_matches(name, prefix))
                .map(|(name, description)| {
                    PaletteItem::with_replacement(
                        format!("${name}"),
                        description,
                        format!("${name}"),
                    )
                })
                .collect(),
            _ => Vec::new(),
        };
        if matches.is_empty() {
            return None;
        }

        Some(ArgumentPalette {
            before: value[..token_start].to_string(),
            after: value[cursor..].to_string(),
            append_space: true,
            matches,
        })
    }

    fn argument_replacement(&self, context: &PaletteContext) -> Option<ArgumentPalette> {
        let parsed = ParsedCommand::parse(self.input.value())?;
        if parsed.command.is_empty() || parsed.current.quoted {
            return None;
        }

        let matches = argument_matches(&parsed, context);
        if matches.is_empty() {
            return None;
        }

        Some(ArgumentPalette {
            before: self.input.value()[..parsed.current.start].to_string(),
            after: self.input.value()[parsed.current.end..].to_string(),
            append_space: append_space_after_argument(&parsed),
            matches,
        })
    }

    // -----------------------------------------------------------------------
    // History
    // -----------------------------------------------------------------------

    pub fn push_history(&mut self, cmd: String) {
        if cmd.is_empty() {
            return;
        }
        if self.history.last().map(|s| s.as_str()) != Some(&cmd) {
            self.history.push(cmd);
        }
        self.history_idx = None;
        self.saved_draft.clear();
        self.completion_prefix = None;
        self.completion_idx = None;
        self.palette_idx = 0;
    }

    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let new_idx = match self.history_idx {
            None => {
                self.saved_draft = self.input.value().to_string();
                self.history.len() - 1
            }
            Some(0) => 0,
            Some(i) => i - 1,
        };
        self.history_idx = Some(new_idx);
        self.input = Input::new(self.history[new_idx].clone());
        self.completion_prefix = None;
        self.completion_idx = None;
    }

    pub fn history_down(&mut self) {
        match self.history_idx {
            None => {}
            Some(i) if i + 1 >= self.history.len() => {
                self.history_idx = None;
                self.input = Input::new(self.saved_draft.clone());
                self.completion_prefix = None;
                self.completion_idx = None;
            }
            Some(i) => {
                self.history_idx = Some(i + 1);
                self.input = Input::new(self.history[i + 1].clone());
                self.completion_prefix = None;
                self.completion_idx = None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Legacy Tab-completion (kept for test compatibility)
    // -----------------------------------------------------------------------

    pub fn active_suggestions(&self) -> Vec<&'static str> {
        let prefix = self
            .completion_prefix
            .as_deref()
            .unwrap_or_else(|| self.input.value());
        if prefix.is_empty() || !prefix.starts_with('/') {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .map(|(cmd, _)| *cmd)
            .collect()
    }

    pub fn complete(&mut self) {
        if self.completion_prefix.is_none() {
            self.completion_prefix = Some(self.input.value().to_string());
        }
        let suggestions = self.active_suggestions();
        if suggestions.is_empty() {
            self.completion_prefix = None;
            return;
        }
        if suggestions.len() == 1 {
            let cmd = suggestions[0];
            let new_val = if REQUIRES_ARGS.contains(&cmd) {
                format!("{} ", cmd)
            } else {
                cmd.to_string()
            };
            self.input = Input::new(new_val);
            self.completion_prefix = None;
            self.completion_idx = None;
            return;
        }
        let next_idx = match self.completion_idx {
            None => 0,
            Some(i) => (i + 1) % suggestions.len(),
        };
        self.completion_idx = Some(next_idx);
        self.input = Input::new(suggestions[next_idx].to_string());
    }

    pub fn dismiss_completions(&mut self) {
        self.completion_prefix = None;
        self.completion_idx = None;
        self.palette_idx = 0;
    }

    fn get_wrapped_lines(&self, width: usize) -> (Vec<Line<'_>>, (u16, u16)) {
        let value = self.input.value();
        let mut lines = Vec::new();
        let mut cursor_coords = (0, 0);

        let mut current_line = String::new();
        let mut x = 2; // initial "> " prefix
        let mut y = 0;

        current_line.push_str("> ");

        for (i, c) in value.chars().enumerate() {
            if i == self.input.cursor() {
                cursor_coords = (x as u16, y as u16);
            }

            if c == '\n' {
                lines.push(Line::from(current_line));
                current_line = String::from("  ");
                y += 1;
                x = 2;
            } else {
                current_line.push(c);
                x += 1;
                if x >= width.saturating_sub(2) {
                    lines.push(Line::from(current_line));
                    current_line = String::from("  ");
                    y += 1;
                    x = 2;
                }
            }
        }

        if value.chars().count() == self.input.cursor() {
            cursor_coords = (x as u16, y as u16);
        }

        lines.push(Line::from(current_line));
        (lines, cursor_coords)
    }

    pub fn line_count(&self, width: usize) -> usize {
        let (lines, _) = self.get_wrapped_lines(width);
        lines.len()
    }

    // -----------------------------------------------------------------------
    // Render
    // -----------------------------------------------------------------------

    /// Render just the input bar into `area`.
    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let (lines, (cursor_x, cursor_y)) = self.get_wrapped_lines(area.width as usize);

        let widget = Paragraph::new(lines)
            .style(Style::default().fg(THEME.text))
            .block(
                Block::default()
                    .title(Span::styled(" Command ", THEME.title_style()))
                    .borders(Borders::ALL)
                    .border_style(THEME.border_style()),
            );
        frame.render_widget(widget, area);

        frame.set_cursor_position((area.x + 1 + cursor_x, area.y + 1 + cursor_y));
    }

    /// Render the command palette as a floating overlay anchored above `input_area`.
    /// Call this after `render()`, passing the full terminal area and the input bar rect.
    pub fn render_palette(
        &self,
        frame: &mut Frame,
        full_area: Rect,
        input_area: Rect,
        context: &PaletteContext,
    ) {
        if !self.palette_open(context) {
            return;
        }
        let matches = self.palette_matches(context);
        if matches.is_empty() {
            return;
        }

        // Palette height: one row per match + 2 border rows, capped so it doesn't exceed screen.
        let palette_h =
            (matches.len() as u16 + 2).min(full_area.height.saturating_sub(input_area.height + 2));
        if palette_h < 3 {
            return;
        }

        // Position the palette directly above the input bar, same horizontal span.
        let palette_area = Rect {
            x: input_area.x,
            y: input_area.y.saturating_sub(palette_h),
            width: input_area.width,
            height: palette_h,
        };

        let cursor = self.palette_idx.min(matches.len().saturating_sub(1));
        let cmd_col = matches
            .iter()
            .map(|item| item.value.len())
            .max()
            .unwrap_or(8)
            + 3;

        use ratatui::widgets::Clear;
        frame.render_widget(Clear, palette_area);

        let items: Vec<ListItem> = matches
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let selected = i == cursor;
                let (cmd_style, desc_style, row_bg) = if selected {
                    (
                        Style::default()
                            .fg(THEME.text)
                            .bg(THEME.secondary)
                            .add_modifier(Modifier::BOLD),
                        Style::default().fg(THEME.text).bg(THEME.secondary),
                        THEME.secondary,
                    )
                } else {
                    (
                        Style::default().fg(THEME.primary),
                        Style::default().fg(THEME.muted),
                        Color::Reset,
                    )
                };
                let cmd_padded = format!(" {:<width$}", item.value, width = cmd_col);
                let line = Line::from(vec![
                    Span::styled(cmd_padded, cmd_style),
                    Span::styled(item.description.clone(), desc_style),
                ])
                .style(Style::default().bg(row_bg));
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(THEME.border_style())
                .style(Style::default().bg(THEME.bg)),
        );
        frame.render_widget(list, palette_area);
    }
}

/// Commands that require arguments and get a trailing space when selected.
const REQUIRES_ARGS: &[&str] = &[
    "/start",
    "/run",
    "/resume",
    "/focus",
    "/apikey",
    "/websearch",
    "/skill",
    "/update",
    "/model",
    "/provider",
    "/connect",
];

struct ArgumentPalette {
    before: String,
    after: String,
    append_space: bool,
    matches: Vec<PaletteItem>,
}

#[derive(Debug)]
struct ParsedCommand<'a> {
    command: &'a str,
    args: Vec<ParsedArg<'a>>,
    current: ParsedArg<'a>,
}

#[derive(Clone, Copy, Debug)]
struct ParsedArg<'a> {
    index: usize,
    start: usize,
    end: usize,
    text: &'a str,
    quoted: bool,
}

impl<'a> ParsedCommand<'a> {
    fn parse(value: &'a str) -> Option<Self> {
        if !value.starts_with('/') {
            return None;
        }
        let command_end = value.find(char::is_whitespace)?;
        let command = &value[..command_end];
        let mut args = Vec::new();
        let mut pos = command_end;
        let bytes_len = value.len();

        while pos < bytes_len {
            let skipped = skip_whitespace(value, pos);
            if skipped == bytes_len {
                break;
            }
            pos = skipped;
            let arg_start = pos;
            let mut chars = value[pos..].char_indices();
            let (_, first) = chars.next()?;
            if first == '"' {
                let content_start = pos + first.len_utf8();
                let mut content_end = bytes_len;
                let mut arg_end = bytes_len;
                for (offset, ch) in value[content_start..].char_indices() {
                    if ch == '"' {
                        content_end = content_start + offset;
                        arg_end = content_end + ch.len_utf8();
                        break;
                    }
                }
                args.push(ParsedArg {
                    index: args.len(),
                    start: arg_start,
                    end: arg_end,
                    text: &value[content_start..content_end],
                    quoted: true,
                });
                pos = arg_end;
            } else {
                let mut arg_end = bytes_len;
                for (offset, ch) in value[pos..].char_indices() {
                    if ch.is_whitespace() {
                        arg_end = pos + offset;
                        break;
                    }
                }
                args.push(ParsedArg {
                    index: args.len(),
                    start: arg_start,
                    end: arg_end,
                    text: &value[arg_start..arg_end],
                    quoted: false,
                });
                pos = arg_end;
            }
        }

        let current = if value.ends_with(char::is_whitespace) {
            ParsedArg {
                index: args.len(),
                start: value.len(),
                end: value.len(),
                text: "",
                quoted: false,
            }
        } else {
            *args.last()?
        };

        Some(Self {
            command,
            args,
            current,
        })
    }
}

fn skip_whitespace(value: &str, mut pos: usize) -> usize {
    while pos < value.len() {
        let Some(ch) = value[pos..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn argument_matches(parsed: &ParsedCommand<'_>, context: &PaletteContext) -> Vec<PaletteItem> {
    let candidates = match (parsed.command, parsed.current.index) {
        ("/start" | "/run", 0) => workflows::available_workflows()
            .iter()
            .map(|workflow| PaletteItem::new(workflow.name, workflow.description))
            .collect(),
        ("/websearch", 0) => static_items(WEBSEARCH_ACTIONS),
        ("/update", 0) => static_items(UPDATE_ACTIONS),
        ("/skill" | "/skills", 0) => static_items(SKILL_ACTIONS),
        ("/skill" | "/skills", 1) if skill_command_takes_installed_name(parsed) => context
            .skills
            .iter()
            .map(|(name, description)| PaletteItem::new(name, description))
            .collect(),
        ("/apikey" | "/provider" | "/connect", 0) => context
            .providers
            .iter()
            .map(|(name, description)| PaletteItem::new(name, description))
            .collect(),
        ("/connect", 1) => parsed
            .args
            .first()
            .map(|provider| {
                auth::methods_for_provider(provider.text)
                    .into_iter()
                    .map(|method| PaletteItem::new(method.id, method.description))
                    .collect()
            })
            .unwrap_or_default(),
        ("/model", 0) => static_items(MODEL_ROLES),
        ("/model", 1) => context
            .models
            .iter()
            .map(|model| PaletteItem::new(model, "Cached model"))
            .collect(),
        ("/focus", 0) => FOCUS_TARGETS
            .iter()
            .map(|(value, description)| PaletteItem::new(*value, *description))
            .chain(
                context
                    .agents
                    .iter()
                    .map(|agent| PaletteItem::new(agent, "Active agent")),
            )
            .collect(),
        ("/agent", 0) => context
            .agents
            .iter()
            .map(|agent| PaletteItem::new(agent, "Active agent"))
            .collect(),
        ("/resume", 0) => context
            .resume_sessions
            .iter()
            .map(|session| {
                PaletteItem::with_replacement(
                    &session.label,
                    &session.description,
                    quote_arg_if_needed(&session.path),
                )
            })
            .collect(),
        _ => Vec::new(),
    };

    filter_items(candidates, parsed.current.text)
}

fn static_items(items: &[(&str, &str)]) -> Vec<PaletteItem> {
    items
        .iter()
        .map(|(value, description)| PaletteItem::new(*value, *description))
        .collect()
}

fn filter_items(items: Vec<PaletteItem>, prefix: &str) -> Vec<PaletteItem> {
    if prefix.is_empty() {
        return items;
    }
    let prefix = prefix.to_lowercase();
    items
        .into_iter()
        .filter(|item| {
            let value = item.value.to_lowercase();
            let replacement = item.replacement.to_lowercase();
            value.starts_with(&prefix)
                || replacement.starts_with(&prefix)
                || value.split('/').any(|segment| segment.starts_with(&prefix))
        })
        .collect()
}

fn mention_matches(value: &str, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let value = value.to_ascii_lowercase();
    let prefix = prefix.to_ascii_lowercase();
    value.starts_with(&prefix) || value.split('/').any(|segment| segment.starts_with(&prefix))
}

fn skill_command_takes_installed_name(parsed: &ParsedCommand<'_>) -> bool {
    matches!(
        parsed.args.first().map(|arg| arg.text),
        Some(
            "show" | "enable" | "activate" | "disable" | "deactivate" | "remove" | "delete" | "rm"
        )
    )
}

fn append_space_after_argument(parsed: &ParsedCommand<'_>) -> bool {
    match (parsed.command, parsed.current.index) {
        ("/start" | "/run", 0) => true,
        ("/skill" | "/skills", 0) => true,
        ("/skill" | "/skills", 1) => false,
        ("/apikey", 0) => true,
        ("/connect", 0) => true,
        ("/connect", 1) => false,
        ("/model", 0) => true,
        ("/model", 1) => false,
        ("/agent", 0) => true,
        _ => false,
    }
}

fn quote_arg_if_needed(value: &str) -> String {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        format!("\"{}\"", value.replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

pub fn default_provider_suggestions() -> Vec<(String, String)> {
    registry::BUILTIN_PROVIDERS
        .iter()
        .map(|provider| (provider.id.to_string(), provider.description.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use tui_input::backend::crossterm::EventHandler;

    fn make_terminal() -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(80, 24)).unwrap()
    }

    fn type_into(bar: &mut InputBar, text: &str) {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        for ch in text.chars() {
            bar.input.handle_event(&Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            )));
        }
    }

    fn context() -> PaletteContext {
        PaletteContext {
            providers: vec![
                ("ollama".to_string(), "local Ollama server".to_string()),
                ("openrouter".to_string(), "API key required".to_string()),
                ("web_search".to_string(), "Brave Search API key".to_string()),
            ],
            models: vec![
                "ollama/qwen2.5-coder:32b".to_string(),
                "ollama/llama3.1:8b".to_string(),
            ],
            agents: vec!["ceo".to_string(), "developer:src/main.rs".to_string()],
            resume_sessions: vec![
                ResumeSuggestion {
                    label: "dev build app".to_string(),
                    description: "/tmp/cortex-app".to_string(),
                    path: "/tmp/cortex-app".to_string(),
                },
                ResumeSuggestion {
                    label: "marketing launch".to_string(),
                    description: "/tmp/cortex project".to_string(),
                    path: "/tmp/cortex project".to_string(),
                },
            ],
            skills: vec![
                ("rust".to_string(), "Rust workflow skill".to_string()),
                ("docs".to_string(), "Documentation skill".to_string()),
            ],
            project_paths: vec![
                ("src/".to_string(), "Directory".to_string()),
                ("src/main.rs".to_string(), "File".to_string()),
                ("README.md".to_string(), "File".to_string()),
            ],
        }
    }

    fn values(items: Vec<PaletteItem>) -> Vec<String> {
        items.into_iter().map(|item| item.value).collect()
    }

    #[test]
    fn renders_input_bar() {
        let mut terminal = make_terminal();
        terminal
            .draw(|f| {
                let area = f.area();
                InputBar::new().render(f, area);
            })
            .unwrap();
    }

    #[test]
    fn renders_with_text() {
        let mut terminal = make_terminal();
        let mut bar = InputBar::new();
        type_into(&mut bar, "hello world");
        terminal
            .draw(|f| {
                let area = f.area();
                bar.render(f, area);
            })
            .unwrap();
    }

    // -----------------------------------------------------------------------
    // History tests
    // -----------------------------------------------------------------------

    #[test]
    fn history_navigates_up_and_down() {
        let mut bar = InputBar::new();
        bar.push_history("/start dev \"idea1\"".to_string());
        bar.push_history("/status".to_string());

        bar.history_up();
        assert_eq!(bar.input.value(), "/status");

        bar.history_up();
        assert_eq!(bar.input.value(), "/start dev \"idea1\"");

        bar.history_down();
        assert_eq!(bar.input.value(), "/status");

        bar.history_down();
        assert_eq!(bar.input.value(), "");
        assert!(bar.history_idx.is_none());
    }

    #[test]
    fn history_deduplicates() {
        let mut bar = InputBar::new();
        bar.push_history("/status".to_string());
        bar.push_history("/status".to_string());
        assert_eq!(bar.history.len(), 1);
    }

    #[test]
    fn history_restores_draft() {
        let mut bar = InputBar::new();
        bar.push_history("/help".to_string());
        type_into(&mut bar, "/sta");

        bar.history_up();
        assert_eq!(bar.input.value(), "/help");

        bar.history_down();
        assert_eq!(bar.input.value(), "/sta");
    }

    #[test]
    fn history_up_at_top_stays() {
        let mut bar = InputBar::new();
        bar.push_history("/help".to_string());
        bar.history_up();
        bar.history_up();
        assert_eq!(bar.input.value(), "/help");
    }

    // -----------------------------------------------------------------------
    // Palette tests
    // -----------------------------------------------------------------------

    #[test]
    fn tab_completes_unique_prefix() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/sta");
        let suggestions = bar.active_suggestions();
        assert!(suggestions.len() >= 2);
    }

    #[test]
    fn tab_cycles_completions() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/s");
        let count = bar.active_suggestions().len();
        assert!(count > 1);

        bar.complete();
        assert_eq!(bar.completion_idx, Some(0));
        bar.complete();
        assert_eq!(bar.completion_idx, Some(1));
        bar.complete();
        assert_eq!(bar.completion_idx, Some(2 % count));
    }

    #[test]
    fn tab_single_match_inserts() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/hel");
        bar.complete();
        assert!(bar.input.value().starts_with("/help"));
        assert!(bar.completion_idx.is_none());
    }

    #[test]
    fn no_suggestions_without_slash() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "hello");
        assert!(bar.active_suggestions().is_empty());
    }

    #[test]
    fn dismiss_clears_completion_idx() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/s");
        bar.complete();
        assert!(bar.completion_idx.is_some());
        bar.dismiss_completions();
        assert!(bar.completion_idx.is_none());
    }

    #[test]
    fn palette_opens_on_slash() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/");
        let ctx = context();
        assert!(bar.palette_open(&ctx));
        assert!(!bar.palette_matches(&ctx).is_empty());
    }

    #[test]
    fn palette_filters_as_typed() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/mod");
        let ctx = context();
        let m = bar.palette_matches(&ctx);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].value, "/model");
    }

    #[test]
    fn palette_includes_init_command() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/ini");
        let matches = bar.palette_matches(&context());
        assert_eq!(matches[0].value, "/init");
        assert_eq!(matches[0].description, "Generate or update AGENTS.md");
    }

    #[test]
    fn palette_navigation() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/");
        let ctx = context();
        assert_eq!(bar.palette_idx, 0);
        bar.palette_down(&ctx);
        assert_eq!(bar.palette_idx, 1);
        bar.palette_up();
        assert_eq!(bar.palette_idx, 0);
    }

    #[test]
    fn palette_select_inserts_command() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/hel");
        let selected = bar.palette_select(&context());
        assert_eq!(selected, Some("/help".to_string()));
        assert_eq!(bar.input.value(), "/help");
    }

    #[test]
    fn palette_select_complete_command_can_dispatch_immediately() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/status");
        let selected = bar.palette_select(&context());
        assert_eq!(selected, Some("/status".to_string()));
        assert!(!bar.input.value().ends_with(' '));
    }

    #[test]
    fn palette_select_arg_command_waits_for_prompt() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/ru");
        let selected = bar.palette_select(&context());
        assert_eq!(selected, Some("/run ".to_string()));
        assert!(bar.input.value().ends_with(' '));
    }

    #[test]
    fn workflow_palette_opens_after_start_space() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/start ");
        let ctx = context();
        assert!(bar.palette_open(&ctx));

        let matches = bar.palette_matches(&ctx);
        assert_eq!(matches[0].value, "dev");
        assert!(matches.iter().any(|item| item.value == "code-review"));
    }

    #[test]
    fn workflow_palette_opens_after_run_space() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/run ");
        let ctx = context();
        assert!(bar.palette_open(&ctx));

        let names = values(bar.palette_matches(&ctx));
        assert_eq!(
            names,
            vec!["dev", "marketing", "prospecting", "code-review"]
        );
    }

    #[test]
    fn workflow_palette_filters_by_prefix() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/start ma");

        let matches = bar.palette_matches(&context());
        assert_eq!(matches[0].value, "marketing");
        assert_eq!(matches[0].description, "Marketing and content workflow");
    }

    #[test]
    fn workflow_palette_select_inserts_workflow_and_keeps_prompt_open() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/run code");
        let selected = bar.palette_select(&context());

        assert_eq!(selected, Some("/run code-review ".to_string()));
        assert_eq!(bar.input.value(), "/run code-review ");
    }

    #[test]
    fn workflow_palette_stays_closed_for_default_workflow_prompt() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/start \"build a chat app\"");
        let ctx = context();

        assert!(!bar.palette_open(&ctx));
        assert!(bar.palette_matches(&ctx).is_empty());
    }

    #[test]
    fn workflow_palette_stays_closed_after_workflow_selection() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/start dev ");
        let ctx = context();

        assert!(!bar.palette_open(&ctx));
        assert!(bar.palette_matches(&ctx).is_empty());
    }

    #[test]
    fn websearch_palette_suggests_actions() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/websearch e");
        assert_eq!(values(bar.palette_matches(&context())), vec!["enable"]);
    }

    #[test]
    fn skill_palette_suggests_installed_skill_names() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/skill enable r");
        assert_eq!(values(bar.palette_matches(&context())), vec!["rust"]);
    }

    #[test]
    fn at_mentions_suggest_project_paths_inline() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "explique @main");
        assert!(bar.palette_open(&context()));
        assert_eq!(
            values(bar.palette_matches(&context())),
            vec!["@src/main.rs"]
        );
    }

    #[test]
    fn at_mentions_insert_selected_path_inline() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "lis @read");
        let selected = bar.palette_select(&context());

        assert_eq!(selected, Some("lis @README.md ".to_string()));
        assert_eq!(bar.input.value(), "lis @README.md ");
    }

    #[test]
    fn dollar_mentions_suggest_skills_inline() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "utilise $ru");

        assert!(bar.palette_open(&context()));
        assert_eq!(values(bar.palette_matches(&context())), vec!["$rust"]);
    }

    #[test]
    fn provider_palette_suggests_providers() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/provider open");
        assert_eq!(values(bar.palette_matches(&context())), vec!["openrouter"]);
    }

    #[test]
    fn connect_palette_suggests_auth_methods_after_provider() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/connect ollama ");
        assert_eq!(values(bar.palette_matches(&context())), vec!["local"]);
    }

    #[test]
    fn model_palette_suggests_roles_then_cached_models() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/model devel");
        assert_eq!(values(bar.palette_matches(&context())), vec!["developer"]);

        let mut bar = InputBar::new();
        type_into(&mut bar, "/model developer llama");
        assert_eq!(
            values(bar.palette_matches(&context())),
            vec!["ollama/llama3.1:8b"]
        );
    }

    #[test]
    fn agent_palettes_suggest_active_agents() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/focus dev");
        assert_eq!(
            values(bar.palette_matches(&context())),
            vec!["developer:src/main.rs"]
        );

        let mut bar = InputBar::new();
        type_into(&mut bar, "/agent ce");
        assert_eq!(values(bar.palette_matches(&context())), vec!["ceo"]);
    }

    #[test]
    fn resume_palette_inserts_quoted_paths_when_needed() {
        let mut bar = InputBar::new();
        type_into(&mut bar, "/resume marketing");
        let selected = bar.palette_select(&context());

        assert_eq!(
            selected,
            Some("/resume \"/tmp/cortex project\"".to_string())
        );
        assert_eq!(bar.input.value(), "/resume \"/tmp/cortex project\"");
    }
}
