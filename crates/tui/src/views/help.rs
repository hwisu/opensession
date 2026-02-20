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
    let close_hint_line = Line::from(Span::styled(
        "Press any key to close",
        Style::new().fg(Color::DarkGray),
    ));

    let mut lines = vec![
        Line::from(Span::styled("── Global ──", header_style)),
        Line::from(vec![
            Span::styled("  1/2/3     ", key_style),
            Span::styled("Switch tabs (Sessions/Handoff/Settings)", desc_style),
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
            Span::styled("Cycle view mode (Local/Repo)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  p         ", key_style),
            Span::styled("Publish session", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  m         ", key_style),
            Span::styled("Multi-column by active agent count", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  t         ", key_style),
            Span::styled("Cycle tool filter (Local/Repo)", desc_style),
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
            Span::styled("Cycle tool filter (DB view)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  d         ", key_style),
            Span::styled("Delete session (Repo view)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  PgDn/PgUp ", key_style),
            Span::styled("Previous/next page", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  LIVE badge ", key_style),
            Span::styled("Recent source update + recent event activity", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled("── Session Detail ──", header_style)),
        Line::from(vec![
            Span::styled("  j/k       ", key_style),
            Span::styled("Navigate events", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g/G       ", key_style),
            Span::styled("Jump to first/latest event", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  h/l, ←/→  ", key_style),
            Span::styled("Horizontal scroll", desc_style),
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
            Span::styled("  d         ", key_style),
            Span::styled("Toggle file diff preview for selected event", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  0-9       ", key_style),
            Span::styled("Filters: 1=All 2=User 3=Agent 4=Think 5=Tools", desc_style),
        ]),
        Line::from(vec![
            Span::styled("            ", key_style),
            Span::styled("         6=Files 7=Shell 8=Task 9=Web 0=Other", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  follow    ", key_style),
            Span::styled("ON keeps tail attached; scroll-up turns it OFF", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Esc/q     ", key_style),
            Span::styled("Back to session list", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled("── Handoff ──", header_style)),
        Line::from(vec![
            Span::styled("  j/k       ", key_style),
            Span::styled("Move selected session in picker", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Space     ", key_style),
            Span::styled("Multi-select sessions for merged handoff", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Enter     ", key_style),
            Span::styled("Refresh handoff preview (stays in Handoff tab)", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g         ", key_style),
            Span::styled("Generate HANDOFF.md from selected sessions", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  s         ", key_style),
            Span::styled("Save selected sessions as handoff artifact", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  r         ", key_style),
            Span::styled("Refresh last saved artifact if stale", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  Esc/q     ", key_style),
            Span::styled("Back to sessions", desc_style),
        ]),
        Line::raw(""),
        Line::from(Span::styled("── Settings ──", header_style)),
        Line::from(vec![
            Span::styled("  [/]       ", key_style),
            Span::styled(
                "Switch section (Capture Flow/Privacy/Git/Web Sync (Public))",
                desc_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("  s         ", key_style),
            Span::styled("Save config", desc_style),
        ]),
        Line::from(vec![
            Span::styled("  g         ", key_style),
            Span::styled("Regenerate API key (Web Sync (Public))", desc_style),
        ]),
        Line::raw(""),
        close_hint_line.clone(),
    ];

    // Keep close hint visible even when the help body exceeds the popup height.
    let max_lines = inner.height as usize;
    if max_lines == 0 {
        return;
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            *last = close_hint_line;
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

#[cfg(test)]
mod tests {
    use super::render;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::Terminal;

    fn buffer_to_string(buffer: &Buffer) -> String {
        let area = *buffer.area();
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn render_shows_shortcuts_and_close_hint() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                let area = frame.area();
                render(frame, area);
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Keyboard Shortcuts"));
        assert!(text.contains("Session List"));
        assert!(text.contains("Session Detail"));
        assert!(text.contains("Press any key to close"));
    }

    #[test]
    fn render_handles_small_terminal_area() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render(frame, Rect::new(0, 0, 30, 10));
            })
            .expect("draw");

        let text = buffer_to_string(terminal.backend().buffer());
        assert!(text.contains("Keyboard"));
    }
}
