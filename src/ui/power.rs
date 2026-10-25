use crate::app::{tail, App};
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let p = &s.power;
    let rows = Layout::vertical([Constraint::Length(8), Constraint::Length(13), Constraint::Min(6)]).split(area);

    // ── 1. plain English ─────────────────────────────────────────────
    {
        let b = section("in plain English", th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        match p.batteries.first() {
            Some(bat) => {
                let pct = bat.capacity_pct.unwrap_or(0.0);
                let color = th.level_inv(pct);
                let r = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);
                gauge_rows(frame, r[0], &[("Battery".into(), pct / 100.0, format!("{:.0}%", pct), color)], th, 9);
                let (headline, hc) = if bat.is_charging() {
                    (
                        match bat.time_to_full_s {
                            Some(t) => {
                                format!("Charging — full in {}{}", dur_words(t), if bat.time_estimated { " (estimated)" } else { "" })
                            }
                            None => "Charging".into(),
                        },
                        th.good,
                    )
                } else if bat.is_discharging() {
                    (
                        match bat.time_to_empty_s {
                            Some(t) => format!(
                                "On battery — {} left at the current pace{}",
                                dur_words(t),
                                if bat.time_estimated { " (estimated)" } else { "" }
                            ),
                            None => "On battery".into(),
                        },
                        if pct > 20.0 { Color::White } else { th.bad },
                    )
                } else if bat.status.eq_ignore_ascii_case("full") {
                    ("Plugged in — battery is full".into(), th.good)
                } else if bat.status.eq_ignore_ascii_case("not charging") {
                    ("Plugged in — holding charge (not charging right now)".into(), Color::White)
                } else {
                    (format!("Status: {}", bat.status), Color::White)
                };
                frame.render_widget(Paragraph::new(Line::from(vec![fg("Status   ", th.label), bold(headline, hc)])), r[1]);
                let draw = match bat.power_w {
                    Some(w) if bat.is_discharging() => format!(
                        "Using {:.1} W right now{}",
                        w,
                        if w < 5.0 {
                            " — very light use"
                        } else if w < 12.0 {
                            " — typical laptop use"
                        } else if w < 25.0 {
                            " — working hard"
                        } else {
                            " — heavy load, battery will drain quickly"
                        }
                    ),
                    Some(w) if bat.is_charging() => format!("Charging at {:.1} W", w),
                    _ => "Power draw not reported".into(),
                };
                frame.render_widget(Paragraph::new(Line::from(vec![fg("Power    ", th.label), fg(draw, Color::White)])), r[2]);
                let health = match bat.health_pct {
                    Some(h) => {
                        let words = if h >= 90.0 {
                            "excellent"
                        } else if h >= 80.0 {
                            "good"
                        } else if h >= 70.0 {
                            "worn — noticeably less runtime than new"
                        } else {
                            "tired — consider a replacement"
                        };
                        format!(
                            "{:.0}% of original capacity ({}){}",
                            h,
                            words,
                            bat.cycles.map(|c| format!(" · {} charge cycles", c)).unwrap_or_default()
                        )
                    }
                    None => "unknown".into(),
                };
                frame.render_widget(
                    Paragraph::new(Line::from(vec![fg("Health   ", th.label), fg(health, th.level_inv(bat.health_pct.unwrap_or(100.0)))])),
                    r[3],
                );
                let ac = if p.on_ac {
                    let src: Vec<String> = p
                        .supplies
                        .iter()
                        .filter(|s| s.online)
                        .map(|s| {
                            format!(
                                "{}{}",
                                s.kind,
                                s.power_w
                                    .map(|w| format!(" {:.0} W", w))
                                    .or(s.input_power_limit_w.map(|w| format!(" up to {:.0} W", w)))
                                    .unwrap_or_default()
                            )
                        })
                        .collect();
                    format!("Plugged in ({})", if src.is_empty() { "mains".into() } else { src.join(", ") })
                } else {
                    "Unplugged".into()
                };
                frame.render_widget(Paragraph::new(Line::from(vec![fg("Charger  ", th.label), fg(ac, Color::White)])), r[4]);
                let tip = if bat.is_discharging() && pct <= 15.0 {
                    ("Plug in soon.", th.bad)
                } else if bat.end_threshold.map(|t| t < 100).unwrap_or(false) {
                    ("A charge limit is set to protect battery longevity.", th.good)
                } else if let Some(t) = bat.temp_c.filter(|t| *t > 40.0) {
                    (if t > 45.0 { "Battery is hot — let it cool for its own sake." } else { "Battery is warm." }, th.warn)
                } else {
                    ("Nothing to worry about.", th.good)
                };
                frame.render_widget(Paragraph::new(Line::from(vec![fg("Tip      ", th.label), fg(tip.0, tip.1)])), r[5]);
            }
            None => {
                let lines = vec![
                    Line::from(vec![
                        fg("● ", th.good),
                        bold("Running on mains power", Color::White),
                        dim(" — no battery was detected in this machine."),
                    ]),
                    Line::from(dim("That is normal for a desktop or server. Power supplies and platform settings are listed below.")),
                ];
                frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
            }
        }
    }

    // ── 2. battery detail ────────────────────────────────────────────
    {
        let b = section("battery detail", th);
        let inner = b.inner(rows[1]);
        frame.render_widget(b, rows[1]);
        if let Some(bat) = p.batteries.first() {
            let cols = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)]).split(inner);
            let energy = vec![
                ("energy now", optf(bat.energy_now_wh, 2, " Wh")),
                ("energy full", optf(bat.energy_full_wh, 2, " Wh")),
                ("energy design", optf(bat.energy_full_design_wh, 2, " Wh")),
                ("charge now", optf(bat.charge_now_ah.map(|v| v * 1000.0), 0, " mAh")),
                ("charge full", optf(bat.charge_full_ah.map(|v| v * 1000.0), 0, " mAh")),
                ("charge design", optf(bat.charge_full_design_ah.map(|v| v * 1000.0), 0, " mAh")),
                ("capacity level", bat.capacity_level.clone().unwrap_or_else(|| "—".into())),
                ("time to empty", opt(bat.time_to_empty_s.map(dur), "")),
                ("time to full", opt(bat.time_to_full_s.map(dur), "")),
                ("wear", optf(bat.health_pct.map(|h| 100.0 - h), 1, "%")),
                ("cycles", opt(bat.cycles, "")),
            ];
            frame.render_widget(kv_table(energy, th, 15), cols[0]);
            let elec = vec![
                ("voltage", optf(bat.voltage_v, 3, " V")),
                ("current", optf(bat.current_a.map(|a| a * 1000.0), 0, " mA")),
                ("power", optf(bat.power_w, 2, " W")),
                (
                    "design voltage",
                    match (bat.voltage_min_design_v, bat.voltage_max_design_v) {
                        (Some(a), Some(b)) => format!("{:.2} – {:.2} V", a, b),
                        (Some(a), None) => format!("{:.2} V", a),
                        _ => "—".into(),
                    },
                ),
                (
                    "cells (est.)",
                    bat.voltage_max_design_v.map(|v| format!("{}S", (v / 4.3).round().max(1.0) as u32)).unwrap_or_else(|| "—".into()),
                ),
                ("temperature", bat.temp_c.map(|c| temp(c, app.cfg.fahrenheit)).unwrap_or_else(|| "—".into())),
                (
                    "charge limit",
                    match (bat.start_threshold, bat.end_threshold) {
                        (Some(a), Some(b)) => format!("start {}% · stop {}%", a, b),
                        (None, Some(b)) => format!("stop {}%", b),
                        _ => "none".into(),
                    },
                ),
                ("charge behaviour", bat.charge_behaviour.clone().unwrap_or_else(|| "—".into())),
                (
                    "CC max / CV",
                    format!("{} / {}", optf(bat.const_charge_current_max_a, 2, " A"), optf(bat.const_charge_voltage_v, 2, " V")),
                ),
                ("term current", optf(bat.charge_term_current_a.map(|a| a * 1000.0), 0, " mA")),
            ];
            frame.render_widget(kv_table(elec, th, 17), cols[1]);
            let ident = vec![
                ("device", bat.name.clone()),
                ("model", bat.model.clone().unwrap_or_else(|| "—".into())),
                ("manufacturer", bat.manufacturer.clone().unwrap_or_else(|| "—".into())),
                ("technology", bat.technology.clone().unwrap_or_else(|| "—".into())),
                ("serial", bat.serial.clone().unwrap_or_else(|| "—".into())),
                ("manufactured", bat.manufactured.clone().unwrap_or_else(|| "—".into())),
                ("kernel health", bat.health.clone().unwrap_or_else(|| "—".into())),
                ("present", if bat.present { "yes".into() } else { "no".into() }),
                ("status", bat.status.clone()),
            ];
            frame.render_widget(kv_table(ident, th, 14), cols[2]);
        } else {
            frame.render_widget(Paragraph::new(Line::from(dim("no battery"))), inner);
        }
    }

    // ── 3. supplies, rails, settings, history ────────────────────────
    {
        let cols = Layout::horizontal([Constraint::Percentage(36), Constraint::Percentage(32), Constraint::Percentage(32)]).split(rows[2]);
        let b = section("chargers & supplies", th);
        let wide = cols[0].width >= 64;
        let mut hdr_cells = vec!["NAME", "TYPE", "STATE"];
        if wide {
            hdr_cells.extend(["VOLT", "CURR"]);
        }
        hdr_cells.extend(["POWER", "LIMIT"]);
        let hdr = Row::new(hdr_cells).style(Style::new().fg(th.muted));
        let trs: Vec<Row> = p
            .supplies
            .iter()
            .map(|x| {
                let mut cells = vec![
                    Cell::from(Span::styled(x.name.clone(), Style::new().fg(Color::White))),
                    Cell::from(format!("{}{}", x.kind, x.usb_type.as_ref().map(|t| format!(" ({})", t)).unwrap_or_default())),
                    Cell::from(Span::styled(
                        if x.online { "online" } else { "idle" },
                        Style::new().fg(if x.online { th.good } else { th.muted }),
                    )),
                ];
                if wide {
                    cells.push(Cell::from(optf(x.voltage_v, 1, " V")));
                    cells.push(Cell::from(optf(x.current_a, 2, " A")));
                }
                cells.push(Cell::from(optf(x.power_w, 1, " W")));
                cells.push(Cell::from(optf(x.input_power_limit_w, 0, " W")));
                Row::new(cells)
            })
            .collect();
        let mut widths = vec![Constraint::Min(12), Constraint::Length(10), Constraint::Length(6)];
        if wide {
            widths.extend([Constraint::Length(7), Constraint::Length(7)]);
        }
        widths.extend([Constraint::Length(7), Constraint::Length(6)]);
        let t = Table::new(trs, widths).header(hdr).block(b);
        frame.render_widget(t, cols[0]);

        let b = section("platform & rails", th);
        let inner = b.inner(cols[1]);
        frame.render_widget(b, cols[1]);
        let mut kv: Vec<(&str, String)> = vec![
            ("cpu governor", p.governor.clone().unwrap_or_else(|| "—".into())),
            ("energy pref", p.epp.clone().unwrap_or_else(|| "—".into())),
            ("platform profile", p.platform_profile.clone().unwrap_or_else(|| "n/a".into())),
            ("screen brightness", optf(p.backlight_pct, 0, "%")),
            (
                "lid",
                match p.lid_open {
                    Some(true) => "open".into(),
                    Some(false) => "closed".into(),
                    None => "—".into(),
                },
            ),
        ];
        for (label, w) in p.rails.iter().take(8) {
            kv.push((Box::leak(label.clone().into_boxed_str()), format!("{:.2} W", w)));
        }
        frame.render_widget(kv_table(kv, th, 22), inner);

        let b = section("power draw history", th);
        let inner = b.inner(cols[2]);
        frame.render_widget(b, cols[2]);
        let r = Layout::vertical([Constraint::Length(1), Constraint::Min(2)]).split(inner);
        let now = p.batteries.first().and_then(|b| b.power_w).or(p.system_power_w);
        let avg = {
            let d = &app.hist.batt_mw;
            if d.is_empty() {
                None
            } else {
                Some(d.iter().sum::<u64>() as f64 / d.len() as f64 / 1000.0)
            }
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg("battery W  ", th.label),
                fg(optf(now, 1, " W"), Color::White),
                dim(format!("  avg {} over {} samples", optf(avg, 1, " W"), app.hist.batt_mw.len())),
            ])),
            r[0],
        );
        frame.render_widget(spark(&tail(&app.hist.batt_mw, r[1].width as usize), None, th.accent), r[1]);
    }
}
