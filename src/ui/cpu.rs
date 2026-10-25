use crate::app::{tail, App};
use crate::ui::widgets::*;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let c = &s.cpu;
    let n = c.per_core.len().max(1);
    let ncols = ((n as f64) / 16.0).ceil().clamp(1.0, 4.0) as usize;
    let core_rows = n.div_ceil(ncols) as u16 + 2;
    let rows = Layout::vertical([Constraint::Length(8), Constraint::Length(core_rows), Constraint::Min(9)]).split(area);

    // ── summary ──────────────────────────────────────────────────────
    {
        let b = section_with_right("processor", &app.host.cpu_model, th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        let r = Layout::vertical([Constraint::Length(1), Constraint::Length(3), Constraint::Length(1), Constraint::Length(1)]).split(inner);
        gauge_rows(frame, r[0], &[("Total".into(), c.total_pct / 100.0, format!("{:>5.1}%", c.total_pct), th.level(c.total_pct))], th, 7);
        frame.render_widget(spark(&tail(&app.hist.cpu, r[1].width as usize), Some(100), th.accent), r[1]);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg("Split  ", th.label),
                fg(format!("user {:.1}%", c.user_pct), Color::White),
                dim(" · "),
                fg(format!("system {:.1}%", c.system_pct), Color::White),
                dim(" · "),
                fg(format!("iowait {:.1}%", c.iowait_pct), if c.iowait_pct > 10.0 { th.warn } else { Color::White }),
                dim(" · "),
                fg(format!("irq {:.1}%", c.irq_pct), Color::White),
                dim(" · "),
                fg(format!("steal {:.1}%", c.steal_pct), if c.steal_pct > 1.0 { th.warn } else { Color::White }),
            ])),
            r[2],
        );
        let (l1, l5, l15) = c.load;
        let n = c.per_core.len().max(1) as f64;
        let plain = if l1 / n < 0.7 {
            "relaxed"
        } else if l1 / n < 1.0 {
            "busy"
        } else {
            "saturated — tasks are queueing"
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg("Load   ", th.label),
                fg(format!("{:.2}  {:.2}  {:.2}", l1, l5, l15), Color::White),
                dim(format!("  (1 · 5 · 15 min, {} cores → ", c.per_core.len())),
                fg(plain, th.level(l1 / n * 100.0)),
                dim(")"),
            ])),
            r[3],
        );
    }

    // ── per core ─────────────────────────────────────────────────────
    {
        let b = section_with_right("cores", &format!("{} logical", c.per_core.len()), th);
        let inner = b.inner(rows[1]);
        frame.render_widget(b, rows[1]);
        let per_col = n.div_ceil(ncols);
        let cols = Layout::horizontal(vec![Constraint::Ratio(1, ncols as u32); ncols]).split(inner);
        for ci in 0..ncols {
            let items: Vec<(String, f64, String, Color)> = (ci * per_col..((ci + 1) * per_col).min(n))
                .map(|i| {
                    let pct = c.per_core[i];
                    let f = c.freq_mhz.get(i).copied().flatten();
                    let mx = c.max_mhz.get(i).copied().flatten();
                    let ftxt = match (f, mx) {
                        (Some(f), Some(m)) => format!("{:>4.2} GHz /{:.1}", f as f64 / 1000.0, m as f64 / 1000.0),
                        (Some(f), None) => format!("{:>4.2} GHz", f as f64 / 1000.0),
                        _ => String::new(),
                    };
                    (format!("cpu{:<2}", i), pct / 100.0, format!("{:>5.1}%  {}", pct, ftxt), th.level(pct))
                })
                .collect();
            gauge_rows(frame, cols[ci], &items, th, 6);
        }
    }

    // ── details ──────────────────────────────────────────────────────
    {
        let b = section("scheduler & frequency scaling", th);
        let inner = b.inner(rows[2]);
        frame.render_widget(b, rows[2]);
        let cols = Layout::horizontal([Constraint::Ratio(1, 3), Constraint::Ratio(1, 3), Constraint::Ratio(1, 3)]).split(inner);
        let left = vec![
            ("context sw/s", format!("{:.0}", c.ctxt_per_s)),
            ("interrupts/s", format!("{:.0}", c.intr_per_s)),
            ("forks/s", format!("{:.1}", c.forks_per_s)),
            ("running", format!("{}", c.running)),
            ("blocked (D)", format!("{}", c.blocked)),
            ("threads", format!("{}", c.threads_total)),
            ("forks since boot", format!("{}", c.forks_total)),
        ];
        frame.render_widget(kv_table(left, th, 17), cols[0]);
        let freqs: Vec<u32> = c.freq_mhz.iter().flatten().copied().collect();
        let (fmin, fmax, favg) = if freqs.is_empty() {
            (0, 0, 0.0)
        } else {
            (*freqs.iter().min().unwrap(), *freqs.iter().max().unwrap(), freqs.iter().sum::<u32>() as f64 / freqs.len() as f64)
        };
        let hw_min = c.min_mhz.iter().flatten().min().copied();
        let hw_max = c.max_mhz.iter().flatten().max().copied();
        let mid = vec![
            ("governor", c.governor.clone().unwrap_or_else(|| "—".into())),
            ("driver", c.driver.clone().unwrap_or_else(|| "—".into())),
            ("energy pref", c.epp.clone().unwrap_or_else(|| "—".into())),
            (
                "clock now",
                if freqs.is_empty() {
                    "—".into()
                } else {
                    format!("{:.2} avg · {:.2}–{:.2} GHz", favg / 1000.0, fmin as f64 / 1000.0, fmax as f64 / 1000.0)
                },
            ),
            (
                "clock range",
                match (hw_min, hw_max) {
                    (Some(a), Some(b)) => format!("{:.2} – {:.2} GHz", a as f64 / 1000.0, b as f64 / 1000.0),
                    _ => "—".into(),
                },
            ),
            ("architecture", app.host.arch.clone()),
        ];
        frame.render_widget(kv_table(mid, th, 13), cols[1]);
        let right = match &c.psi {
            Some(p) => vec![
                ("pressure (PSI)", "share of time tasks stalled for CPU".to_string()),
                ("some 10s", format!("{:.2}%", p.some10)),
                ("some 60s", format!("{:.2}%", p.some60)),
                ("some 300s", format!("{:.2}%", p.some300)),
                ("full 10s", format!("{:.2}%", p.full10)),
                ("full 60s", format!("{:.2}%", p.full60)),
            ],
            None => vec![("pressure (PSI)", "not available (CONFIG_PSI)".to_string())],
        };
        frame.render_widget(kv_table(right, th, 15), cols[2]);
    }
}
