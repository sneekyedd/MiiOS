//! App icons, drawn from simple shapes (no image files), so they're
//! crisp at any size and cost nothing to store. Each icon is drawn
//! centred at (cx, cy) with size `s` (roughly its width in pixels).

use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::{Canvas, Rect};

fn r(cx: i32, cy: i32, w: i32, h: i32) -> Rect { Rect::new(cx - w / 2, cy - h / 2, w, h) }

/// Notebook with lines and a pencil.
pub fn write(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let page = r(cx - s / 10, cy, s * 6 / 10, s * 3 / 4);
    c.fill_round_rect(page.offset(0, s / 30), s / 12, with_alpha(0x303858, 40));
    c.fill_round_rect(page, s / 12, WHITE);
    for i in 0..4 {
        let y = page.y + page.h / 5 + i * page.h / 6;
        c.fill_round_rect(Rect::new(page.x + page.w / 6, y, page.w * 2 / 3 - (i % 2) * page.w / 5, s / 28 + 1), s / 56 + 1, hex(0xB8C2DD));
    }
    // pencil
    let (x0, y0) = (fx(cx + s / 3), fx(cy - s / 3));
    let (x1, y1) = (fx(cx + s / 12), fx(cy + s / 4));
    c.line_fx(x0, y0, x1, y1, fx(s / 9), hex(0xFFB547));
    c.line_fx(x1 + (x0 - x1) / 8, y1 + (y0 - y1) / 8, x1, y1, fx(s / 18), INK);
    c.fill_ellipse_fx(x0, y0, fx(s / 18), fx(s / 18), hex(0xFF8FA3));
}

/// Folder.
pub fn files(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let back = r(cx, cy, s * 3 / 4, s * 9 / 16);
    c.fill_round_rect(Rect::new(back.x, back.y - s / 10, s / 3, s / 5), s / 16, hex(0x7FC4F5));
    c.fill_round_rect(back, s / 12, hex(0x7FC4F5));
    c.fill_round_rect(Rect::new(back.x + s / 14, back.y - s / 20, back.w - s / 7, back.h / 2), s / 20, WHITE);
    c.fill_round_rect(Rect::new(back.x, back.y + s / 8, back.w, back.h - s / 8), s / 12, hex(0x9AD3FA));
}

/// Analog clock showing the given time.
pub fn clock(c: &mut Canvas, cx: i32, cy: i32, s: i32, hour: u32, min: u32) {
    let rad = s * 4 / 10;
    c.fill_circle(cx, cy + s / 40, rad, with_alpha(0x303858, 40));
    c.fill_circle(cx, cy, rad, hex(0xFF9EB5));
    c.fill_circle(cx, cy, rad * 8 / 10, WHITE);
    for i in 0..12 {
        let a = i * 256 / 12;
        let (dx, dy) = (sin(a) * rad * 68 / 100, -cos(a) * rad * 68 / 100);
        c.fill_ellipse_fx(fx(cx) + dx, fx(cy) + dy, fx(s / 50 + 1), fx(s / 50 + 1), INK_SOFT);
    }
    let ma = (min * 256 / 60) as i32;
    let ha = ((hour % 12) * 256 / 12 + min * 256 / 720) as i32;
    let (cxf, cyf) = (fx(cx), fx(cy));
    c.line_fx(cxf, cyf, cxf + sin(ha) * rad * 40 / 100, cyf - cos(ha) * rad * 40 / 100, fx(s / 16), INK);
    c.line_fx(cxf, cyf, cxf + sin(ma) * rad * 60 / 100, cyf - cos(ma) * rad * 60 / 100, fx(s / 24), INK);
    c.fill_circle(cx, cy, s / 28 + 1, ACCENT);
}

/// Gear (a ring of bumps around a circle).
pub fn settings(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let col = hex(0x9C8CF0);
    let rad = s * 3 / 10;
    for i in 0..8 {
        let a = i * 32;
        let (dx, dy) = (sin(a) * rad, cos(a) * rad);
        c.fill_ellipse_fx(fx(cx) + dx, fx(cy) + dy, fx(s / 9), fx(s / 9), col);
    }
    c.fill_circle(cx, cy, rad, col);
    c.fill_circle(cx, cy, rad * 45 / 100, WHITE);
}

/// "About": a speech bubble with an i.
pub fn about(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let col = hex(0x5CCB9B);
    c.fill_circle(cx, cy, s * 4 / 10, col);
    c.fill_circle(cx, cy - s / 6, s / 16, WHITE);
    c.fill_round_rect(r(cx, cy + s / 12, s / 9, s / 3), s / 18, WHITE);
}

/// Placeholder for an empty slot: a soft "+".
pub fn empty(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let col = hex(0xD5DAE6);
    c.fill_round_rect(r(cx, cy, s / 2, s / 10), s / 20, col);
    c.fill_round_rect(r(cx, cy, s / 10, s / 2), s / 20, col);
}

/// Globe (the web).
pub fn web(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let rad = s * 4 / 10;
    let blue = hex(0x35B6F2);
    c.fill_circle(cx, cy + s / 40, rad, with_alpha(0x303858, 40));
    c.fill_circle(cx, cy, rad, blue);
    let w = fx(s / 22 + 1);
    c.ring_fx(fx(cx), fx(cy), fx(rad * 8 / 10), w, WHITE);
    c.line_fx(fx(cx - rad * 8 / 10), fx(cy), fx(cx + rad * 8 / 10), fx(cy), w, WHITE);
    c.line_fx(fx(cx), fx(cy - rad * 8 / 10), fx(cx), fx(cy + rad * 8 / 10), w, WHITE);
    // the curved "longitude" lines, as thin ellipse outlines
    c.fill_ellipse_fx(fx(cx), fx(cy), fx(rad * 4 / 10), fx(rad * 8 / 10), WHITE);
    c.fill_ellipse_fx(fx(cx), fx(cy), fx(rad * 4 / 10) - w, fx(rad * 8 / 10) - w, blue);
    c.line_fx(fx(cx), fx(cy - rad * 8 / 10), fx(cx), fx(cy + rad * 8 / 10), w, WHITE);
    c.line_fx(fx(cx - rad * 8 / 10), fx(cy), fx(cx + rad * 8 / 10), fx(cy), w, WHITE);
}

/// Wi-Fi: three arcs and a dot.
pub fn wifi(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let col = hex(0x2BB7A3);
    let base = fx(cy + s * 3 / 10);
    for (i, r) in [s * 2 / 10, s * 35 / 100, s / 2].iter().enumerate() {
        let _ = i;
        c.arc_fx(fx(cx), base, fx(*r), fx(*r * 6 / 10), true, fx(s / 12 + 1), col);
    }
    c.fill_ellipse_fx(fx(cx), base - fx(s / 30), fx(s / 14 + 1), fx(s / 14 + 1), col);
}

/// Terminal: a dark window with a ">_" prompt.
pub fn terminal(c: &mut Canvas, cx: i32, cy: i32, s: i32) {
    let w = r(cx, cy, s * 8 / 10, s * 6 / 10);
    c.fill_round_rect(w.offset(0, s / 30), s / 10, with_alpha(0x303858, 40));
    c.fill_round_rect(w, s / 10, INK);
    let lw = fx(s / 16 + 1);
    let (x0, y0) = (fx(w.x + s / 7), fx(w.y + s / 6));
    c.line_fx(x0, y0, x0 + fx(s / 8), y0 + fx(s / 9), lw, hex(0x7ED957));
    c.line_fx(x0 + fx(s / 8), y0 + fx(s / 9), x0, y0 + fx(s * 2 / 9), lw, hex(0x7ED957));
    c.line_fx(x0 + fx(s / 5), y0 + fx(s * 2 / 9), x0 + fx(s * 4 / 10), y0 + fx(s * 2 / 9), lw, WHITE);
}
