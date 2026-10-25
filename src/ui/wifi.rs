//! Wi-Fi view: plain English at the top, RF engineering at the bottom.

use crate::app::{tail, App};
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState, Wrap};

/// (label, bars 0–4, color, what it means)
pub fn quality(dbm: i32) -> (&'static str, usize, Color, &'static str) {
    if dbm >= -50 {
        ("Excellent", 4, Color::Rgb(120, 200, 120), "as good as it gets — video calls, 4K streaming, big downloads all fine")
    } else if dbm >= -60 {
        ("Good", 3, Color::Rgb(120, 200, 120), "solid — streaming and calls should be smooth")
    } else if dbm >= -67 {
        ("Fair", 2, Color::Rgb(230, 190, 90), "okay for browsing and HD video; calls may stutter now and then")
    } else if dbm >= -75 {
        ("Weak", 1, Color::Rgb(230, 190, 90), "pages load, but video and calls will struggle — move closer to the router")
    } else {
        ("Poor", 0, Color::Rgb(240, 100, 100), "barely connected — expect drops; move closer or switch networks")
    }
}

fn speed_words(mbps: f64) -> &'static str {
    if mbps >= 300.0 {
        "fast enough for anything you can throw at it"
    } else if mbps >= 100.0 {
        "plenty for 4K video and large downloads"
    } else if mbps >= 25.0 {
        "fine for HD streaming and video calls"
    } else if mbps >= 5.0 {
        "fine for browsing and email; HD video may buffer"
    } else if mbps > 0.0 {
        "slow — expect buffering"
    } else {
        ""
    }
}

fn band_words(b: &str) -> &'static str {
    match b {
        "2.4 GHz" => "longer range, slower, more crowded",
        "5 GHz" => "faster, shorter range",
        "6 GHz" => "fastest, newest, shortest range",
        _ => "",
    }
}

fn security_words(s: &str) -> (String, Color) {
    let good = Color::Rgb(120, 200, 120);
    let warn = Color::Rgb(230, 190, 90);
    let bad = Color::Rgb(240, 100, 100);
    if s.is_empty() {
        ("security unknown".into(), Color::Gray)
    } else if s == "Open" {
        ("OPEN — no password; anyone nearby can read unencrypted traffic".into(), bad)
    } else if s == "WEP" {
        ("WEP — obsolete, easily cracked".into(), bad)
    } else if s.contains("WPA3") {
        (format!("{} — modern, password protected", s), good)
    } else if s.contains("WPA2") {
        (format!("{} — password protected", s), good)
    } else if s.starts_with("WPA") {
        (format!("{} — old but password protected", s), warn)
    } else {
        (s.to_string(), good)
    }
}

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let w = &s.wifi;
    let Some(link) = &w.link else {
        let b = section("wi-fi", th);
        let msg = if w.iw_available {
            "No wireless interface found on this machine."
        } else {
            "No wireless interface found, and `iw` is not installed."
        };
        frame.render_widget(Paragraph::new(Line::from(fg(msg, th.warn))).block(b), area);
        return;
    };
    let tall = area.height >= 44;
    let rows = Layout::vertical([
        Constraint::Length(8),
        Constraint::Length(if tall { 10 } else { 9 }),
        Constraint::Min(5),
        Constraint::Length(if tall { 13 } else { 11 }),
    ])
    .split(area);

    // ── 1. plain English ─────────────────────────────────────────────
    {
        let ifs = if w.ifaces.len() > 1 { format!("{} (+{} more)", link.iface, w.ifaces.len() - 1) } else { link.iface.clone() };
        let b = section_with_right("in plain English", &ifs, th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        let mut lines: Vec<Line> = Vec::new();
        if link.connected {
            let dbm = link.signal_dbm.unwrap_or(-100);
            let (q, bars, color, meaning) = quality(dbm);
            lines.push(Line::from(vec![fg("✔ ", th.good), fg("Connected to ", Color::White), bold(format!("“{}”", link.ssid), th.accent)]));
            lines.push(Line::from(vec![
                fg("Signal    ", th.label),
                fg(signal_bars(bars), color),
                Span::raw(" "),
                bold(q, color),
                dim(format!("  — {}", meaning)),
            ]));
            let best = link.tx_mbps.max(link.rx_mbps);
            lines.push(Line::from(vec![
                fg("Speed     ", th.label),
                fg(format!("up to {}", bits(best * 1e6)), Color::White),
                dim(format!(" between you and the router — {}", speed_words(best))),
            ]));
            lines.push(Line::from(vec![
                fg("Band      ", th.label),
                fg(link.band(), Color::White),
                dim(format!(" ({}) on channel {}", band_words(link.band()), link.chan)),
            ]));
            let (sec, sc) = security_words(&link.security);
            lines.push(Line::from(vec![fg("Security  ", th.label), fg(sec, sc)]));
            let ip = s.net.ifaces.iter().find(|i| i.name == link.iface).and_then(|i| i.ipv4.first().cloned());
            let mut tip: Vec<Span> = vec![fg("Tip       ", th.label)];
            if link.security == "Open" {
                tip.push(fg("Avoid banking or passwords on this network unless the site shows a padlock.", th.warn));
            } else if dbm < -70 {
                tip.push(fg("Weak signal: move closer to the router, or try the 2.4 GHz network if there is one.", th.warn));
            } else if link.band() == "2.4 GHz" && w.scan.iter().filter(|b| b.chan == link.chan).count() > 3 {
                tip.push(fg("Your channel is crowded. If your router offers a 5 GHz network, it will usually be faster.", th.warn));
            } else if ip.is_none() {
                tip.push(fg("Connected to the router but no internet address yet — give it a few seconds or reconnect.", th.warn));
            } else if link.tx_failed.unwrap_or(0) > 100
                && link.tx_packets.unwrap_or(1) > 0
                && link.tx_failed.unwrap_or(0) as f64 / link.tx_packets.unwrap_or(1) as f64 > 0.01
            {
                tip.push(fg("Many packets are failing to send — interference or a flaky access point.", th.warn));
            } else {
                tip.push(fg(
                    format!("Everything looks healthy. Your address on this network is {}.", ip.unwrap_or_else(|| "pending".into())),
                    th.good,
                ));
            }
            lines.push(Line::from(tip));
        } else {
            lines.push(Line::from(vec![fg("✘ ", th.bad), bold("Not connected to Wi-Fi", th.bad)]));
            lines.push(Line::from(dim(format!(
                "{} network{} visible nearby. Pick one from your system's Wi-Fi menu to connect.",
                w.scan.len(),
                if w.scan.len() == 1 { " is" } else { "s are" }
            ))));
            if let Some(best) = w
                .scan
                .iter()
                .filter(|b| !b.ssid.is_empty())
                .max_by(|a, b| a.signal.partial_cmp(&b.signal).unwrap_or(std::cmp::Ordering::Equal))
            {
                lines.push(Line::from(vec![
                    fg("Strongest ", th.label),
                    fg(format!("“{}” ({}, {:.0} dBm)", best.ssid, best.security, best.signal), Color::White),
                ]));
            }
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    // ── 2. connection details ────────────────────────────────────────
    {
        let b = section("connection", th);
        let inner = b.inner(rows[1]);
        frame.render_widget(b, rows[1]);
        let cols = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(inner);
        let assoc = w.scan.iter().find(|b| b.associated);
        let gen = assoc.map(|b| b.gen.clone()).unwrap_or_else(|| "—".into());
        let width = if link.width.is_empty() { assoc.map(|b| b.width.clone()).unwrap_or_default() } else { link.width.clone() };
        let left = vec![
            ("SSID", if link.connected { link.ssid.clone() } else { "—".into() }),
            (
                "Access point",
                format!(
                    "{}{}",
                    link.bssid,
                    assoc.and_then(|b| b.country.clone()).map(|c| format!("  (country {})", c)).unwrap_or_default()
                ),
            ),
            (
                "Channel",
                if link.freq > 0 {
                    format!(
                        "{} @ {} MHz · {} wide{}",
                        link.chan,
                        link.freq,
                        width,
                        link.center1.map(|c| format!(" · center {} MHz", c)).unwrap_or_default()
                    )
                } else {
                    "—".into()
                },
            ),
            ("Standard", format!("{}  ({})", gen, link.band())),
            ("Security", if link.security.is_empty() { "—".into() } else { link.security.clone() }),
            ("Connected for", opt(link.connected_secs.map(dur), "")),
            ("Gateway", s.net.default_v4.as_ref().filter(|(i, _)| i == &link.iface).map(|(_, g)| g.clone()).unwrap_or_else(|| "—".into())),
            (
                "Power save",
                match link.power_save {
                    Some(true) => "on".into(),
                    Some(false) => "off".into(),
                    None => "—".into(),
                },
            ),
        ];
        frame.render_widget(kv_table(left, th, 15), cols[0]);
        let sig = match (link.signal_dbm, link.signal_avg) {
            (Some(a), Some(b)) => format!(
                "{} dBm  (avg {} dBm){}",
                a,
                b,
                if link.signal_chains.len() > 1 { format!("  chains {:?}", link.signal_chains) } else { String::new() }
            ),
            (Some(a), None) => format!("{} dBm", a),
            _ => "—".into(),
        };
        let noise = match (link.noise_dbm, link.snr()) {
            (Some(n), Some(snr)) => format!("{} dBm  → SNR {} dB", n, snr),
            _ => "not reported by driver".into(),
        };
        let right = vec![
            ("Signal", sig),
            ("Noise floor", noise),
            ("Link quality", link.link_quality.map(|(q, m)| format!("{}/{}", q, m)).unwrap_or_else(|| "—".into())),
            ("TX rate", if link.tx_bitrate.is_empty() { "—".into() } else { link.tx_bitrate.clone() }),
            ("RX rate", if link.rx_bitrate.is_empty() { "—".into() } else { link.rx_bitrate.clone() }),
            ("Expected tput", optf(link.expected_mbps, 1, " Mbit/s")),
            ("TX power", optf(link.txpower_dbm, 1, " dBm")),
            (
                "BSS load",
                assoc
                    .map(|b| {
                        format!(
                            "{} clients · {}% airtime busy",
                            b.stations.map(|s| s.to_string()).unwrap_or_else(|| "?".into()),
                            b.util_pct.map(|u| format!("{:.0}", u)).unwrap_or_else(|| "?".into())
                        )
                    })
                    .unwrap_or_else(|| "—".into()),
            ),
        ];
        frame.render_widget(kv_table(right, th, 15), cols[1]);
    }

    // ── 3. nearby networks ───────────────────────────────────────────
    {
        let scan: Vec<&crate::collect::wifi::Bss> =
            w.scan.iter().filter(|b| app.cfg.show_hidden_networks || !b.ssid.is_empty() || b.associated).collect();
        let n = scan.len();
        let age = w
            .scan_age_s
            .map(|a| format!("{} network{} · cache {:.0}s old · ↑↓ to browse", n, if n == 1 { "" } else { "s" }, a))
            .unwrap_or_else(|| "no scan results (needs `iw`)".into());
        let b = section_with_right("nearby networks", &age, th);
        let hdr =
            Row::new(vec!["", "SSID", "BSSID", "CH", "BAND", "WIDTH", "SIGNAL", "", "SECURITY", "STANDARD", "CLIENTS", "AIRTIME", "SEEN"])
                .style(Style::new().fg(th.muted));
        let trs: Vec<Row> = scan
            .iter()
            .map(|b| {
                let (q, bars, color, _) = quality(b.signal.round() as i32);
                let ssid = if b.ssid.is_empty() { "⟨hidden⟩".to_string() } else { b.ssid.clone() };
                let sec_color = if b.security == "Open" || b.security == "WEP" {
                    th.bad
                } else if b.security.contains("WPA3") {
                    th.good
                } else {
                    Color::Gray
                };
                Row::new(vec![
                    Cell::from(Span::styled(if b.associated { "★" } else { " " }, Style::new().fg(th.accent))),
                    Cell::from(Span::styled(
                        ssid,
                        if b.associated { Style::new().fg(th.accent).bold() } else { Style::new().fg(Color::White) },
                    )),
                    Cell::from(Span::styled(b.bssid.clone(), Style::new().fg(th.muted))),
                    Cell::from(format!("{}", b.chan)),
                    Cell::from(crate::collect::wifi::band_of(b.freq)),
                    Cell::from(b.width.clone()),
                    Cell::from(Span::styled(format!("{:>4.0} dBm", b.signal), Style::new().fg(color))),
                    Cell::from(Span::styled(format!("{} {}", signal_bars(bars), q), Style::new().fg(color))),
                    Cell::from(Span::styled(b.security.clone(), Style::new().fg(sec_color))),
                    Cell::from(b.gen.clone()),
                    Cell::from(b.stations.map(|s| s.to_string()).unwrap_or_else(|| "—".into())),
                    Cell::from(b.util_pct.map(|u| format!("{:.0}%", u)).unwrap_or_else(|| "—".into())),
                    Cell::from(Span::styled(
                        b.last_seen_ms.map(|m| dur(m / 1000)).unwrap_or_else(|| "—".into()),
                        Style::new().fg(th.muted),
                    )),
                ])
            })
            .collect();
        let t = Table::new(
            trs,
            [
                Constraint::Length(1),
                Constraint::Min(14),
                Constraint::Length(17),
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(7),
                Constraint::Length(8),
                Constraint::Length(14),
                Constraint::Length(20),
                Constraint::Length(10),
                Constraint::Length(7),
                Constraint::Length(7),
                Constraint::Length(7),
            ],
        )
        .header(hdr)
        .block(b)
        .row_highlight_style(Style::new().bg(Color::Rgb(40, 50, 60)))
        .highlight_symbol("▶");
        if !scan.is_empty() {
            app.wifi_sel = app.wifi_sel.min(scan.len() - 1);
        }
        let mut st = TableState::default().with_selected(if scan.is_empty() { None } else { Some(app.wifi_sel) });
        frame.render_stateful_widget(t, rows[2], &mut st);
        app.hit.wifi_rows =
            Some(Rect::new(rows[2].x + 1, rows[2].y + 2, rows[2].width.saturating_sub(2), rows[2].height.saturating_sub(3)));
        app.hit.wifi_offset = st.offset();
    }

    // ── 4. for the wireless engineer ─────────────────────────────────
    {
        let b = section_with_right("RF engineering", "nl80211 station · PHY · counters", th);
        let inner = b.inner(rows[3]);
        frame.render_widget(b, rows[3]);
        let cols = Layout::horizontal([
            Constraint::Percentage(22),
            Constraint::Percentage(22),
            Constraint::Percentage(34),
            Constraint::Percentage(22),
        ])
        .split(inner);
        let yn = |b: bool| if b { "yes" } else { "no" }.to_string();
        let sta = vec![
            ("authorized", yn(link.authorized)),
            ("authenticated", yn(link.authenticated)),
            ("associated", yn(link.associated)),
            ("WMM/QoS", yn(link.wmm)),
            ("MFP (802.11w)", yn(link.mfp)),
            ("mode", if link.mode.is_empty() { "—".into() } else { link.mode.clone() }),
            ("DTIM period", opt(link.dtim, "")),
            ("beacon interval", opt(link.beacon_int, " TU")),
            ("inactive", opt(link.inactive_ms, " ms")),
            ("last ack signal", opt(link.last_ack_signal, " dBm")),
            ("avg ack signal", opt(link.avg_ack_signal, " dBm")),
        ];
        frame.render_widget(kv_table(sta, th, 16), cols[0]);
        let pct = |a: Option<u64>, b: Option<u64>| match (a, b) {
            (Some(a), Some(b)) if b > 0 => format!("{}  ({:.2}%)", a, a as f64 / b as f64 * 100.0),
            (Some(a), _) => a.to_string(),
            _ => "—".into(),
        };
        let ctr = vec![
            ("tx packets", opt(link.tx_packets, "")),
            ("rx packets", opt(link.rx_packets, "")),
            ("tx bytes", link.tx_bytes.map(bytes).unwrap_or_else(|| "—".into())),
            ("rx bytes", link.rx_bytes.map(bytes).unwrap_or_else(|| "—".into())),
            ("tx failed", pct(link.tx_failed, link.tx_packets)),
            ("tx retries", pct(link.tx_retries, link.tx_packets)),
            ("rx drop misc", opt(link.rx_drop_misc, "")),
            ("beacon loss", opt(link.beacon_loss, "")),
            ("beacons rx", opt(link.beacon_rx, "")),
            ("retry discards", opt(link.disc_retry, "")),
            ("misc discards", opt(link.disc_misc, "")),
            ("missed beacons", opt(link.missed_beacon, "")),
        ];
        frame.render_widget(kv_table(ctr, th, 15), cols[1]);
        let mut radio: Vec<(&str, String)> = vec![
            ("interface", format!("{} · {}", link.iface, link.mac)),
            ("driver / phy", format!("{} · {}", link.driver.clone().unwrap_or_else(|| "—".into()), link.phy)),
            ("regulatory", w.regdom.clone().unwrap_or_else(|| "—".into())),
        ];
        if let Some(p) = &w.phy {
            let mut gens = Vec::new();
            if p.ht {
                gens.push("HT");
            }
            if p.vht {
                gens.push("VHT");
            }
            if p.he {
                gens.push("HE");
            }
            if p.eht {
                gens.push("EHT");
            }
            radio.push(("bands", if p.bands.is_empty() { "—".into() } else { p.bands.join(" · ") }));
            radio.push(("PHY support", format!("{}  ({} channels)", gens.join("/"), p.channels)));
            radio.push(("antennas", p.antennas.clone().unwrap_or_else(|| "—".into())));
            radio.push(("ciphers / modes", format!("{} ciphers · {} modes", p.ciphers, p.modes.len())));
            radio.push(("max scan SSIDs", opt(p.max_scan_ssids, "")));
        }
        let busiest: Vec<String> = w.chan_load.iter().take(4).map(|(c, n)| format!("ch{}×{}", c, n)).collect();
        radio.push(("busiest channels", if busiest.is_empty() { "—".into() } else { busiest.join("  ") }));
        let co = w.scan.iter().filter(|b| b.chan == link.chan && !b.associated).count();
        radio.push((
            "co-channel BSS",
            if link.connected { format!("{} other AP{} on ch {}", co, if co == 1 { "" } else { "s" }, link.chan) } else { "—".into() },
        ));
        frame.render_widget(kv_table(radio, th, 17), cols[2]);

        let r = Layout::vertical([Constraint::Length(1), Constraint::Min(2), Constraint::Length(1), Constraint::Min(2)]).split(cols[3]);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg("signal ", th.label),
                dim(format!("{} dBm", link.signal_dbm.map(|v| v.to_string()).unwrap_or_else(|| "—".into()))),
            ])),
            r[0],
        );
        frame.render_widget(spark(&tail(&app.hist.wifi_sig, r[1].width as usize), Some(80), th.accent), r[1]);
        frame.render_widget(Paragraph::new(Line::from(vec![fg("tx rate ", th.label), dim(bits(link.tx_mbps * 1e6))])), r[2]);
        frame.render_widget(spark(&tail(&app.hist.wifi_tx, r[3].width as usize), None, th.good), r[3]);
    }
}
