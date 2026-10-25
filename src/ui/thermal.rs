use crate::app::{tail, App};
use crate::collect::thermal::Kind;
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let t = &s.thermal;
    let f = app.cfg.fahrenheit;
    let rows = Layout::vertical([Constraint::Length(6), Constraint::Min(6), Constraint::Length(7)]).split(area);

    {
        let b = section("in plain English", th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        let mut lines = Vec::new();
        match t.hottest() {
            Some(h) => {
                let words = if h.value < 50.0 {
                    ("Cool and quiet.", th.good)
                } else if h.value < 70.0 {
                    ("Warm — normal under load.", th.good)
                } else if h.value < 85.0 {
                    ("Hot — fans should be working; performance may be trimmed.", th.warn)
                } else {
                    ("Very hot — check airflow and vents.", th.bad)
                };
                lines.push(Line::from(vec![
                    fg("Hottest  ", th.label),
                    bold(temp(h.value, f), th.level(h.value)),
                    dim(format!("  {} on {}", h.label, h.chip)),
                ]));
                lines.push(Line::from(vec![fg("Verdict  ", th.label), fg(words.0, words.1)]));
            }
            None => lines.push(Line::from(dim("No temperature sensors exposed by the kernel."))),
        }
        let fans: Vec<String> = t.fans().map(|f| format!("{} {:.0} RPM", f.label, f.value)).collect();
        lines.push(Line::from(vec![
            fg("Fans     ", th.label),
            fg(if fans.is_empty() { "none reported (fanless, or no driver)".into() } else { fans.join(" · ") }, Color::White),
        ]));
        lines.push(Line::from(vec![
            fg("Sensors  ", th.label),
            fg(
                format!("{} readings from {} chips", t.sensors.len(), {
                    let mut c: Vec<&str> = t.sensors.iter().map(|s| s.chip.as_str()).collect();
                    c.dedup();
                    c.len()
                }),
                Color::White,
            ),
        ]));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    {
        let b = section_with_right("all sensors", "hwmon", th);
        let hdr = Row::new(vec!["CHIP", "SENSOR", "KIND", "VALUE", "", "MAX", "CRIT"]).style(Style::new().fg(th.muted));
        let mut sensors: Vec<_> = t.sensors.iter().collect();
        sensors
            .sort_by(|a, b| (a.kind as u8).cmp(&(b.kind as u8)).then(b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal)));
        let trs: Vec<Row> = sensors
            .iter()
            .map(|x| {
                let (txt, color, ratio) = match x.kind {
                    Kind::Temp => (
                        format!("{:>7.1} {}", temp_val(x.value, f), temp_unit(f)),
                        th.level(x.value),
                        x.value / x.crit.or(x.max).unwrap_or(100.0).max(1.0),
                    ),
                    Kind::Fan => (format!("{:>6.0} RPM", x.value), Color::White, x.value / x.max.unwrap_or(6000.0).max(1.0)),
                    Kind::Volt => (format!("{:>7.3} V", x.value), Color::White, 0.0),
                    Kind::Curr => (format!("{:>7.3} A", x.value), Color::White, 0.0),
                    Kind::Power => (format!("{:>7.2} W", x.value), th.accent, 0.0),
                };
                Row::new(vec![
                    Cell::from(Span::styled(x.chip.clone(), Style::new().fg(th.muted))),
                    Cell::from(Span::styled(x.label.clone(), Style::new().fg(Color::White))),
                    Cell::from(x.kind.name()),
                    Cell::from(Span::styled(txt, Style::new().fg(color))),
                    Cell::from(Span::styled(if ratio > 0.0 { bar(ratio, 14) } else { String::new() }, Style::new().fg(color))),
                    Cell::from(if x.kind == Kind::Temp {
                        optf(x.max.map(|v| temp_val(v, f)), 0, temp_unit(f))
                    } else {
                        optf(x.max, 0, x.kind.unit())
                    }),
                    Cell::from(if x.kind == Kind::Temp {
                        optf(x.crit.map(|v| temp_val(v, f)), 0, temp_unit(f))
                    } else {
                        optf(x.crit, 0, x.kind.unit())
                    }),
                ])
            })
            .collect();
        let tb = Table::new(
            trs,
            [
                Constraint::Length(16),
                Constraint::Min(20),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Length(8),
                Constraint::Length(8),
            ],
        )
        .header(hdr)
        .block(b);
        frame.render_widget(tb, rows[1]);
    }

    {
        let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[2]);
        let b = section("thermal zones & cooling", th);
        let inner = b.inner(cols[0]);
        frame.render_widget(b, cols[0]);
        let mut kv: Vec<(&str, String)> = Vec::new();
        for (name, v) in &t.zones {
            kv.push((Box::leak(name.clone().into_boxed_str()), temp(*v, f)));
        }
        for (name, cur, max) in t.cooling.iter().take(4) {
            kv.push((Box::leak(format!("cooling {}", name).into_boxed_str()), format!("state {}/{}", cur, max)));
        }
        if kv.is_empty() {
            kv.push(("thermal zones", "none".into()));
        }
        frame.render_widget(kv_table(kv, th, 24), inner);

        let b = section("hottest sensor history", th);
        let inner = b.inner(cols[1]);
        frame.render_widget(b, cols[1]);
        let r = Layout::vertical([Constraint::Length(1), Constraint::Min(2)]).split(inner);
        let hist = tail(&app.hist.temp, r[1].width as usize);
        let (mn, mx) = (hist.iter().min().copied().unwrap_or(0) as f64 / 10.0, hist.iter().max().copied().unwrap_or(0) as f64 / 10.0);
        frame.render_widget(
            Paragraph::new(Line::from(dim(format!(
                "range {:.1} – {:.1} {} over {} samples",
                temp_val(mn, f),
                temp_val(mx, f),
                temp_unit(f),
                hist.len()
            )))),
            r[0],
        );
        frame.render_widget(spark(&hist, None, th.accent), r[1]);
    }
}
