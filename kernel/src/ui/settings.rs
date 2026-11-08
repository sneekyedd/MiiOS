//! Settings: time zone, storage info, and power (restart / turn off).

use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use core::fmt::Write as _;

const ROWS: usize = 3; // time zone, restart, turn off

pub struct Settings { sel: usize, hover_back: bool, w: i32, h: i32 }

impl Settings {
    pub fn new(w: i32, h: i32) -> Settings { Settings { sel: 0, hover_back: false, w, h } }
    fn x(&self) -> i32 { (self.w - 1000) / 2 }
    fn row(&self, i: usize) -> Rect { Rect::new(self.x(), 300 + i as i32 * 130, 1000, 104) }
    fn minus(&self) -> Rect { let r = self.row(0); Rect::new(r.right() - 220, r.y + 22, 60, 60) }
    fn plus(&self) -> Rect { let r = self.row(0); Rect::new(r.right() - 90, r.y + 22, 60, 60) }
    fn all(&self) -> Rect { Rect::new(0, 110, self.w, self.h - 110) }

    fn change_tz(&mut self, s: &mut Shared, by: i32, d: &mut Damage) {
        s.tz_minutes = (s.tz_minutes + by).clamp(-12 * 60, 14 * 60);
        s.local_time = crate::time::unix_now() + s.tz_minutes as i64 * 60;
        d.add(self.all());
    }

    fn activate(&mut self, i: usize, s: &mut Shared) {
        let _ = super::save_profile(s); // save before the machine goes away
        match i {
            1 => crate::arch::power::restart(),
            2 => crate::arch::power::power_off(),
            _ => {}
        }
    }
}

impl App for Settings {
    fn enter(&mut self, s: &mut Shared) { self.w = s.w; self.h = s.h; }

    fn event(&mut self, e: &UiEvent, s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key, .. } => match key {
                Key::Escape => { let _ = super::save_profile(s); return Action::Back; }
                Key::Up => { if self.sel > 0 { self.sel -= 1; d.add(self.all()); } }
                Key::Down => { if self.sel + 1 < ROWS { self.sel += 1; d.add(self.all()); } }
                Key::Left if self.sel == 0 => self.change_tz(s, -30, d),
                Key::Right if self.sel == 0 => self.change_tz(s, 30, d),
                Key::Enter => self.activate(self.sel, s),
                _ => {}
            },
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
                if let Some(i) = (0..ROWS).find(|&i| self.row(i).contains(x, y)) { if i != self.sel { self.sel = i; d.add(self.all()); } }
            }
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { let _ = super::save_profile(s); return Action::Back; }
                if self.minus().contains(x, y) { self.change_tz(s, -30, d); }
                else if self.plus().contains(x, y) { self.change_tz(s, 30, d); }
                else if let Some(i) = (1..ROWS).find(|&i| self.row(i).contains(x, y)) { self.activate(i, s); }
            }
            _ => {}
        }
        Action::None
    }

    fn second(&mut self, _s: &mut Shared, d: &mut Damage) { d.add(Rect::new(0, 140, self.w, 120)); }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Settings", self.color(), self.hover_back);
        let dt = super::clock::from_unix(s.local_time);
        let mut t = FmtBuf::<64>::new();
        let _ = write!(t, "{:02}:{:02}:{:02}", dt.hour, dt.min, dt.sec);
        c.text_centered(&font::HUGE, s.w / 2, 150, t.as_str(), INK);
        t.clear();
        let _ = write!(t, "{}, {} {} {}", super::clock::WEEKDAYS[dt.weekday as usize], dt.day, super::clock::MONTHS[(dt.month - 1) as usize], dt.year);
        c.text_centered(&font::MEDIUM, s.w / 2, 222, t.as_str(), INK_SOFT);

        let labels = ["Time zone", "Restart", "Turn off"];
        for i in 0..ROWS {
            let r = self.row(i);
            if !r.inflate(10).overlaps(&c.clip) { continue; }
            c.shadow(r, 30, 8, 40);
            c.fill_round_rect(r, 30, WHITE);
            if i == self.sel { c.stroke_round_rect(r.inflate(6), 36, 5, ACCENT); }
            c.text(&font::LARGE, r.x + 40, r.y + 36, labels[i], INK);
            match i {
                0 => {
                    t.clear();
                    let m = s.tz_minutes.abs();
                    let _ = write!(t, "UTC{}{:02}:{:02}", if s.tz_minutes < 0 { '-' } else { '+' }, m / 60, m % 60);
                    c.text(&font::LARGE, r.right() - 470, r.y + 36, t.as_str(), INK_SOFT);
                    widgets::button(c, self.minus(), "-", false);
                    widgets::button(c, self.plus(), "+", false);
                }
                1 => { c.text(&font::MEDIUM, r.right() - 420, r.y + 42, "Saves your pals first", INK_SOFT); }
                _ => { c.text(&font::MEDIUM, r.right() - 420, r.y + 42, "Saves your pals first", INK_SOFT); }
            }
        }
        let mut info = FmtBuf::<128>::new();
        match crate::fs::info() {
            Some(i) => { let _ = write!(info, "Saving to: {} ({} files)", i.disk, i.files); }
            None => { let _ = write!(info, "No storage: changes last until you turn off. Set up a disk in Files."); }
        }
        c.text(&font::MEDIUM, self.x(), self.row(2).bottom() + 50, info.as_str(), INK_SOFT);
        c.text(&font::SMALL, self.x(), self.h - 40, "Up/Down: choose   Left/Right: change time zone   Enter: do it   Esc: back", INK_SOFT);
    }

    fn color(&self) -> Color { PASTELS[5] }
}
