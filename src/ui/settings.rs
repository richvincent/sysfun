//! Settings view: tab order and visibility, plus a handful of preferences. Every change is
//! written to the config file immediately.

use crate::app::{App, SettingsItem, View};
use crate::ui::logo;
use crate::ui::widgets::*;
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    // Tall terminals get the mark beside the intro text; short ones keep the compact header.
    let (lw, lh) = logo::cells(&logo::LOGO16);
    let with_logo = area.height >= 34 && area.width >= 100;
    let intro_h = if with_logo { lh + 2 } else { 5 };
    let rows = Layout::vertical([Constraint::Length(intro_h), Constraint::Min(8), Constraint::Length(4)]).split(area);
    app.hit.settings_rows.clear();

    // ── intro ────────────────────────────────────────────────────────
    {
        let b = section("settings", th);
        let mut inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        if with_logo {
            logo::draw(frame, Rect::new(inner.x + 1, inner.y, lw, lh), &logo::LOGO16, app.mono());
            inner = Rect::new(inner.x + lw + 3, inner.y + 1, inner.width.saturating_sub(lw + 4), inner.height.saturating_sub(1));
        }
        let lines = vec![
            Line::from(vec![
                fg("Arrange the tab bar and set a few preferences. ", Color::White),
                dim("Changes save instantly; nothing to confirm."),
            ]),
            Line::from(vec![
                bold("↑↓", th.accent),
                dim(" select   "),
                bold("space / enter", th.accent),
                dim(" show·hide tab, or change a preference   "),
                bold("←→", th.accent),
                dim(" move tab left·right, or change value   "),
                bold("d", th.accent),
                dim(" restore defaults"),
            ]),
            Line::from(vec![
                dim("Also: "),
                bold("K / J", th.accent),
                dim(" or "),
                bold("Shift-↑↓", th.accent),
                dim(" move a tab; click a row to select it, click again to act on it."),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    let items = SettingsItem::all(&app.cfg);
    app.settings_sel = app.settings_sel.min(items.len() - 1);
    let cols = Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)]).split(rows[1]);
    let sel_style = Style::new().bg(Color::Rgb(45, 50, 60));

    // ── tabs ─────────────────────────────────────────────────────────
    {
        let visible = app.cfg.tabs.iter().filter(|(_, on)| *on).count();
        let b = section_with_right("tab bar", &format!("{} of {} shown · order is left → right", visible, app.cfg.tabs.len()), th);
        let inner = b.inner(cols[0]);
        frame.render_widget(b, cols[0]);
        let mut shown = 0;
        for (i, (view, on)) in app.cfg.tabs.iter().enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let r = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            app.hit.settings_rows.push(r);
            let selected = items.get(app.settings_sel) == Some(&SettingsItem::Tab(i));
            let number = if *on {
                shown += 1;
                format!("{}", shown)
            } else {
                "·".into()
            };
            let mut spans = vec![
                if selected { bold("▶ ", th.accent) } else { Span::raw("  ") },
                if *on { fg("✓ ", th.good) } else { dim("✗ ") },
                fg(format!("{:>2}  ", number), if *on { th.accent } else { th.muted }),
                if *on { fg(view.title(), Color::White) } else { dim(view.title()) },
            ];
            if !*on {
                spans.push(dim("   hidden"));
            }
            if *view == app.cfg.default_view {
                spans.push(dim("   (default)"));
            }
            let mut p = Paragraph::new(Line::from(spans));
            if selected {
                p = p.style(sel_style);
            }
            frame.render_widget(p, r);
        }
    }

    // ── preferences ──────────────────────────────────────────────────
    {
        let b = section("preferences", th);
        let inner = b.inner(cols[1]);
        frame.render_widget(b, cols[1]);
        let c = &app.cfg;
        let prefs: Vec<(&str, String, String)> = vec![
            ("Default view", c.default_view.title().to_string(), "the tab shown at launch when no flag is given".into()),
            ("Refresh every", format!("{} ms", c.interval_ms), "how often sensors and counters are sampled".into()),
            ("Temperature", if c.fahrenheit { "°F".into() } else { "°C".into() }, "unit for every temperature on screen".into()),
            ("Theme", c.theme.label().to_string(), "a colour per view, or grey accents with status colours only".into()),
            (
                "Mouse",
                if c.mouse { "on".into() } else { "off".into() },
                "click tabs and rows, wheel to scroll; off leaves text selection to the terminal".into(),
            ),
            (
                "Hidden Wi-Fi networks",
                if c.show_hidden_networks { "show".into() } else { "hide".into() },
                "list access points that do not broadcast a name".into(),
            ),
        ];
        for (k, (label, value, help)) in prefs.iter().enumerate() {
            let y = inner.y + (k as u16) * 2;
            if y + 1 >= inner.y + inner.height {
                break;
            }
            let r = Rect::new(inner.x, y, inner.width, 1);
            app.hit.settings_rows.push(Rect::new(inner.x, y, inner.width, 2));
            let selected = items.get(app.settings_sel) == Some(&SettingsItem::PREFS[k]);
            let line = Line::from(vec![
                if selected { bold("▶ ", th.accent) } else { Span::raw("  ") },
                fg(format!("{:<22}", label), Color::White),
                dim("‹ "),
                bold(value.clone(), th.accent),
                dim(" ›"),
            ]);
            let mut p = Paragraph::new(line);
            if selected {
                p = p.style(sel_style);
            }
            frame.render_widget(p, r);
            frame.render_widget(Paragraph::new(Line::from(dim(format!("    {}", help)))), Rect::new(inner.x, y + 1, inner.width, 1));
        }
    }

    // ── status ───────────────────────────────────────────────────────
    {
        let b = section("file", th);
        let inner = b.inner(rows[2]);
        frame.render_widget(b, rows[2]);
        let lines = vec![
            Line::from(vec![
                fg("config  ", th.label),
                fg(app.cfg_path.display().to_string(), Color::White),
                dim("   (plain TOML, safe to edit by hand; a bad line falls back to defaults)"),
            ]),
            Line::from(vec![
                fg("status  ", th.label),
                match &app.cfg_msg {
                    Some(m) if m.starts_with("saved") => fg(m.clone(), th.good),
                    Some(m) => fg(m.clone(), th.warn),
                    None => dim(if app.cfg == crate::config::Config::default() {
                        "defaults in use, nothing saved yet".to_string()
                    } else {
                        "loaded".to_string()
                    }),
                },
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }
    let _ = View::Settings;
}
