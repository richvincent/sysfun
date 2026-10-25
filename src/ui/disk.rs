use crate::app::{tail, App};
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let d = &s.disk;
    let fs_h = (d.mounts.len() as u16 + 3).clamp(4, 12);
    let dev_h = (d.devices.len() as u16 + 3).clamp(4, 10);
    let rows = Layout::vertical([Constraint::Length(fs_h), Constraint::Length(dev_h), Constraint::Min(6)]).split(area);

    // ── filesystems ──────────────────────────────────────────────────
    {
        let b = section_with_right("filesystems", "where your files live", th);
        let hdr = Row::new(vec!["MOUNT", "TYPE", "SIZE", "USED", "FREE", "USE", "", "OPTIONS"]).style(Style::new().fg(th.muted));
        let trs: Vec<Row> = d
            .mounts
            .iter()
            .map(|m| {
                let pct = m.used_pct();
                let mut name = m.mount.clone();
                if !m.extra_mounts.is_empty() {
                    name = format!("{}  +{}", name, m.extra_mounts.join(" "));
                }
                let stale = if m.stale { " (slow)" } else { "" };
                Row::new(vec![
                    Cell::from(Span::styled(name, Style::new().fg(Color::White))),
                    Cell::from(format!("{}{}", m.fstype, stale)),
                    Cell::from(bytes(m.total)),
                    Cell::from(bytes(m.used)),
                    Cell::from(Span::styled(bytes(m.avail), Style::new().fg(th.level(pct)))),
                    Cell::from(Span::styled(format!("{:>3.0}%", pct), Style::new().fg(th.level(pct)))),
                    Cell::from(Span::styled(bar(pct / 100.0, 12), Style::new().fg(th.level(pct)))),
                    Cell::from(Span::styled(format!("{}  {}", m.device, m.opts), Style::new().fg(th.muted))),
                ])
            })
            .collect();
        let t = Table::new(
            trs,
            [
                Constraint::Min(16),
                Constraint::Length(11),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(9),
                Constraint::Length(4),
                Constraint::Length(12),
                Constraint::Min(10),
            ],
        )
        .header(hdr)
        .block(b);
        frame.render_widget(t, rows[0]);
    }

    // ── block devices ────────────────────────────────────────────────
    {
        let b = section_with_right("drives", "live throughput", th);
        let hdr = Row::new(vec!["DEVICE", "MODEL", "BUS", "SIZE", "READ/s", "WRITE/s", "R IOPS", "W IOPS", "BUSY", "QUEUE", "SCHED"])
            .style(Style::new().fg(th.muted));
        let trs: Vec<Row> = d
            .devices
            .iter()
            .map(|x| {
                let name = match &x.mapper_name {
                    Some(m) => format!("{} ({})", x.name, m),
                    None => x.name.clone(),
                };
                let model =
                    x.model.clone().unwrap_or_else(|| if x.slaves.is_empty() { "—".into() } else { format!("on {}", x.slaves.join(",")) });
                Row::new(vec![
                    Cell::from(Span::styled(name, Style::new().fg(Color::White))),
                    Cell::from(model),
                    Cell::from(format!("{}{}", x.transport, if x.rotational { " HDD" } else { "" })),
                    Cell::from(bytes(x.size)),
                    Cell::from(Span::styled(rate(x.read_bps), Style::new().fg(if x.read_bps > 1e6 { th.good } else { Color::Gray }))),
                    Cell::from(Span::styled(rate(x.write_bps), Style::new().fg(if x.write_bps > 1e6 { th.warn } else { Color::Gray }))),
                    Cell::from(format!("{:.0}", x.read_iops)),
                    Cell::from(format!("{:.0}", x.write_iops)),
                    Cell::from(Span::styled(format!("{:>3.0}%", x.util_pct), Style::new().fg(th.level(x.util_pct)))),
                    Cell::from(format!("{}", x.in_flight)),
                    Cell::from(x.scheduler.clone().unwrap_or_else(|| "—".into())),
                ])
            })
            .collect();
        let t = Table::new(
            trs,
            [
                Constraint::Min(14),
                Constraint::Min(16),
                Constraint::Length(14),
                Constraint::Length(9),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(7),
                Constraint::Length(7),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(12),
            ],
        )
        .header(hdr)
        .block(b);
        frame.render_widget(t, rows[1]);
    }

    // ── detail: partitions + I/O history ─────────────────────────────
    {
        let cols = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(rows[2]);
        let primary = d.devices.iter().find(|x| x.slaves.is_empty() && !x.name.starts_with("zram"));
        let title = primary.map(|p| format!("{} layout", p.name)).unwrap_or_else(|| "layout".into());
        let b = section(&title, th);
        let inner = b.inner(cols[0]);
        frame.render_widget(b, cols[0]);
        if let Some(p) = primary {
            let mut lines: Vec<Line> = vec![Line::from(vec![
                fg(p.model.clone().unwrap_or_else(|| p.name.clone()), Color::White),
                dim(format!(
                    "  · serial {} · fw {} · {}{}",
                    p.serial.clone().unwrap_or_else(|| "—".into()),
                    p.firmware.clone().unwrap_or_else(|| "—".into()),
                    if p.discard { "TRIM " } else { "" },
                    if p.removable { "removable" } else { "" }
                )),
            ])];
            lines.push(Line::from(dim(format!("lifetime: {} read · {} written", bytes(p.total_read), bytes(p.total_written)))));
            for part in &p.parts {
                let m = part.mount.clone().unwrap_or_else(|| "not mounted".into());
                let label = part.label.clone().map(|l| format!("“{}”  ", l)).unwrap_or_default();
                let mapped = d
                    .devices
                    .iter()
                    .find(|x| x.slaves.contains(&part.name))
                    .map(|x| format!(" → {} ({})", x.name, x.mapper_name.clone().unwrap_or_default()));
                lines.push(Line::from(vec![
                    fg(format!("  {:<12}", part.name), th.label),
                    fg(format!("{:>9}  ", bytes(part.size)), Color::White),
                    fg(format!("{:<8}", part.fstype.clone().unwrap_or_else(|| "?".into())), th.accent),
                    Span::raw("  "),
                    fg(label, Color::Gray),
                    dim(m),
                    dim(mapped.unwrap_or_default()),
                ]));
            }
            frame.render_widget(Paragraph::new(lines), inner);
        }
        let b = section("I/O history", th);
        let inner = b.inner(cols[1]);
        frame.render_widget(b, cols[1]);
        let r =
            Layout::vertical([Constraint::Length(1), Constraint::Min(2), Constraint::Length(1), Constraint::Min(2), Constraint::Length(1)])
                .split(inner);
        frame.render_widget(Paragraph::new(Line::from(vec![fg("read  ", th.good), fg(rate(d.total_read_bps), Color::White)])), r[0]);
        frame.render_widget(spark(&tail(&app.hist.disk_r, r[1].width as usize), None, th.good), r[1]);
        frame.render_widget(Paragraph::new(Line::from(vec![fg("write ", th.warn), fg(rate(d.total_write_bps), Color::White)])), r[2]);
        frame.render_widget(spark(&tail(&app.hist.disk_w, r[3].width as usize), None, th.warn), r[3]);
        if let Some(p) = &d.psi {
            frame.render_widget(
                Paragraph::new(Line::from(dim(format!("I/O pressure some {:.2}% / full {:.2}% (10s)", p.some10, p.full10)))),
                r[4],
            );
        }
    }
}
