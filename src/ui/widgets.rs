//! Theme and reusable widget builders.

use crate::app::View;
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Cell, LineGauge, Paragraph, Row, Sparkline, Table};

#[derive(Clone, Copy)]
pub struct Theme {
    pub accent: Color,
    pub border: Color,
    pub label: Color,
    pub muted: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
}

impl Theme {
    pub fn for_view(v: View) -> Self {
        let accent = match v {
            View::Overview => Color::Rgb(94, 196, 196),
            View::Cpu => Color::Rgb(199, 125, 255),
            View::Mem => Color::Rgb(120, 200, 120),
            View::Disk => Color::Rgb(230, 190, 90),
            View::Net => Color::Rgb(100, 160, 255),
            View::Wifi => Color::Rgb(90, 200, 240),
            View::Power => Color::Rgb(150, 220, 120),
            View::Thermal => Color::Rgb(255, 130, 110),
            View::Procs => Color::Rgb(220, 220, 220),
        };
        Theme {
            accent,
            border: Color::DarkGray,
            label: Color::Gray,
            muted: Color::DarkGray,
            good: Color::Rgb(120, 200, 120),
            warn: Color::Rgb(230, 190, 90),
            bad: Color::Rgb(240, 100, 100),
        }
    }

    /// Green→yellow→red for a 0–100 utilisation value.
    pub fn level(&self, pct: f64) -> Color {
        if pct < 60.0 {
            self.good
        } else if pct < 85.0 {
            self.warn
        } else {
            self.bad
        }
    }
    /// Inverse: high is good (battery, health).
    pub fn level_inv(&self, pct: f64) -> Color {
        if pct > 50.0 {
            self.good
        } else if pct > 20.0 {
            self.warn
        } else {
            self.bad
        }
    }
}

pub fn section(title: &str, th: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(th.border))
        .title(Line::from(vec![Span::raw(" "), Span::styled(title.to_string(), Style::new().fg(th.accent).bold()), Span::raw(" ")]))
}

pub fn section_with_right(title: &str, right: &str, th: &Theme) -> Block<'static> {
    section(title, th).title_top(
        Line::from(vec![Span::raw(" "), Span::styled(right.to_string(), Style::new().fg(th.muted)), Span::raw(" ")]).right_aligned(),
    )
}

/// Label / value rows rendered as a compact two-column table.
pub fn kv_table<'a>(rows: Vec<(&'a str, String)>, th: &Theme, label_w: u16) -> Table<'a> {
    let rows: Vec<Row> =
        rows.into_iter().map(|(k, v)| Row::new(vec![Cell::from(Span::styled(k, Style::new().fg(th.label))), Cell::from(v)])).collect();
    Table::new(rows, [Constraint::Length(label_w), Constraint::Min(0)]).column_spacing(1)
}

pub fn gauge<'a>(ratio: f64, label: String, color: Color, th: &Theme) -> LineGauge<'a> {
    LineGauge::default()
        .ratio(ratio.clamp(0.0, 1.0))
        .filled_style(Style::new().fg(color))
        .unfilled_style(Style::new().fg(th.muted))
        .line_set(symbols::line::THICK)
        .label(Span::styled(label, Style::new().fg(Color::White)))
}

pub fn spark<'a>(data: &'a [u64], max: Option<u64>, color: Color) -> Sparkline<'a> {
    let mut s = Sparkline::default().data(data).style(Style::new().fg(color));
    if let Some(m) = max {
        s = s.max(m);
    }
    s
}

pub fn dim(s: impl Into<String>) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(Color::DarkGray))
}

pub fn fg(s: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(c))
}

pub fn bold(s: impl Into<String>, c: Color) -> Span<'static> {
    Span::styled(s.into(), Style::new().fg(c).bold())
}

pub fn opt<T: std::fmt::Display>(v: Option<T>, unit: &str) -> String {
    match v {
        Some(x) => format!("{}{}", x, unit),
        None => "—".into(),
    }
}

pub fn optf(v: Option<f64>, prec: usize, unit: &str) -> String {
    match v {
        Some(x) => format!("{:.*}{}", prec, x, unit),
        None => "—".into(),
    }
}

/// Render a vertical list of (label, ratio, text) gauges inside `area`, one row each.
pub fn gauge_rows(frame: &mut Frame, area: Rect, rows: &[(String, f64, String, Color)], th: &Theme, label_w: u16) {
    let n = rows.len().min(area.height as usize);
    if n == 0 {
        return;
    }
    let chunks = Layout::vertical(vec![Constraint::Length(1); n]).split(area);
    for (i, (label, ratio, text, color)) in rows.iter().take(n).enumerate() {
        let cols = Layout::horizontal([Constraint::Length(label_w), Constraint::Min(8)]).split(chunks[i]);
        frame.render_widget(Paragraph::new(Span::styled(label.clone(), Style::new().fg(th.label))), cols[0]);
        frame.render_widget(gauge(*ratio, text.clone(), *color, th), cols[1]);
    }
}
