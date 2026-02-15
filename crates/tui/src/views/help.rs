use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph};

pub fn render(frame: &mut Frame, area: Rect) {
    // Center the help overlay
    let popup_width = 60u16.min(area.width.saturating_sub(4));
    let popup_height = 30u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    frame.render_widget(Clear, popup_area);

    let block = Theme::block_accent()
        .title(" Keyboard Shortcuts ")
        .padding(Theme::PADDING_CARD);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let key_style = Style::new().fg(Theme::ACCENT_YELLOW).bold();
    let desc_style = Style::new().fg(Theme::TEXT_CONTENT);
    let header_style = Style::new().fg(Theme::ACCENT_BLUE).bold();

    let lines = vec![
        Line::from(Span::styled("── Global ──", header_style)),
        Line::from(vec![
            Span::styled("  1/2/3/4   ", key_style),
            Span::styled(
                "Switch tabs (Sessions/Collaboration/Operations/Settings)",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("  ?         ", key_style),
            Span::styled("Toggle this help", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  q         ", key_style),
            Span::styled("Quit", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled("── Session List ──", header_style)),
        Line::from(vec![
            Span::styled("  j/k       ", key_style),
            Span::styled("Navigate up/down", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g/G       ", key_style),
            Span::styled("Jump to first/last", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Enter     ", key_style),
            Span::styled("Open session detail", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  /         ", key_style),
            Span::styled("Search sessions", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Tab       ", key_style),
            Span::styled("Cycle view mode (Local/Team/Repo)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  p         ", key_style),
            Span::styled("Publish session (multi-target)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  m         ", key_style),
            Span::styled("Multi-column by active agent count", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  t         ", key_style),
            Span::styled("Cycle tool filter (Local/Team/Repo)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  r         ", key_style),
            Span::styled("Cycle time range (All/24h/7d/30d)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  R         ", key_style),
            Span::styled("Repo picker (search + open)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  f         ", key_style),
            Span::styled("Cycle tool filter (DB view, legacy)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  d         ", key_style),
            Span::styled("Delete session (Team/Repo)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgDn/PgUp ", key_style),
            Span::styled("Previous/next page", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled("── Session Detail ──", header_style)),
        Line::from(vec![
            Span::styled("  j/k       ", key_style),
            Span::styled("Navigate events", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  h/l, ←/→  ", key_style),
            Span::styled("Horizontal scroll/drag (Turn + Linear)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgDn/PgUp ", key_style),
            Span::styled("Jump 10 events", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  u/U       ", key_style),
            Span::styled("Next/prev user message", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  n/N       ", key_style),
            Span::styled("Next/prev same-type event", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Enter/Spc ", key_style),
            Span::styled("Linear: expand event · Turn: raw thread toggle", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  v         ", key_style),
            Span::styled("Toggle turn view (side-by-side)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  1-6       ", key_style),
            Span::styled("Filter: All/Msgs/Tools/Think/Files/Shell", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  c         ", key_style),
            Span::styled("Linear only: toggle consecutive collapse", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  LLM       ", key_style),
            Span::styled(
                "Timeline summary status (Settings > Timeline Intelligence)",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("  RT        ", key_style),
            Span::styled("Auto-refresh status", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Esc/q     ", key_style),
            Span::styled("Back to session list", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "── Collaboration / Operations ──",
            header_style,
        )),
        Line::from(vec![
            Span::styled("  i         ", key_style),
            Span::styled("Open inbox from collaboration view", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  d/s/r     ", key_style),
            Span::styled("Operations actions: daemon/save/refresh", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled("── Settings ──", header_style)),
        Line::from(vec![
            Span::styled("  [/]       ", key_style),
            Span::styled(
                "Switch section (Workspace/Capture/Timeline/Storage/Account)",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("  s         ", key_style),
            Span::styled("Save config", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g         ", key_style),
            Span::styled("Regenerate API key (Account)", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "Press any key to close",
            Style::new().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
