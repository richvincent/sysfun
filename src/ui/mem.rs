use crate::app::{tail, App};
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let m = &s.mem;
    let rows = Layout::vertical([Constraint::Length(10), Constraint::Length(14), Constraint::Min(4)]).split(area);

    {
        let b = section_with_right("memory", &format!("{} installed", bytes(m.total)), th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        let r = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(inner);
        let used = m.used_pct();
        gauge_rows(
            frame,
            r[0],
            &[("In use".into(), used / 100.0, format!("{:>5.1}%  {} of {}", used, bytes(m.used), bytes(m.total)), th.level(used))],
            th,
            9,
        );
        let cache_pct = (m.cached + m.buffers) as f64 / m.total.max(1) as f64 * 100.0;
        gauge_rows(
            frame,
            r[1],
            &[(
                "Cache".into(),
                cache_pct / 100.0,
                format!("{:>5.1}%  {} — file cache, given back instantly when needed", cache_pct, bytes(m.cached + m.buffers)),
                th.accent,
            )],
            th,
            9,
        );
        gauge_rows(
            frame,
            r[2],
            &[(
                "Swap".into(),
                m.swap_pct() / 100.0,
                format!("{:>5.1}%  {} of {}", m.swap_pct(), bytes(m.swap_used), bytes(m.swap_total)),
                th.level(m.swap_pct()),
            )],
            th,
            9,
        );
        let plain = if used < 70.0 {
            "Plenty of memory free."
        } else if used < 90.0 {
            "Getting full — the kernel will start swapping if this keeps rising."
        } else {
            "Nearly out of memory — close something or expect slowdowns."
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg("Status   ", th.label),
                fg(format!("{} is available right now. {}", bytes(m.available), plain), th.level(used)),
            ])),
            r[3],
        );
        frame.render_widget(spark(&tail(&app.hist.mem, r[4].width as usize), Some(100), th.accent), r[4]);
        frame.render_widget(Paragraph::new(Line::from(dim("history of memory in use (%)"))), r[5]);
    }

    {
        let b = section("breakdown", th);
        let inner = b.inner(rows[1]);
        frame.render_widget(b, rows[1]);
        let cols = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)]).split(inner);
        let left = vec![
            ("total", bytes(m.total)),
            ("free (unused)", bytes(m.free)),
            ("available", bytes(m.available)),
            ("anonymous", bytes(m.anon)),
            ("file cache", bytes(m.cached)),
            ("buffers", bytes(m.buffers)),
            ("shared (shmem)", bytes(m.shared)),
            ("mapped", bytes(m.mapped)),
            ("active", bytes(m.active)),
            ("inactive", bytes(m.inactive)),
            ("dirty", bytes(m.dirty)),
            ("writeback", bytes(m.writeback)),
        ];
        frame.render_widget(kv_table(left, th, 15), cols[0]);
        let mid = vec![
            ("slab", bytes(m.slab)),
            ("  reclaimable", bytes(m.sreclaim)),
            ("  unreclaimable", bytes(m.sunreclaim)),
            ("kernel stacks", bytes(m.kernel_stack)),
            ("page tables", bytes(m.page_tables)),
            ("committed", bytes(m.committed)),
            ("commit limit", bytes(m.commit_limit)),
            (
                "overcommit",
                if m.commit_limit > 0 {
                    format!("{:.0}% of limit", m.committed as f64 / m.commit_limit as f64 * 100.0)
                } else {
                    "—".into()
                },
            ),
            (
                "huge pages",
                if m.hugepages_total > 0 { format!("{} × {}", m.hugepages_total, bytes(m.hugepage_size)) } else { "none".into() },
            ),
            ("swap cached", bytes(m.swap_cached)),
            ("zswap", if m.zswap > 0 { format!("{} holding {}", bytes(m.zswap), bytes(m.zswapped)) } else { "off".into() }),
        ];
        frame.render_widget(kv_table(mid, th, 16), cols[1]);
        let mut right: Vec<(&str, String)> = Vec::new();
        for z in &m.zram {
            let ratio = if z.compr > 0 { z.orig as f64 / z.compr as f64 } else { 0.0 };
            right.push(("zram device", format!("{} · {} · {}", z.dev, z.algo, bytes(z.disksize))));
            right.push(("  stored", format!("{} → {} ({:.1}:1)", bytes(z.orig), bytes(z.compr), ratio)));
            right.push(("  RAM used", bytes(z.mem_used)));
        }
        match &m.psi {
            Some(p) => {
                right.push(("pressure (PSI)", "time stalled waiting for memory".into()));
                right.push(("some 10/60/300s", format!("{:.2}% · {:.2}% · {:.2}%", p.some10, p.some60, p.some300)));
                right.push(("full 10/60/300s", format!("{:.2}% · {:.2}% · {:.2}%", p.full10, p.full60, p.full300)));
            }
            None => right.push(("pressure (PSI)", "not available".into())),
        }
        frame.render_widget(kv_table(right, th, 16), cols[2]);
    }

    {
        let b = section_with_right("biggest memory users", "resident set size", th);
        let mut procs: Vec<_> = s.procs.procs.iter().collect();
        procs.sort_by_key(|a| std::cmp::Reverse(a.rss));
        let hdr = Row::new(vec!["PID", "RSS", "MEM%", "", "USER", "COMMAND"]).style(Style::new().fg(th.muted));
        let trs: Vec<Row> = procs
            .iter()
            .take(rows[2].height.saturating_sub(3) as usize)
            .map(|p| {
                Row::new(vec![
                    Cell::from(format!("{}", p.pid)),
                    Cell::from(Span::styled(bytes(p.rss), Style::new().fg(Color::White))),
                    Cell::from(format!("{:>4.1}", p.mem_pct)),
                    Cell::from(Span::styled(bar(p.mem_pct / 25.0, 10), Style::new().fg(th.accent))),
                    Cell::from(Span::styled(p.user.clone(), Style::new().fg(th.label))),
                    Cell::from(p.cmd.clone()),
                ])
            })
            .collect();
        let t = Table::new(
            trs,
            [
                Constraint::Length(7),
                Constraint::Length(9),
                Constraint::Length(5),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Min(10),
            ],
        )
        .header(hdr)
        .block(b);
        frame.render_widget(t, rows[2]);
    }
}
