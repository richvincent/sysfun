use crate::app::{tail, App};
use crate::ui::widgets::*;
use crate::ui::wifi::quality;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Length(10), Constraint::Min(0)]).split(area);

    // ── banner ───────────────────────────────────────────────────────
    let h = &app.host;
    let banner = vec![Line::from(vec![
        dim(format!("{}@", h.user)),
        bold(h.hostname.clone(), th.accent),
        dim("  ·  "),
        fg(h.model.clone(), Color::White),
        dim("  ·  "),
        fg(h.os.clone(), Color::Gray),
        dim("  ·  "),
        fg(format!("Linux {}", h.kernel), Color::Gray),
        dim("  ·  "),
        fg(format!("up {}", dur(s.uptime)), Color::Gray),
        dim("  ·  "),
        fg(h.cpu_model.clone(), Color::Gray),
        dim(if h.desktop.is_empty() { String::new() } else { format!("  ·  {}", h.desktop) }),
    ])];
    let b = section("this machine", th);
    frame.render_widget(Paragraph::new(banner).block(b), rows[0]);

    // ── gauges ───────────────────────────────────────────────────────
    let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[1]);
    {
        let b = section("compute", th);
        let inner = b.inner(cols[0]);
        frame.render_widget(b, cols[0]);
        let r = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let cpu = s.cpu.total_pct;
        gauge_rows(
            frame,
            r[0],
            &[("CPU".into(), cpu / 100.0, format!("{:>5.1}%  {} cores", cpu, s.cpu.per_core.len()), th.level(cpu))],
            th,
            7,
        );
        frame.render_widget(spark(&tail(&app.hist.cpu, r[1].width as usize), Some(100), th.accent), r[1]);
        let m = &s.mem;
        gauge_rows(
            frame,
            r[2],
            &[(
                "Memory".into(),
                m.used_pct() / 100.0,
                format!("{:>5.1}%  {} of {}", m.used_pct(), bytes(m.used), bytes(m.total)),
                th.level(m.used_pct()),
            )],
            th,
            7,
        );
        gauge_rows(
            frame,
            r[3],
            &[(
                "Swap".into(),
                m.swap_pct() / 100.0,
                format!("{:>5.1}%  {} of {}", m.swap_pct(), bytes(m.swap_used), bytes(m.swap_total)),
                th.level(m.swap_pct()),
            )],
            th,
            7,
        );
        let (l1, l5, l15) = s.cpu.load;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg("Load   ", th.label),
                fg(format!("{:.2} {:.2} {:.2}", l1, l5, l15), Color::White),
                dim(format!("   {} running · {} procs · {} threads", s.cpu.running, s.procs.total, s.procs.threads)),
            ])),
            r[4],
        );
        let freq: Vec<u32> = s.cpu.freq_mhz.iter().flatten().copied().collect();
        if !freq.is_empty() {
            let avg = freq.iter().sum::<u32>() as f64 / freq.len() as f64 / 1000.0;
            let max = freq.iter().max().copied().unwrap_or(0) as f64 / 1000.0;
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    fg("Clock  ", th.label),
                    fg(format!("{:.2} GHz avg · {:.2} GHz peak", avg, max), Color::White),
                    dim(format!("   {}", s.cpu.governor.clone().unwrap_or_default())),
                ])),
                r[5],
            );
        }
    }
    {
        let b = section("around you", th);
        let inner = b.inner(cols[1]);
        frame.render_widget(b, cols[1]);
        let r = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let mut i = 0;
        if let Some(bat) = s.power.batteries.first() {
            let pct = bat.capacity_pct.unwrap_or(0.0);
            let time = if bat.is_discharging() {
                bat.time_to_empty_s.map(|t| format!("{} left", dur(t))).unwrap_or_default()
            } else if bat.is_charging() {
                bat.time_to_full_s.map(|t| format!("{} to full", dur(t))).unwrap_or_default()
            } else {
                String::new()
            };
            let label = format!("{:>3.0}%  {}  {}  {}", pct, bat.status, optf(bat.power_w, 1, " W"), time);
            gauge_rows(frame, r[i], &[("Battery".into(), pct / 100.0, label, th.level_inv(pct))], th, 8);
            i += 1;
        } else {
            frame.render_widget(Paragraph::new(Line::from(vec![fg("Power   ", th.label), fg("mains (no battery)", Color::White)])), r[i]);
            i += 1;
        }
        if let Some(l) = &s.wifi.link {
            let line = if l.connected {
                let (q, bars, color, _) = quality(l.signal_dbm.unwrap_or(-100));
                Line::from(vec![
                    fg("Wi-Fi   ", th.label),
                    bold(l.ssid.clone(), Color::White),
                    Span::raw("  "),
                    fg(signal_bars(bars), color),
                    fg(format!(" {} ", q), color),
                    dim(format!("{} dBm · {} ch {} · {}", l.signal_dbm.unwrap_or(0), l.band(), l.chan, bits(l.tx_mbps * 1e6))),
                ])
            } else {
                Line::from(vec![
                    fg("Wi-Fi   ", th.label),
                    fg("not connected", th.warn),
                    dim(format!("  {} networks nearby", s.wifi.scan.len())),
                ])
            };
            frame.render_widget(Paragraph::new(line), r[i]);
            i += 1;
        }
        if let Some((ifn, gw)) = &s.net.default_v4 {
            let ip = s.net.ifaces.iter().find(|x| &x.name == ifn).and_then(|x| x.ipv4.first().cloned()).unwrap_or_default();
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    fg("Network ", th.label),
                    fg(format!("{} ", ip), Color::White),
                    dim(format!("via {} on {}   ", gw, ifn)),
                    fg(format!("↓ {}", rate(s.net.total_rx_bps)), th.good),
                    Span::raw("  "),
                    fg(format!("↑ {}", rate(s.net.total_tx_bps)), th.warn),
                ])),
                r[i],
            );
            i += 1;
        } else {
            frame.render_widget(Paragraph::new(Line::from(vec![fg("Network ", th.label), fg("offline — no default route", th.bad)])), r[i]);
            i += 1;
        }
        for m in s.disk.mounts.iter().filter(|m| !m.remote).take(3) {
            if i >= r.len() {
                break;
            }
            let label = format!("{:>5.1}%  {} free of {}  {}", m.used_pct(), bytes(m.avail), bytes(m.total), m.fstype);
            let name = if m.mount.len() > 7 { format!("{:.6}…", m.mount) } else { m.mount.clone() };
            gauge_rows(frame, r[i], &[(name, m.used_pct() / 100.0, label, th.level(m.used_pct()))], th, 8);
            i += 1;
        }
        if i < r.len() {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    fg("Disk IO ", th.label),
                    fg(format!("read {}", rate(s.disk.total_read_bps)), th.good),
                    Span::raw("  "),
                    fg(format!("write {}", rate(s.disk.total_write_bps)), th.warn),
                ])),
                r[i],
            );
            i += 1;
        }
        if i < r.len() {
            if let Some(hot) = s.thermal.hottest() {
                let fans: Vec<String> = s.thermal.fans().map(|f| format!("{:.0} RPM", f.value)).collect();
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        fg("Thermal ", th.label),
                        fg(format!("{:.1}°C", hot.value), th.level(hot.value)),
                        dim(format!("  {} ({})", hot.label, hot.chip)),
                        dim(if fans.is_empty() { String::new() } else { format!("  · fans {}", fans.join(", ")) }),
                    ])),
                    r[i],
                );
            }
        }
    }

    // ── bottom: processes + sensors ──────────────────────────────────
    let bot = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)]).split(rows[2]);
    {
        let b = section_with_right("busiest processes", "by cpu", th);
        let mut procs: Vec<_> = s.procs.procs.iter().collect();
        procs.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal).then(b.rss.cmp(&a.rss)));
        let hdr = Row::new(vec!["PID", "CPU%", "MEM", "USER", "COMMAND"]).style(Style::new().fg(th.muted));
        let trs: Vec<Row> = procs
            .iter()
            .take(bot[0].height.saturating_sub(3) as usize)
            .map(|p| {
                Row::new(vec![
                    Cell::from(format!("{}", p.pid)),
                    Cell::from(Span::styled(format!("{:>5.1}", p.cpu_pct), Style::new().fg(th.level(p.cpu_pct)))),
                    Cell::from(bytes(p.rss)),
                    Cell::from(Span::styled(p.user.clone(), Style::new().fg(th.label))),
                    Cell::from(p.cmd.clone()),
                ])
            })
            .collect();
        let t = Table::new(
            trs,
            [Constraint::Length(7), Constraint::Length(6), Constraint::Length(9), Constraint::Length(10), Constraint::Min(10)],
        )
        .header(hdr)
        .block(b);
        frame.render_widget(t, bot[0]);
    }
    {
        let b = section("sensors", th);
        let mut temps: Vec<_> = s.thermal.temps().collect();
        temps.sort_by(|a, b| b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal));
        let trs: Vec<Row> = temps
            .iter()
            .take(bot[1].height.saturating_sub(2) as usize)
            .map(|t| {
                Row::new(vec![
                    Cell::from(Span::styled(t.label.clone(), Style::new().fg(th.label))),
                    Cell::from(Span::styled(format!("{:>6.1}°C", t.value), Style::new().fg(th.level(t.value)))),
                    Cell::from(Span::styled(bar(t.value / 100.0, 10), Style::new().fg(th.level(t.value)))),
                ])
            })
            .collect();
        let t = Table::new(trs, [Constraint::Min(10), Constraint::Length(8), Constraint::Length(10)]).block(b);
        frame.render_widget(t, bot[1]);
    }
}
