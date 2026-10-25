//! Draws the sysfun mark with half-block cells: one text cell shows two vertically stacked pixels
//! (▀ with the top pixel as foreground and the bottom pixel as background).

use crate::ui::logo_data::Logo;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

#[allow(unused_imports)]
use crate::ui::logo_data as _logo_data;
pub use crate::ui::logo_data::{LOGO16, LOGO24};

/// Columns and rows the logo occupies.
pub fn cells(logo: &Logo) -> (u16, u16) {
    (logo.size as u16, logo.size.div_ceil(2) as u16)
}

fn pixel(logo: &Logo, x: usize, y: usize, mono: bool) -> Option<Color> {
    if y >= logo.size {
        return None;
    }
    let p = logo.px[y * logo.size + x];
    let (r, g, b, a) = ((p >> 24) as u8, (p >> 16) as u8, (p >> 8) as u8, p as u8);
    if a < 128 {
        return None;
    }
    if mono {
        let l = (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) as u8;
        Some(Color::Rgb(l, l, l))
    } else {
        Some(Color::Rgb(r, g, b))
    }
}

pub fn draw(frame: &mut Frame, at: Rect, logo: &Logo, mono: bool) {
    let (w, h) = cells(logo);
    let w = w.min(at.width);
    for ry in 0..h.min(at.height) {
        let mut spans = Vec::with_capacity(w as usize);
        for x in 0..w as usize {
            let top = pixel(logo, x, 2 * ry as usize, mono);
            let bot = pixel(logo, x, 2 * ry as usize + 1, mono);
            let (ch, style) = match (top, bot) {
                (None, None) => (" ", Style::default()),
                (Some(t), None) => ("▀", Style::default().fg(t)),
                (None, Some(b)) => ("▄", Style::default().fg(b)),
                (Some(t), Some(b)) => ("▀", Style::default().fg(t).bg(b)),
            };
            spans.push(Span::styled(ch, style));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), Rect::new(at.x, at.y + ry, w, 1));
    }
}
