//! Drawing primitives.
//!
//! A `Canvas` is a block of pixels in RAM (our *back buffer*) plus a *clip
//! rectangle*. Every primitive only touches pixels inside the clip, so when
//! only a small part of the screen changed we can redraw just that part: set
//! the clip to the changed area, draw the scene, and anything outside is
//! skipped almost for free.
//!
//! Smooth ("anti-aliased") edges: for curved shapes we sample each pixel row
//! at 4 heights. For each sample we compute exactly where the shape starts and
//! ends (in 1/256 pixel units) and add up how much of each pixel is covered.
//! Pixels fully inside are filled with a fast `rep stosd`; only the few edge
//! pixels need blending. That's cheap and gives clean curves.

use super::color::{self, Color};
use super::font::Font;
use super::math::*;
use super::rect::Rect;
use crate::arch::cpu;

pub struct Canvas {
    pub buf: *mut u32,
    pub width: i32,
    pub height: i32,
    pub stride: i32, // pixels per row in memory
    pub clip: Rect,
}

/// Number of vertical sub-samples per pixel row for anti-aliasing.
const SUB: i32 = 4;

impl Canvas {
    pub fn new(buf: *mut u32, width: i32, height: i32, stride: i32) -> Canvas {
        Canvas { buf, width, height, stride, clip: Rect::new(0, 0, width, height) }
    }

    pub fn bounds(&self) -> Rect { Rect::new(0, 0, self.width, self.height) }

    /// Restrict drawing to `r` (and the screen). Returns the old clip.
    pub fn set_clip(&mut self, r: Rect) -> Rect {
        let old = self.clip;
        self.clip = r.intersect(&self.bounds());
        old
    }

    #[inline(always)]
    fn row(&self, y: i32) -> *mut u32 { unsafe { self.buf.add((y * self.stride) as usize) } }

    #[inline(always)]
    pub fn put(&mut self, x: i32, y: i32, c: Color) {
        if self.clip.contains(x, y) { unsafe { *self.row(y).add(x as usize) = c & 0xFFFFFF } }
    }

    /// Blend one pixel with coverage `a` (0..=256), also honouring the colour's alpha.
    #[inline(always)]
    fn blend_px(&mut self, x: i32, y: i32, c: Color, a: u32) {
        if a == 0 || !self.clip.contains(x, y) { return; }
        let a = (a * (color::alpha(c) + 1)) >> 8;
        unsafe {
            let p = self.row(y).add(x as usize);
            *p = color::blend(*p, c, a);
        }
    }

    /// Horizontal run of pixels [x0, x1) on row y (already clipped by caller or not).
    #[inline(always)]
    fn hspan(&mut self, y: i32, x0: i32, x1: i32, c: Color) {
        if y < self.clip.y || y >= self.clip.bottom() { return; }
        let x0 = x0.max(self.clip.x);
        let x1 = x1.min(self.clip.right());
        if x1 <= x0 { return; }
        let a = color::alpha(c);
        unsafe {
            let p = self.row(y).add(x0 as usize);
            if a == 255 {
                cpu::fill_u32(p, c & 0xFFFFFF, (x1 - x0) as usize);
            } else {
                for i in 0..(x1 - x0) as usize {
                    *p.add(i) = color::blend(*p.add(i), c, a + 1);
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Rectangles
    // ------------------------------------------------------------------

    pub fn fill_rect(&mut self, r: Rect, c: Color) {
        let r = r.intersect(&self.clip);
        for y in r.y..r.bottom() { self.hspan(y, r.x, r.right(), c); }
    }

    /// Fill with a vertical gradient from `top` to `bottom` colour.
    pub fn fill_gradient(&mut self, r: Rect, top: Color, bottom: Color) {
        let cr = r.intersect(&self.clip);
        for y in cr.y..cr.bottom() {
            let t = ((y - r.y) * 256 / r.h.max(1)) as u32;
            self.hspan(y, cr.x, cr.right(), color::mix(top, bottom, t));
        }
    }

    // ------------------------------------------------------------------
    // Generic anti-aliased filled shape
    // ------------------------------------------------------------------

    /// Fill any shape that is "convex per row": for a height `y` (24.8 fixed),
    /// `span(y)` returns the left and right edge (24.8 fixed) or None.
    /// `top` and `bottom` are the shape's vertical extent in 24.8 fixed.
    pub fn fill_spans(&mut self, top: Fx, bottom: Fx, c: Color, span: impl Fn(Fx) -> Option<(Fx, Fx)>) {
        let y0 = fx_floor(top).max(self.clip.y);
        let y1 = fx_ceil(bottom).min(self.clip.bottom());
        for py in y0..y1 {
            let mut lo = [0 as Fx; SUB as usize];
            let mut hi = [0 as Fx; SUB as usize];
            let mut n = 0usize;
            for s in 0..SUB {
                // sample height: middle of each quarter of the pixel row
                let sy = fx(py) + (s * 2 + 1) * ONE / (SUB * 2);
                if sy < top || sy > bottom { continue; }
                if let Some((a, b)) = span(sy) {
                    if b > a { lo[n] = a; hi[n] = b; n += 1; }
                }
            }
            if n == 0 { continue; }
            let (mut min_lo, mut max_lo, mut min_hi, mut max_hi) = (Fx::MAX, Fx::MIN, Fx::MAX, Fx::MIN);
            for i in 0..n {
                min_lo = min_lo.min(lo[i]); max_lo = max_lo.max(lo[i]);
                min_hi = min_hi.min(hi[i]); max_hi = max_hi.max(hi[i]);
            }
            // Pixels completely covered by every sample: solid fill.
            let solid0 = if n as i32 == SUB { fx_ceil(max_lo) } else { i32::MAX };
            let solid1 = if n as i32 == SUB { fx_floor(min_hi) } else { i32::MIN };
            // Edge pixels: compute partial coverage.
            let e0 = fx_floor(min_lo);
            let e1 = fx_ceil(max_hi);
            let cover = |px: i32| -> u32 {
                let (pl, pr) = (fx(px), fx(px + 1));
                let mut sum = 0i32;
                for i in 0..n {
                    let a = lo[i].max(pl);
                    let b = hi[i].min(pr);
                    if b > a { sum += b - a; }
                }
                (sum / SUB) as u32 // 0..=256
            };
            let (cl, cr) = (self.clip.x, self.clip.right());
            if solid1 > solid0 {
                for px in e0.max(cl)..solid0.min(cr) { let a = cover(px); self.blend_px(px, py, c, a); }
                self.hspan(py, solid0, solid1, c);
                for px in solid1.max(cl)..e1.min(cr) { let a = cover(px); self.blend_px(px, py, c, a); }
            } else {
                let xs = e0.max(self.clip.x);
                let xe = e1.min(self.clip.right());
                for px in xs..xe { let a = cover(px); self.blend_px(px, py, c, a); }
            }
        }
    }

    /// Anti-aliased filled ellipse. Centre and radii in 24.8 fixed point,
    /// so shapes can move by fractions of a pixel (smooth animation).
    pub fn fill_ellipse_fx(&mut self, cx: Fx, cy: Fx, rx: Fx, ry: Fx, c: Color) {
        if rx <= 0 || ry <= 0 { return; }
        let bb = Rect::new(fx_floor(cx - rx) - 1, fx_floor(cy - ry) - 1, fx_ceil(rx * 2) + 3, fx_ceil(ry * 2) + 3);
        if !bb.overlaps(&self.clip) { return; }
        let (rx64, ry64) = (rx as i64, ry as i64);
        self.fill_spans(cy - ry, cy + ry, c, |y| {
            let dy = (y - cy) as i64;
            // half-width = rx * sqrt(1 - dy^2/ry^2) = rx/ry * sqrt(ry^2 - dy^2)
            let inside = ry64 * ry64 - dy * dy;
            if inside <= 0 { return None; }
            let hw = (rx64 * isqrt(inside as u64) as i64 / ry64) as Fx;
            Some((cx - hw, cx + hw))
        });
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, c: Color) {
        self.fill_ellipse_fx(fx(cx), fx(cy), fx(r), fx(r), c);
    }

    /// Anti-aliased rounded rectangle (24.8 fixed point position/size).
    pub fn fill_round_rect_fx(&mut self, x: Fx, y: Fx, w: Fx, h: Fx, radius: Fx, c: Color) {
        let r = radius.min(w / 2).min(h / 2).max(0);
        let bb = Rect::new(fx_floor(x) - 1, fx_floor(y) - 1, fx_ceil(w) + 3, fx_ceil(h) + 3);
        if !bb.overlaps(&self.clip) || w <= 0 || h <= 0 { return; }
        // Fast path for the straight middle band: plain solid rows when aligned to pixels.
        let r64 = r as i64;
        self.fill_spans(y, y + h, c, |sy| {
            let dy = if sy < y + r { (y + r - sy) as i64 } else if sy > y + h - r { (sy - (y + h - r)) as i64 } else { 0 };
            let inset = if dy == 0 { 0 } else {
                let inside = r64 * r64 - dy * dy;
                if inside < 0 { return None; }
                r - isqrt(inside as u64) as Fx
            };
            Some((x + inset, x + w - inset))
        });
    }

    pub fn fill_round_rect(&mut self, r: Rect, radius: i32, c: Color) {
        self.fill_round_rect_fx(fx(r.x), fx(r.y), fx(r.w), fx(r.h), fx(radius), c);
    }

    /// Rounded-rectangle outline of the given thickness (drawn as a ring).
    pub fn stroke_round_rect(&mut self, r: Rect, radius: i32, thickness: i32, c: Color) {
        let (x, y, w, h, rad, t) = (fx(r.x), fx(r.y), fx(r.w), fx(r.h), fx(radius), fx(thickness));
        let ri = (rad - t).max(0);
        let outer = |sy: Fx| -> Option<(Fx, Fx)> { round_span(x, y, w, h, rad, sy) };
        let inner = |sy: Fx| -> Option<(Fx, Fx)> { round_span(x + t, y + t, w - 2 * t, h - 2 * t, ri, sy) };
        // Left and right parts of the ring, each convex per row.
        self.fill_spans(y, y + h, c, |sy| {
            let (a, b) = outer(sy)?;
            match inner(sy) { Some((ia, _)) => Some((a, ia.min(b))), None => Some((a, b)) }
        });
        self.fill_spans(y + t, y + h - t, c, |sy| {
            let (_, b) = outer(sy)?;
            let (_, ib) = inner(sy)?;
            Some((ib, b))
        });
    }

    /// A soft drop shadow under a rounded rect: stacked translucent layers,
    /// each a bit smaller. Only the rim around `r` is drawn (the middle will be
    /// covered by whatever casts the shadow), which saves most of the work.
    pub fn shadow(&mut self, r: Rect, radius: i32, spread: i32, strength: u8) {
        let layers = spread.max(1);
        let a = (strength as i32 / layers).clamp(1, 255) as u8;
        let c = color::with_alpha(0x303858, a);
        let dy = spread / 3;
        let inner = Rect::new(r.x + radius, r.y + radius + dy, r.w - 2 * radius, r.h - 2 * radius);
        let outer = r.inflate(spread).offset(0, dy);
        let bands = [
            Rect::new(outer.x, outer.y, outer.w, inner.y - outer.y),
            Rect::new(outer.x, inner.bottom(), outer.w, outer.bottom() - inner.bottom()),
            Rect::new(outer.x, inner.y, inner.x - outer.x, inner.h),
            Rect::new(inner.right(), inner.y, outer.right() - inner.right(), inner.h),
        ];
        let saved = self.clip;
        for b in bands {
            self.clip = saved.intersect(&b);
            if self.clip.is_empty() { continue; }
            for i in 0..layers {
                let rr = r.inflate(spread - i).offset(0, dy);
                self.fill_round_rect(rr, radius + spread - i, c);
            }
        }
        self.clip = saved;
    }

    // ------------------------------------------------------------------
    // Signed-distance shapes (lines, arcs)
    // ------------------------------------------------------------------

    /// Fill the pixels where `dist(x, y)` (24.8 fixed, negative = inside) is <= 0,
    /// with a 1-pixel soft edge. Good for thick lines and arcs.
    pub fn fill_sdf(&mut self, bbox: Rect, c: Color, dist: impl Fn(Fx, Fx) -> Fx) {
        let r = bbox.intersect(&self.clip);
        for py in r.y..r.bottom() {
            for px in r.x..r.right() {
                let d = dist(fx(px) + HALF, fx(py) + HALF);
                let a = (HALF - d).clamp(0, ONE) as u32;
                if a > 0 { self.blend_px(px, py, c, a); }
            }
        }
    }

    /// Thick line with round ends, from (x0,y0) to (x1,y1), all in 24.8 fixed.
    pub fn line_fx(&mut self, x0: Fx, y0: Fx, x1: Fx, y1: Fx, width: Fx, c: Color) {
        let hw = width / 2;
        let bb = Rect::new(fx_floor(x0.min(x1) - hw) - 1, fx_floor(y0.min(y1) - hw) - 1,
            fx_ceil((x1 - x0).abs() + width) + 3, fx_ceil((y1 - y0).abs() + width) + 3);
        let (dx, dy) = ((x1 - x0) as i64, (y1 - y0) as i64);
        let len2 = (dx * dx + dy * dy).max(1);
        self.fill_sdf(bb, c, |px, py| {
            let (vx, vy) = ((px - x0) as i64, (py - y0) as i64);
            let t = ((vx * dx + vy * dy) * 256 / len2).clamp(0, 256);
            let (qx, qy) = (vx - dx * t / 256, vy - dy * t / 256);
            isqrt((qx * qx + qy * qy) as u64) as Fx - hw
        });
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, width: i32, c: Color) {
        self.line_fx(fx(x0) + HALF, fx(y0) + HALF, fx(x1) + HALF, fx(y1) + HALF, fx(width), c);
    }

    /// Circle outline (a ring) with soft edges.
    pub fn ring_fx(&mut self, cx: Fx, cy: Fx, r: Fx, width: Fx, c: Color) {
        let ext = r + width;
        let bb = Rect::new(fx_floor(cx - ext) - 1, fx_floor(cy - ext) - 1, fx_ceil(2 * ext) + 3, fx_ceil(2 * ext) + 3);
        let hw = width / 2;
        self.fill_sdf(bb, c, |px, py| {
            let (dx, dy) = ((px - cx) as i64, (py - cy) as i64);
            let d = isqrt((dx * dx + dy * dy) as u64) as i64 - r as i64;
            (d.abs() - hw as i64) as Fx
        });
    }

    /// Arc of a circle (centre cx,cy radius r), only the part below the line
    /// y = cy + cut (cut can be negative). Round ends. Perfect for smiles:
    /// a smile is the bottom of a circle; a frown is the top (use `up = true`).
    pub fn arc_fx(&mut self, cx: Fx, cy: Fx, r: Fx, cut: Fx, up: bool, width: Fx, c: Color) {
        let hw = width / 2;
        let bb = Rect::new(fx_floor(cx - r - hw) - 1, fx_floor(cy - r - hw) - 1,
            fx_ceil(2 * (r + hw)) + 3, fx_ceil(2 * (r + hw)) + 3);
        // Ends of the arc: where the circle meets the cut line.
        let ex = isqrt(((r as i64 * r as i64) - (cut as i64 * cut as i64)).max(0) as u64) as i64;
        let ey = if up { cy - cut } else { cy + cut } as i64;
        self.fill_sdf(bb, c, |px, py| {
            let (dx, dy) = ((px - cx) as i64, (py - cy) as i64);
            let in_range = if up { dy <= -(cut as i64) } else { dy >= cut as i64 };
            if in_range {
                let d = isqrt((dx * dx + dy * dy) as u64) as i64 - r as i64;
                (d.abs() - hw as i64) as Fx
            } else {
                let (ax, ay) = (px as i64 - (cx as i64 - ex), py as i64 - ey);
                let (bx, by) = (px as i64 - (cx as i64 + ex), py as i64 - ey);
                let d = isqrt((ax * ax + ay * ay) as u64).min(isqrt((bx * bx + by * by) as u64));
                d as Fx - hw
            }
        });
    }

    // ------------------------------------------------------------------
    // Text
    // ------------------------------------------------------------------

    /// Draw text with a bitmap font. Returns the x position after the text.
    pub fn text(&mut self, f: &Font, x: i32, y: i32, s: &str, c: Color) -> i32 {
        let mut cx = x;
        let line = Rect::new(x, y, f.text_width(s) as i32, f.height as i32);
        if !line.overlaps(&self.clip) { return x + line.w; }
        let col = c & 0xFFFFFF;
        for ch in s.chars() {
            let gr = Rect::new(cx, y, f.width as i32, f.height as i32);
            if gr.overlaps(&self.clip) {
                let g = f.glyph(ch);
                let vis = gr.intersect(&self.clip);
                for py in vis.y..vis.bottom() {
                    let row = self.row(py);
                    for px in vis.x..vis.right() {
                        if f.bit(g, (px - cx) as u32, (py - y) as u32) {
                            unsafe { *row.add(px as usize) = col }
                        }
                    }
                }
            }
            cx += f.width as i32;
        }
        cx
    }

    /// Text centred horizontally on `cx`.
    pub fn text_centered(&mut self, f: &Font, cx: i32, y: i32, s: &str, c: Color) {
        let w = f.text_width(s) as i32;
        self.text(f, cx - w / 2, y, s, c);
    }
}

/// Left/right edge of a rounded rect at height sy (helper for outlines).
fn round_span(x: Fx, y: Fx, w: Fx, h: Fx, r: Fx, sy: Fx) -> Option<(Fx, Fx)> {
    if w <= 0 || h <= 0 || sy < y || sy > y + h { return None; }
    let dy = if sy < y + r { (y + r - sy) as i64 } else if sy > y + h - r { (sy - (y + h - r)) as i64 } else { 0 };
    let inset = if dy == 0 { 0 } else {
        let inside = r as i64 * r as i64 - dy * dy;
        if inside < 0 { return None; }
        r - isqrt(inside as u64) as Fx
    };
    Some((x + inset, x + w - inset))
}
