use crate::app::{tail, App};
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let n = &s.net;
    let if_h = (n.ifaces.len() as u16 + 3).clamp(4, 12);
    let rows = Layout::vertical([Constraint::Length(6), Constraint::Length(if_h), Constraint::Min(6)]).split(area);

    // ── plain summary ────────────────────────────────────────────────
    {
        let b = section("connection", th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        let mut lines = Vec::new();
        match &n.default_v4 {
            Some((ifn, gw)) => {
                let iface = n.ifaces.iter().find(|x| &x.name == ifn);
                let ip = iface.and_then(|x| x.ipv4.first().cloned()).unwrap_or_else(|| "no address".into());
                let kind = iface.map(|x| x.kind.clone()).unwrap_or_default();
                lines.push(Line::from(vec![
                    fg("● ", th.good),
                    bold("Online", th.good),
                    dim(format!("  via {} ({}) · this machine is ", ifn, kind)),
                    fg(ip, Color::White),
                    dim(" · router "),
                    fg(gw.clone(), Color::White),
                ]));
            }
            None => lines.push(Line::from(vec![
                fg("● ", th.bad),
                bold("Offline", th.bad),
                dim("  no default route — nothing can reach the internet"),
            ])),
        }
        let dns = if n.dns_upstream.is_empty() {
            n.dns.join(", ")
        } else {
            format!("{} (stub) → {}", n.dns.join(", "), n.dns_upstream.join(", "))
        };
        lines.push(Line::from(vec![
            fg("DNS      ", th.label),
            fg(if dns.is_empty() { "none configured".into() } else { dns }, Color::White),
            dim(if n.search.is_empty() { String::new() } else { format!("   search {}", n.search.join(" ")) }),
        ]));
        if let Some((ifn, gw)) = &n.default_v6 {
            lines.push(Line::from(vec![fg("IPv6     ", th.label), fg(format!("default via {} on {}", gw, ifn), Color::White)]));
        } else {
            lines.push(Line::from(vec![fg("IPv6     ", th.label), dim("no IPv6 default route")]));
        }
        lines.push(Line::from(vec![
            fg("Traffic  ", th.label),
            fg(format!("↓ {}", rate(n.total_rx_bps)), th.good),
            Span::raw("   "),
            fg(format!("↑ {}", rate(n.total_tx_bps)), th.warn),
            dim("   (all interfaces except loopback)"),
        ]));
        frame.render_widget(Paragraph::new(lines), inner);
    }

    // ── interfaces ───────────────────────────────────────────────────
    {
        let b = section("interfaces", th);
        let hdr =
            Row::new(vec!["", "NAME", "TYPE", "STATE", "IPv4", "IPv6", "RX/s", "TX/s", "RX total", "TX total", "ERR/DROP", "MTU", "MAC"])
                .style(Style::new().fg(th.muted));
        let trs: Vec<Row> = n
            .ifaces
            .iter()
            .map(|i| {
                let up = i.state == "up" || (i.state == "unknown" && i.carrier) || i.kind == "loopback" || i.kind == "vpn/tunnel";
                let state_c = if up { th.good } else { th.muted };
                let v6 = i.ipv6.first().cloned().unwrap_or_else(|| "—".into());
                let errs = i.rx_errors + i.tx_errors;
                let drops = i.rx_dropped + i.tx_dropped;
                Row::new(vec![
                    Cell::from(Span::styled(if i.is_default { "★" } else { " " }, Style::new().fg(th.accent))),
                    Cell::from(Span::styled(i.name.clone(), Style::new().fg(Color::White).bold())),
                    Cell::from(i.kind.clone()),
                    Cell::from(Span::styled(
                        if i.state == "unknown" && i.carrier { "up".to_string() } else { i.state.clone() },
                        Style::new().fg(state_c),
                    )),
                    Cell::from(i.ipv4.first().cloned().unwrap_or_else(|| "—".into())),
                    Cell::from(Span::styled(v6, Style::new().fg(th.label))),
                    Cell::from(Span::styled(rate(i.rx_bps), Style::new().fg(if i.rx_bps > 1000.0 { th.good } else { Color::Gray }))),
                    Cell::from(Span::styled(rate(i.tx_bps), Style::new().fg(if i.tx_bps > 1000.0 { th.warn } else { Color::Gray }))),
                    Cell::from(bytes(i.rx_bytes)),
                    Cell::from(bytes(i.tx_bytes)),
                    Cell::from(Span::styled(
                        format!("{}/{}", errs, drops),
                        Style::new().fg(if errs > 0 {
                            th.bad
                        } else if drops > 0 {
                            th.warn
                        } else {
                            Color::Gray
                        }),
                    )),
                    Cell::from(format!("{}", i.mtu)),
                    Cell::from(Span::styled(i.mac.clone(), Style::new().fg(th.muted))),
                ])
            })
            .collect();
        let t = Table::new(
            trs,
            [
                Constraint::Length(1),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(7),
                Constraint::Length(19),
                Constraint::Min(16),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(8),
                Constraint::Length(5),
                Constraint::Length(17),
            ],
        )
        .header(hdr)
        .block(b);
        frame.render_widget(t, rows[1]);
    }

    // ── details ──────────────────────────────────────────────────────
    {
        let cols = Layout::horizontal([Constraint::Percentage(34), Constraint::Percentage(33), Constraint::Percentage(33)]).split(rows[2]);
        let b = section("throughput history", th);
        let inner = b.inner(cols[0]);
        frame.render_widget(b, cols[0]);
        let r = Layout::vertical([Constraint::Length(1), Constraint::Min(2), Constraint::Length(1), Constraint::Min(2)]).split(inner);
        frame.render_widget(Paragraph::new(Line::from(vec![fg("download ", th.good), fg(rate(n.total_rx_bps), Color::White)])), r[0]);
        frame.render_widget(spark(&tail(&app.hist.net_rx, r[1].width as usize), None, th.good), r[1]);
        frame.render_widget(Paragraph::new(Line::from(vec![fg("upload   ", th.warn), fg(rate(n.total_tx_bps), Color::White)])), r[2]);
        frame.render_widget(spark(&tail(&app.hist.net_tx, r[3].width as usize), None, th.warn), r[3]);

        let b = section("sockets & TCP", th);
        let inner = b.inner(cols[1]);
        frame.render_widget(b, cols[1]);
        let mut kv: Vec<(&str, String)> = Vec::new();
        if let Some(sk) = &n.sockets {
            kv.push(("sockets in use", format!("{}", sk.used)));
            kv.push(("tcp in use", format!("{} (alloc {})", sk.tcp_inuse, sk.tcp_alloc)));
            kv.push(("tcp time-wait", format!("{}", sk.tcp_tw)));
            kv.push(("tcp orphaned", format!("{}", sk.tcp_orphan)));
            kv.push(("udp in use", format!("{}", sk.udp_inuse)));
        }
        if let Some(t) = &n.tcp {
            kv.push(("tcp established", format!("{}", t.cur_estab)));
            kv.push(("segments in/out /s", format!("{:.0} / {:.0}", t.in_segs_per_s, t.out_segs_per_s)));
            kv.push(("retransmits /s", format!("{:.1}", t.retrans_per_s)));
            kv.push((
                "retrans ratio",
                if t.out_segs_per_s > 0.0 { format!("{:.2}%", t.retrans_per_s / t.out_segs_per_s * 100.0) } else { "—".into() },
            ));
            kv.push(("opens act/pass", format!("{} / {}", t.active_opens, t.passive_opens)));
            kv.push(("tcp in errors", format!("{}", t.in_errs)));
        }
        frame.render_widget(kv_table(kv, th, 19), inner);

        let b = section("default interface", th);
        let inner = b.inner(cols[2]);
        frame.render_widget(b, cols[2]);
        if let Some(i) = n.ifaces.iter().find(|i| i.is_default).or_else(|| n.ifaces.first()) {
            let mut kv: Vec<(&str, String)> = vec![("name", format!("{} ({})", i.name, i.kind))];
            for a in &i.ipv4 {
                kv.push(("ipv4", a.clone()));
            }
            for a in i.ipv6.iter().take(3) {
                kv.push(("ipv6", a.clone()));
            }
            kv.push(("mac", i.mac.clone()));
            kv.push(("mtu", format!("{}", i.mtu)));
            if let Some(sp) = i.speed_mbps {
                kv.push(("link speed", format!("{} Mbit/s {}", sp, i.duplex.clone().unwrap_or_default())));
            }
            kv.push(("packets rx/tx", format!("{} / {}", i.rx_packets, i.tx_packets)));
            kv.push(("pps rx/tx", format!("{:.0} / {:.0}", i.rx_pps, i.tx_pps)));
            kv.push(("errors rx/tx", format!("{} / {}", i.rx_errors, i.tx_errors)));
            kv.push(("dropped rx/tx", format!("{} / {}", i.rx_dropped, i.tx_dropped)));
            kv.push(("driver", i.driver.clone().unwrap_or_else(|| "—".into())));
            frame.render_widget(kv_table(kv, th, 15), inner);
        }
    }
}
