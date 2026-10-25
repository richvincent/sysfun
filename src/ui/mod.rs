//! Rendering. Each view is a function of (`Frame`, area, `App`).

pub mod cpu;
pub mod disk;
pub mod logo;
pub mod logo_data;
pub mod mem;
pub mod net;
pub mod overview;
pub mod power;
pub mod procs;
pub mod settings;
pub mod thermal;
pub mod widgets;
pub mod wifi;

use crate::app::{App, View};
use crate::util::clock;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use widgets::{section, Theme};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let th = Theme::for_view(app.view, app.mono());
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).split(area);

    // ── header: brand + tabs + gear ──────────────────────────────────
    let gear_w: u16 = 3;
    let hdr = Layout::horizontal([Constraint::Length(11), Constraint::Min(0), Constraint::Length(gear_w)]).split(rows[0]);
    // The ◡ echoes the smile-gauge mark; it is a plain BMP glyph so every terminal font has it.
    let brand = Span::styled(" ◡ sysfun ", Style::new().fg(Color::Black).bg(th.accent).bold());
    frame.render_widget(Paragraph::new(Line::from(brand)), hdr[0]);
    app.hit.header = Some(rows[0]);
    app.hit.tabs.clear();
    let mut spans: Vec<Span> = Vec::new();
    let mut x = hdr[1].x;
    let tabs = app.tabs.clone();
    for (i, v) in tabs.iter().enumerate() {
        let key = format!("{} ", i + 1);
        let w = (key.len() + v.title().len()) as u16;
        if i > 0 {
            spans.push(Span::styled(" · ", Style::new().fg(Color::DarkGray)));
        }
        if *v == app.view {
            spans.push(Span::styled(key, Style::new().fg(th.accent).bold()));
            spans.push(Span::styled(v.title(), Style::new().fg(th.accent).bold().underlined()));
        } else {
            spans.push(Span::styled(key, Style::new().fg(Color::DarkGray)));
            spans.push(Span::styled(v.title(), Style::new().fg(Color::Gray)));
        }
        if x + w <= hdr[1].x + hdr[1].width {
            app.hit.tabs.push((Rect::new(x, hdr[1].y, w, 1), *v));
        }
        x += w + 3;
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), hdr[1]);
    let gear_style = if app.view == View::Settings { Style::new().fg(th.accent).bold() } else { Style::new().fg(Color::DarkGray) };
    frame.render_widget(Paragraph::new(Line::from(Span::styled(" ⚙ ", gear_style))), hdr[2]);
    app.hit.gear = Some(hdr[2]);

    // ── body ─────────────────────────────────────────────────────────
    let body = rows[1];
    if app.view == View::Settings {
        settings::draw(frame, body, app, &th);
    } else if app.snap.is_none() {
        let p = Paragraph::new(Line::from(Span::styled("  gathering first sample…", Style::new().fg(Color::DarkGray).italic())));
        frame.render_widget(p, body);
    } else {
        match app.view {
            View::Overview => overview::draw(frame, body, app, &th),
            View::Cpu => cpu::draw(frame, body, app, &th),
            View::Mem => mem::draw(frame, body, app, &th),
            View::Disk => disk::draw(frame, body, app, &th),
            View::Net => net::draw(frame, body, app, &th),
            View::Wifi => wifi::draw(frame, body, app, &th),
            View::Power => power::draw(frame, body, app, &th),
            View::Thermal => thermal::draw(frame, body, app, &th),
            View::Procs => procs::draw(frame, body, app, &th),
            View::Settings => {}
        }
    }

    // ── footer ───────────────────────────────────────────────────────
    let dim = Style::new().fg(Color::DarkGray);
    let key = |k: &str| Span::styled(k.to_string(), Style::new().fg(th.accent));
    let mut left = vec![
        Span::raw(" "),
        key("q"),
        Span::styled(" quit  ", dim),
        key("←→/Tab"),
        Span::styled(" view  ", dim),
        key("1-9"),
        Span::styled(" jump  ", dim),
        key(","),
        Span::styled(" settings  ", dim),
        key("?"),
        Span::styled(" help", dim),
    ];
    match app.view {
        View::Procs => {
            left.extend([Span::styled("  ", dim), key("c/m/p/n"), Span::styled(" sort  ", dim), key("↑↓"), Span::styled(" select", dim)])
        }
        View::Wifi => {
            left.extend([Span::styled("  ", dim), key("↑↓"), Span::styled(" networks  ", dim), key("s"), Span::styled(" rescan", dim)]);
            if let Some(t) = app.rescan_at {
                if t.elapsed().as_secs() < 8 {
                    left.extend([Span::styled("  ", dim), Span::styled("scanning…", Style::new().fg(Color::Yellow))]);
                }
            }
        }
        View::Settings => left.extend([
            Span::styled("  ", dim),
            key("space"),
            Span::styled(" toggle  ", dim),
            key("←→"),
            Span::styled(" change/move  ", dim),
            key("d"),
            Span::styled(" defaults", dim),
        ]),
        _ => {}
    }
    if app.paused {
        left.extend([Span::styled("  ", dim), Span::styled("PAUSED", Style::new().fg(Color::Yellow).bold())]);
    }
    let sample_ms = app.snap.as_ref().map(|s| s.sample_ms).unwrap_or(0.0);
    let slowest = app
        .snap
        .as_ref()
        .and_then(|s| s.timings.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)))
        .map(|(n, ms)| format!(" ({} {:.0})", n, ms))
        .unwrap_or_default();
    let right = Line::from(vec![
        Span::styled(format!("{} ", app.host.hostname), Style::new().fg(Color::Gray)),
        Span::styled(
            format!("· {}ms tick · {:.1}ms sample{} · {:.1}ms draw · ", app.interval_ms, sample_ms, slowest, app.last_draw_ms),
            dim,
        ),
        Span::styled(clock(), Style::new().fg(th.accent)),
        Span::raw(" "),
    ])
    .right_aligned();
    frame.render_widget(Paragraph::new(Line::from(left)), rows[2]);
    frame.render_widget(Paragraph::new(right), rows[2]);

    if app.show_help {
        help(frame, area, &th, app.mono());
    }
}

fn help(frame: &mut Frame, area: Rect, th: &Theme, mono: bool) {
    // With enough rows the overlay opens with an About block: the mark beside name, version and repo.
    let (lw, lh) = logo::cells(&logo::LOGO24);
    let about_h = if area.height >= 40 { lh + 1 } else { 0 };
    let w = 70.min(area.width.saturating_sub(2));
    let h = (25 + about_h).min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let r = Rect::new(x, y, w, h);
    frame.render_widget(Clear, r);
    let b: Block = section("help", th);
    let inner = b.inner(r);
    frame.render_widget(b, r);
    let mut body = inner;
    if about_h > 0 && inner.width > lw + 30 {
        let at = Rect::new(inner.x + 1, inner.y, lw, lh);
        logo::draw(frame, at, &logo::LOGO24, mono);
        let tx = Rect::new(at.x + lw + 3, inner.y + 2, inner.width.saturating_sub(lw + 4), lh);
        let about = vec![
            Line::from(vec![
                Span::styled("sysfun", Style::new().fg(th.accent).bold()),
                Span::styled(format!("  v{}", env!("CARGO_PKG_VERSION")), Style::new().fg(Color::Gray)),
            ]),
            Line::from(Span::styled("see your machine", Style::new().fg(Color::Gray).italic())),
            Line::raw(""),
            Line::from(Span::raw("Plain English on top,")),
            Line::from(Span::raw("kernel counters below.")),
            Line::raw(""),
            Line::from(Span::styled("github.com/richvincent/sysfun", Style::new().fg(Color::DarkGray))),
            Line::from(Span::styled("MIT licence", Style::new().fg(Color::DarkGray))),
        ];
        frame.render_widget(Paragraph::new(about), tx);
        body = Rect::new(inner.x, inner.y + about_h, inner.width, inner.height.saturating_sub(about_h));
    }
    let k = |a: &str, b: &str| {
        Line::from(vec![Span::styled(format!("  {:<14}", a), Style::new().fg(th.accent).bold()), Span::raw(b.to_string())])
    };
    let text = vec![
        Line::raw(""),
        k("q / Esc", "quit"),
        k("Tab / → / l", "next view"),
        k("S-Tab / ← / h", "previous view"),
        k("1 … 9", "jump to the n-th tab"),
        k(",", "settings: reorder or hide tabs, defaults, units, theme"),
        k("↑ ↓ / j k", "move selection (processes, Wi-Fi networks, settings)"),
        k("c m p n", "sort processes by cpu / memory / pid / name"),
        k("s", "rescan Wi-Fi networks (via NetworkManager)"),
        k("space", "pause / resume sampling (in settings: toggle)"),
        k("r", "refresh now"),
        k("+ / -", "faster / slower refresh"),
        k("?", "toggle this help"),
        k("mouse", "click tabs, rows, column headers and ⚙; wheel scrolls"),
        Line::raw(""),
        Line::from(Span::styled("  Flags: --bat --wifi --disk --cpu --mem --net --temp --ps", Style::new().fg(Color::Gray))),
        Line::from(Span::styled("         --once prints a single frame and exits", Style::new().fg(Color::Gray))),
        Line::from(Span::styled("         -i <ms> sets the refresh interval", Style::new().fg(Color::Gray))),
        Line::from(Span::styled("         --config <path> uses another settings file", Style::new().fg(Color::Gray))),
    ];
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), body);
}
