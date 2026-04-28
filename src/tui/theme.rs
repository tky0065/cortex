use ratatui::style::{Color, Modifier, Style};

/// Central theme for the Cortex TUI, providing a "Futuristic Neon" aesthetic.
pub struct Theme {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,
    pub bg: Color,
    pub text: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            primary: Color::Cyan, // Electric Cyan
            secondary: Color::Magenta, // Neon Purple
            accent: Color::Blue,
            success: Color::Green, // Emerald
            warning: Color::Yellow, // Amber
            error: Color::Red, // Crimson
            muted: Color::DarkGray, // Slate
            bg: Color::Reset,
            text: Color::White,
        }
    }
}

pub const THEME: Theme = Theme {
    primary: Color::Rgb(34, 211, 238),    // Cyan 400
    secondary: Color::Rgb(168, 85, 247),  // Purple 500
    accent: Color::Rgb(56, 189, 248),     // Sky 400
    success: Color::Rgb(34, 197, 94),     // Green 500
    warning: Color::Rgb(234, 179, 8),     // Yellow 500
    error: Color::Rgb(239, 68, 68),       // Red 500
    muted: Color::Rgb(71, 85, 105),       // Slate 500
    bg: Color::Rgb(15, 23, 42),           // Slate 900
    text: Color::Rgb(248, 250, 252),      // Slate 50
};

impl Theme {
    pub fn border_style(&self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn active_border_style(&self) -> Style {
        Style::default().fg(self.primary)
    }

    pub fn title_style(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .add_modifier(Modifier::BOLD)
    }

    pub fn log_timestamp(&self) -> Style {
        Style::default().fg(self.muted)
    }

    pub fn agent_tag(&self, color: Color) -> Style {
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }
}
