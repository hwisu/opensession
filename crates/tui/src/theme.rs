use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Padding};

pub struct Theme;

impl Theme {
    // ── Background ───────────────────────────────────────────────────
    pub const BG_SURFACE: Color = Color::Rgb(30, 35, 50);

    // ── Border ───────────────────────────────────────────────────────
    pub const BORDER_DIM: Color = Color::DarkGray;
    pub const BORDER_NORMAL: Color = Color::Rgb(60, 65, 80);
    pub const BORDER_ACCENT: Color = Color::Rgb(100, 180, 240);

    // ── Text hierarchy ───────────────────────────────────────────────
    pub const TEXT_PRIMARY: Color = Color::White;
    pub const TEXT_SECONDARY: Color = Color::Rgb(140, 145, 160);
    pub const TEXT_MUTED: Color = Color::Rgb(80, 85, 100);
    pub const TEXT_DISABLED: Color = Color::Rgb(60, 65, 80);
    pub const TEXT_HINT: Color = Color::Rgb(60, 65, 80);
    pub const TEXT_CONTENT: Color = Color::Rgb(170, 175, 190);
    pub const TEXT_DIMMER: Color = Color::Rgb(50, 55, 70);

    // ── Key style (for footer hints) ─────────────────────────────────
    pub const TEXT_KEY: Color = Color::Rgb(140, 145, 160);
    pub const TEXT_KEY_DESC: Color = Color::DarkGray;

    // ── Accent ───────────────────────────────────────────────────────
    pub const ACCENT_BLUE: Color = Color::Rgb(100, 180, 240);
    pub const ACCENT_GREEN: Color = Color::Rgb(80, 200, 120);
    pub const ACCENT_RED: Color = Color::Rgb(220, 80, 80);
    pub const ACCENT_YELLOW: Color = Color::Rgb(220, 180, 60);
    pub const ACCENT_PURPLE: Color = Color::Rgb(180, 140, 220);
    pub const ACCENT_ORANGE: Color = Color::Rgb(217, 119, 80);
    pub const ACCENT_CYAN: Color = Color::Rgb(80, 200, 200);
    pub const ACCENT_TEAL: Color = Color::Rgb(80, 180, 160);

    // ── Semantic ─────────────────────────────────────────────────────
    pub const TOGGLE_ON: Color = Color::Rgb(80, 200, 120);
    pub const TOGGLE_OFF: Color = Color::Rgb(220, 80, 80);

    // ── Detail view colors ───────────────────────────────────────────
    pub const GUTTER: Color = Color::Rgb(55, 60, 75);
    pub const TREE: Color = Color::Rgb(70, 75, 90);
    pub const BAR_DIM: Color = Color::Rgb(70, 75, 90);

    // ── Role colors ──────────────────────────────────────────────────
    pub const ROLE_USER: Color = Color::Rgb(80, 180, 100);
    pub const ROLE_AGENT: Color = Color::Rgb(100, 140, 220);
    pub const ROLE_AGENT_BRIGHT: Color = Color::Rgb(100, 160, 240);
    pub const ROLE_SYSTEM: Color = Color::Gray;
    pub const ROLE_TASK: Color = Color::Rgb(180, 140, 80);

    // ── Misc detail colors ───────────────────────────────────────────
    pub const TAG_COLOR: Color = Color::Rgb(100, 120, 160);

    // ── Badge backgrounds ────────────────────────────────────────────
    pub const BADGE_LOCAL: Color = Color::Rgb(100, 105, 120);
    pub const BADGE_SERVER: Color = Color::Rgb(80, 160, 240);
    pub const BADGE_PERSONAL: Color = Color::Rgb(80, 200, 120);

    // ── Tab style ────────────────────────────────────────────────────
    pub const TAB_INACTIVE: Color = Color::Rgb(120, 125, 140);
    pub const TAB_DIM: Color = Color::Rgb(70, 75, 90);

    // ── Settings ─────────────────────────────────────────────────────
    pub const FIELD_VALUE: Color = Color::Rgb(100, 105, 120);

    // ── Padding ──────────────────────────────────────────────────────
    pub const PADDING_CARD: Padding = Padding::new(2, 2, 1, 1);
    pub const PADDING_COMPACT: Padding = Padding::new(1, 1, 0, 0);

    // ── Block helpers ────────────────────────────────────────────────

    pub fn block() -> Block<'static> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Self::BORDER_NORMAL))
    }

    pub fn block_dim() -> Block<'static> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Self::BORDER_DIM))
    }

    pub fn block_accent() -> Block<'static> {
        Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Self::BORDER_ACCENT))
    }
}

// ── User color palette ────────────────────────────────────────────────

const USER_PALETTE: [Color; 8] = [
    Color::Rgb(100, 180, 240), // blue
    Color::Rgb(80, 200, 120),  // green
    Color::Rgb(220, 180, 60),  // yellow
    Color::Rgb(180, 140, 220), // purple
    Color::Rgb(220, 130, 80),  // orange
    Color::Rgb(80, 200, 200),  // teal
    Color::Rgb(220, 100, 160), // pink
    Color::Rgb(160, 200, 80),  // lime
];

pub fn user_color(nickname: &str) -> Color {
    let hash = nickname
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    USER_PALETTE[(hash as usize) % USER_PALETTE.len()]
}

// ── Tool icon / color ────────────────────────────────────────────────

pub fn tool_icon(tool: &str) -> &'static str {
    match tool {
        "claude-code" => " CC ",
        "codex" => " Cx ",
        "opencode" => " Oc ",
        "cline" => " Cl ",
        "amp" => " Ap ",
        "cursor" => " Cr ",
        _ => " ?? ",
    }
}

pub fn tool_color(tool: &str) -> Color {
    match tool {
        "claude-code" => Color::Rgb(217, 119, 80),
        "codex" => Color::Rgb(16, 185, 129),
        "opencode" => Color::Rgb(245, 158, 11),
        "cline" => Color::Rgb(239, 68, 68),
        "amp" => Color::Rgb(168, 85, 247),
        "cursor" => Color::Rgb(80, 180, 220),
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_color_is_stable_for_same_nickname() {
        assert_eq!(user_color("alice"), user_color("alice"));
    }

    #[test]
    fn user_color_uses_multiple_palette_slots() {
        let a = user_color("alice");
        let b = user_color("bob");
        let c = user_color("carol");
        assert!(a != b || b != c || a != c);
    }

    #[test]
    fn tool_icon_maps_known_and_unknown_tools() {
        assert_eq!(tool_icon("claude-code"), " CC ");
        assert_eq!(tool_icon("codex"), " Cx ");
        assert_eq!(tool_icon("unknown-tool"), " ?? ");
    }

    #[test]
    fn tool_color_maps_known_and_unknown_tools() {
        assert_eq!(tool_color("claude-code"), Color::Rgb(217, 119, 80));
        assert_eq!(tool_color("codex"), Color::Rgb(16, 185, 129));
        assert_eq!(tool_color("unknown-tool"), Color::White);
    }
}
