use crate::app::{App, ProcSort};
use crate::ui::widgets::*;
use crate::util::*;
use ratatui::prelude::*;
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};

pub fn draw(frame: &mut Frame, area: Rect, app: &mut App, th: &Theme) {
    let s = app.snap.as_ref().unwrap();
    let p = &s.procs;
    let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(4)]).split(area);

    {
        let b = section("processes", th);
        let inner = b.inner(rows[0]);
        frame.render_widget(b, rows[0]);
        let sort_name = match app.proc_sort {
            ProcSort::Cpu => "cpu",
            ProcSort::Mem => "memory",
            ProcSort::Pid => "pid",
            ProcSort::Name => "name",
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                fg(format!("{} processes", p.total), Color::White),
                dim(" · "),
                fg(format!("{} threads", p.threads), Color::White),
                dim(" · "),
                fg(format!("{} running", p.running), th.good),
                dim(" · "),
                fg(format!("{} sleeping", p.sleeping), Color::Gray),
                dim(" · "),
                fg(format!("{} stopped", p.stopped), if p.stopped > 0 { th.warn } else { Color::Gray }),
                dim(" · "),
                fg(format!("{} zombie", p.zombie), if p.zombie > 0 { th.warn } else { Color::Gray }),
                dim("      sorted by "),
                bold(sort_name, th.accent),
                dim("   (c)pu (m)emory (p)id (n)ame"),
            ])),
            inner,
        );
    }

    let mut list: Vec<_> = p.procs.iter().collect();
    match app.proc_sort {
        ProcSort::Cpu => {
            list.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal).then(b.rss.cmp(&a.rss)))
        }
        ProcSort::Mem => list.sort_by_key(|a| std::cmp::Reverse(a.rss)),
        ProcSort::Pid => list.sort_by_key(|a| a.pid),
        ProcSort::Name => list.sort_by_key(|a| a.name.to_lowercase()),
    }
    if !list.is_empty() {
        app.proc_sel = app.proc_sel.min(list.len() - 1);
    }
    let b = section_with_right("table", "↑↓ select · Home top", th);
    let hdr = Row::new(vec!["PID", "USER", "S", "CPU%", "MEM%", "RSS", "THR", "NI", "CPU TIME", "AGE", "COMMAND"])
        .style(Style::new().fg(th.muted));
    let visible = rows[1].height.saturating_sub(3) as usize;
    let start = app.proc_sel.saturating_sub(visible.saturating_sub(1));
    let trs: Vec<Row> = list
        .iter()
        .skip(start)
        .take(visible + 1)
        .map(|x| {
            let st_color = match x.state {
                'R' => th.good,
                'D' => th.warn,
                'Z' | 'T' | 't' => th.bad,
                _ => th.muted,
            };
            Row::new(vec![
                Cell::from(format!("{}", x.pid)),
                Cell::from(Span::styled(x.user.clone(), Style::new().fg(th.label))),
                Cell::from(Span::styled(x.state.to_string(), Style::new().fg(st_color))),
                Cell::from(Span::styled(
                    format!("{:>5.1}", x.cpu_pct),
                    Style::new().fg(if x.cpu_pct > 0.5 { th.level(x.cpu_pct) } else { th.muted }),
                )),
                Cell::from(format!("{:>4.1}", x.mem_pct)),
                Cell::from(bytes(x.rss)),
                Cell::from(format!("{}", x.threads)),
                Cell::from(format!("{}", x.nice)),
                Cell::from(dur(x.cpu_time_s as u64)),
                Cell::from(Span::styled(dur(x.age_s), Style::new().fg(th.muted))),
                Cell::from(Span::styled(x.cmd.clone(), Style::new().fg(Color::White))),
            ])
        })
        .collect();
    let t = Table::new(
        trs,
        [
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(1),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Length(9),
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Length(7),
            Constraint::Min(10),
        ],
    )
    .header(hdr)
    .block(b)
    .row_highlight_style(Style::new().bg(Color::Rgb(45, 45, 55)))
    .highlight_symbol("▶");
    let mut st = TableState::default().with_selected(if list.is_empty() { None } else { Some(app.proc_sel - start) });
    frame.render_stateful_widget(t, rows[1], &mut st);
    // Hit areas: header row sits just inside the top border, data rows below it.
    app.hit.proc_header = Some(Rect::new(rows[1].x + 1, rows[1].y + 1, rows[1].width.saturating_sub(2), 1));
    app.hit.proc_rows = Some(Rect::new(rows[1].x + 1, rows[1].y + 2, rows[1].width.saturating_sub(2), rows[1].height.saturating_sub(3)));
    app.hit.proc_start = start;
}

/// Map an x offset within the header row to a sort key. Offsets mirror the column widths above
/// plus one cell for the highlight symbol and one cell of spacing between columns.
pub fn header_sort(x: u16) -> Option<ProcSort> {
    let widths: [(u16, Option<ProcSort>); 11] = [
        (7, Some(ProcSort::Pid)),
        (11, None),
        (1, None),
        (5, Some(ProcSort::Cpu)),
        (4, Some(ProcSort::Mem)),
        (9, Some(ProcSort::Mem)),
        (4, None),
        (3, None),
        (8, None),
        (7, None),
        (u16::MAX, Some(ProcSort::Name)),
    ];
    let mut cursor = 1u16; // highlight symbol column
    for (w, sort) in widths {
        if x < cursor {
            return None;
        }
        if x < cursor.saturating_add(w) {
            return sort;
        }
        cursor = cursor.saturating_add(w).saturating_add(1);
    }
    None
}
