//! Clock: a big analog clock with a smoothly sweeping second hand, and a
//! stopwatch (Space = start/stop, R = reset).

use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use core::fmt::Write as _;

pub struct ClockApp { hover_back: bool, running: bool, start_us: u64, acc_us: u64, w: i32, h: i32 }

impl ClockApp {
    pub fn new(w: i32, h: i32) -> ClockApp { ClockApp { hover_back: false, running: false, start_us: 0, acc_us: 0, w, h } }
    fn face(&self) -> (i32, i32, i32) { (self.w / 3, self.h / 2 + 40, self.h * 32 / 100) }
    fn face_rect(&self) -> Rect { let (x, y, r) = self.face(); Rect::new(x - r - 12, y - r - 12, 2 * r + 24, 2 * r + 30) }
    fn watch_rect(&self) -> Rect { Rect::new(self.w * 58 / 100, self.h / 2 - 80, self.w * 36 / 100, 200) }
    fn elapsed(&self) -> u64 { self.acc_us + if self.running { crate::time::uptime_us() - self.start_us } else { 0 } }
}

impl App for ClockApp {
    fn enter(&mut self, s: &mut Shared) { self.w = s.w; self.h = s.h; }

    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key, .. } => match key {
                Key::Escape => return Action::Back,
                Key::Char(' ') | Key::Enter => {
                    if self.running { self.acc_us = self.elapsed(); self.running = false; }
                    else { self.start_us = crate::time::uptime_us(); self.running = true; }
                    d.add(self.watch_rect());
                }
                Key::Char('r') => { self.running = false; self.acc_us = 0; d.add(self.watch_rect()); }
                _ => {}
            },
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { return Action::Back; }
                if self.watch_rect().contains(x, y) {
                    if self.running { self.acc_us = self.elapsed(); self.running = false; } else { self.start_us = crate::time::uptime_us(); self.running = true; }
                    d.add(self.watch_rect());
                }
            }
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
            }
            _ => {}
        }
        Action::None
    }

    /// The second hand sweeps smoothly, so redraw the face every frame while open.
    fn tick(&mut self, _s: &mut Shared, d: &mut Damage) -> bool {
        d.add(self.face_rect());
        if self.running { d.add(self.watch_rect()); }
        true
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Clock", self.color(), self.hover_back);
        let (cx, cy, r) = self.face();
        if self.face_rect().overlaps(&c.clip) {
            c.fill_circle(cx, cy + 8, r, with_alpha(0x303858, 40));
            c.fill_circle(cx, cy, r, hex(0xFF9EB5));
            c.fill_circle(cx, cy, r - 18, WHITE);
            for i in 0..60 {
                let a = i * 256 / 60;
                let big = i % 5 == 0;
                let rr = r - 50;
                let (dx, dy) = (sin(a) * rr, -cos(a) * rr);
                let dot = if big { fx(7) } else { fx(3) };
                c.fill_ellipse_fx(fx(cx) + dx, fx(cy) + dy, dot, dot, if big { INK } else { INK_SOFT });
            }
            // Time with sub-second precision for a smooth sweep.
            let us = crate::time::uptime_us();
            let frac = ((us % 1_000_000) * 256 / 1_000_000) as i32;
            let dt = super::clock::from_unix(s.local_time);
            let sec = dt.sec as i32 * 256 + frac;         // seconds in 1/256 units
            let min = dt.min as i32 * 256 + sec / 60;
            let hour = (dt.hour % 12) as i32 * 256 + min / 60;
            let hand = |c: &mut Canvas, angle_256ths: i32, len: i32, w: i32, col: Color| {
                let a = angle_256ths;
                c.line_fx(fx(cx), fx(cy), fx(cx) + sin(a) * len, fx(cy) - cos(a) * len, fx(w), col);
            };
            hand(c, hour * 256 / (12 * 256), r * 45 / 100, 14, INK);
            hand(c, min * 256 / (60 * 256), r * 70 / 100, 9, INK);
            hand(c, sec * 256 / (60 * 256), r * 78 / 100, 4, hex(0xFF6F91));
            c.fill_circle(cx, cy, 14, hex(0xFF6F91));
        }
        // Stopwatch.
        let wr = self.watch_rect();
        if wr.inflate(10).overlaps(&c.clip) {
            c.fill_round_rect(wr, 40, WHITE);
            let e = self.elapsed();
            let mut t = FmtBuf::<24>::new();
            let _ = write!(t, "{:02}:{:02}.{:02}", e / 60_000_000, e / 1_000_000 % 60, e / 10_000 % 100);
            c.text_centered(&font::HUGE, wr.x + wr.w / 2, wr.y + 40, t.as_str(), INK);
            c.text_centered(&font::MEDIUM, wr.x + wr.w / 2, wr.y + 130, if self.running { "Space: stop" } else { "Space: start    R: reset" }, INK_SOFT);
        }
        let mut t = FmtBuf::<16>::new();
        let dt = super::clock::from_unix(s.local_time);
        let _ = write!(t, "{}:{:02}", dt.hour, dt.min);
        c.text_centered(&font::HUGE, wr.x + wr.w / 2, wr.y - 160, t.as_str(), INK_SOFT);
    }

    fn color(&self) -> Color { PASTELS[1] }
}
