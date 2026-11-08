//! Pieces shared by several apps: page background, title bar, buttons.

use super::home::background;
use crate::gfx::color::*;
use crate::gfx::math::fx;
use crate::gfx::{font, Canvas, Color, Rect};

pub const HEADER_H: i32 = 110;

/// Background tinted with the app's colour (matches the opening animation).
pub fn app_background(c: &mut Canvas, color: Color) {
    background(c, mix(color, WHITE, 150), hex(0xF1F4FA));
}

pub fn back_button() -> Rect { Rect::new(40, 26, 64, 64) }

/// Round "back" button + title.
pub fn header(c: &mut Canvas, title: &str, color: Color, hover_back: bool) {
    let b = back_button();
    if b.inflate(12).overlaps(&c.clip) {
        c.shadow(b, 32, 6, 50);
        c.fill_circle(b.x + 32, b.y + 32, 32, if hover_back { ACCENT } else { WHITE });
        let col = if hover_back { WHITE } else { INK };
        // a "<" arrow made of two thick lines
        c.line_fx(fx(b.x + 38), fx(b.y + 18), fx(b.x + 24), fx(b.y + 32), fx(7), col);
        c.line_fx(fx(b.x + 24), fx(b.y + 32), fx(b.x + 38), fx(b.y + 46), fx(7), col);
    }
    c.fill_circle(140, 58, 12, mix(color, INK, 40));
    c.text(&font::LARGE, 166, 42, title, INK);
}

/// A rounded button with a label. `active` = highlighted.
pub fn button(c: &mut Canvas, r: Rect, label: &str, active: bool) {
    if !r.inflate(10).overlaps(&c.clip) { return; }
    c.shadow(r, r.h / 2, 5, 40);
    c.fill_round_rect(r, r.h / 2, if active { ACCENT } else { WHITE });
    c.text_centered(&font::MEDIUM, r.x + r.w / 2, r.y + (r.h - 24) / 2, label, if active { WHITE } else { INK });
}
