//! Rendering. Each view is a function of (`Frame`, area, `App`).

pub mod cpu;
pub mod disk;
pub mod mem;
pub mod net;
pub mod overview;
pub mod power;
pub mod procs;
pub mod thermal;
pub mod widgets;
pub mod wifi;

use crate::app::{App, View};
use crate::util::clock;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Clear, Paragraph, Tabs, Wrap};
use widgets::{section, Theme};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let th = Theme::for_view(app.view);
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)]).split(area);

    // ── header: brand + tabs ─────────────────────────────────────────
    let titles: Vec<Line> = View::ALL
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let key = format!("{}", i + 1);
            if *v == app.view {
                Line::from(vec![
                    Span::styled(key, Style::new().fg(th.accent).bold()),
                    Span::raw(" "),
                    Span::styled(v.title(), Style::new().fg(th.accent).bold().underlined()),
                ])
            } else {
                Line::from(vec![
                    Span::styled(key, Style::new().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(v.title(), Style::new().fg(Color::Gray)),
                ])
            }
        })
        .collect();
    let brand = Span::styled(" sysfun ", Style::new().fg(Color::Black).bg(th.accent).bold());
    let hdr = Layout::horizontal([Constraint::Length(9), Constraint::Min(0)]).split(rows[0]);
    frame.render_widget(Paragraph::new(Line::from(brand)), hdr[0]);
    frame.render_widget(Tabs::new(titles).divider(Span::styled(" · ", Style::new().fg(Color::DarkGray))).padding("", ""), hdr[1]);

    // ── body ─────────────────────────────────────────────────────────
    let body = rows[1];
    if app.snap.is_none() {
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
        key("?"),
        Span::styled(" help", dim),
    ];
    if app.view == View::Procs {
        left.extend([Span::styled("  ", dim), key("c/m/p/n"), Span::styled(" sort  ", dim), key("↑↓"), Span::styled(" select", dim)]);
    }
    if app.view == View::Wifi {
        left.extend([Span::styled("  ", dim), key("↑↓"), Span::styled(" networks  ", dim), key("s"), Span::styled(" rescan", dim)]);
        if let Some(t) = app.rescan_at {
            if t.elapsed().as_secs() < 8 {
                left.extend([Span::styled("  ", dim), Span::styled("scanning…", Style::new().fg(Color::Yellow))]);
            }
        }
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
        help(frame, area, &th);
    }
}

fn help(frame: &mut Frame, area: Rect, th: &Theme) {
    let w = 62.min(area.width.saturating_sub(2));
    let h = 20.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let r = Rect::new(x, y, w, h);
    frame.render_widget(Clear, r);
    let k = |a: &str, b: &str| {
        Line::from(vec![Span::styled(format!("  {:<12}", a), Style::new().fg(th.accent).bold()), Span::raw(b.to_string())])
    };
    let text = vec![
        Line::raw(""),
        k("q / Esc", "quit"),
        k("Tab / → / l", "next view"),
        k("S-Tab / ← / h", "previous view"),
        k("1 … 9", "jump to view"),
        k("↑ ↓ / j k", "move selection (processes, Wi-Fi networks)"),
        k("c m p n", "sort processes by cpu / memory / pid / name"),
        k("space", "pause / resume sampling"),
        k("r", "refresh now"),
        k("s", "rescan Wi-Fi networks (via NetworkManager)"),
        k("+ / -", "faster / slower refresh"),
        k("?", "toggle this help"),
        Line::raw(""),
        Line::from(Span::styled("  Flags: --bat --wifi --disk --cpu --mem --net --temp --ps", Style::new().fg(Color::Gray))),
        Line::from(Span::styled("         --once prints a single frame and exits", Style::new().fg(Color::Gray))),
        Line::from(Span::styled("         -i <ms> sets the refresh interval", Style::new().fg(Color::Gray))),
    ];
    let b: Block = section("help", th);
    frame.render_widget(Paragraph::new(text).block(b).wrap(Wrap { trim: false }), r);
}
