//! Settings: Wi-Fi, time zone, installing MiiOS to the laptop's own drive
//! (when running from the USB stick), restart and turn off.

extern crate alloc;
use super::widgets;
use super::{Action, App, AppId, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row { Wifi, TimeZone, Install, Restart, TurnOff }

/// The "install to this laptop" steps.
enum Install { Off, Pick(usize), Confirm(usize, String), Running, Finished(bool) }

pub struct Settings { sel: usize, hover_back: bool, w: i32, h: i32, install: Install, targets: Vec<(String, String)> }

impl Settings {
    pub fn new(w: i32, h: i32) -> Settings { Settings { sel: 0, hover_back: false, w, h, install: Install::Off, targets: Vec::new() } }

    fn rows(&self) -> Vec<Row> {
        let mut v = Vec::new();
        if crate::host::HOSTED { v.push(Row::Wifi); }
        v.push(Row::TimeZone);
        if crate::host::live_session() { v.push(Row::Install); }
        v.push(Row::Restart);
        v.push(Row::TurnOff);
        v
    }
    fn x(&self) -> i32 { (self.w - 1000) / 2 }
    fn row(&self, i: usize) -> Rect { Rect::new(self.x(), 270 + i as i32 * 116, 1000, 96) }
    fn minus(&self, i: usize) -> Rect { let r = self.row(i); Rect::new(r.right() - 220, r.y + 18, 60, 60) }
    fn plus(&self, i: usize) -> Rect { let r = self.row(i); Rect::new(r.right() - 90, r.y + 18, 60, 60) }
    fn all(&self) -> Rect { Rect::new(0, 110, self.w, self.h - 110) }

    fn change_tz(&mut self, s: &mut Shared, by: i32) {
        s.tz_minutes = (s.tz_minutes + by).clamp(-12 * 60, 14 * 60);
        s.local_time = crate::time::unix_now() + s.tz_minutes as i64 * 60;
    }

    fn activate(&mut self, row: Row, s: &mut Shared) -> Action {
        match row {
            Row::Wifi => return Action::Open(AppId::Wifi, self.row(0)),
            Row::Install => { self.targets = crate::host::install_targets(); self.install = Install::Pick(0); }
            Row::Restart => { let _ = super::save_profile(s); crate::arch::power::restart(); }
            Row::TurnOff => { let _ = super::save_profile(s); crate::arch::power::power_off(); }
            Row::TimeZone => {}
        }
        Action::None
    }

    /// Keys while the installer is open.
    fn install_key(&mut self, key: Key) {
        let n = self.targets.len();
        self.install = match core::mem::replace(&mut self.install, Install::Off) {
            Install::Pick(i) => match key {
                Key::Escape => Install::Off,
                Key::Up => Install::Pick(i.saturating_sub(1)),
                Key::Down => Install::Pick((i + 1).min(n.saturating_sub(1))),
                Key::Enter if n > 0 => Install::Confirm(i, String::new()),
                _ => Install::Pick(i),
            },
            Install::Confirm(i, mut typed) => match key {
                Key::Escape => Install::Pick(i),
                Key::Backspace => { typed.pop(); Install::Confirm(i, typed) }
                Key::Char(c) if typed.len() < 8 => { typed.push(c.to_ascii_uppercase()); Install::Confirm(i, typed) }
                Key::Enter if typed == "ERASE" => { crate::host::install_start(&self.targets[i].0); Install::Running }
                _ => Install::Confirm(i, typed),
            },
            Install::Running => Install::Running, // can't cancel half way
            Install::Finished(ok) => match key {
                Key::Enter if ok => crate::arch::power::restart(),
                Key::Escape | Key::Enter => Install::Off,
                _ => Install::Finished(ok),
            },
            Install::Off => Install::Off,
        };
    }
}

impl App for Settings {
    fn enter(&mut self, s: &mut Shared) { self.w = s.w; self.h = s.h; }

    fn event(&mut self, e: &UiEvent, s: &mut Shared, d: &mut Damage) -> Action {
        d.add(self.all());
        if !matches!(self.install, Install::Off) {
            if let UiEvent::Key { key, .. } = *e { self.install_key(key); }
            return Action::None;
        }
        let rows = self.rows();
        match *e {
            UiEvent::Key { key, .. } => match key {
                Key::Escape => { let _ = super::save_profile(s); return Action::Back; }
                Key::Up => { if self.sel > 0 { self.sel -= 1; } }
                Key::Down => { if self.sel + 1 < rows.len() { self.sel += 1; } }
                Key::Left if rows[self.sel] == Row::TimeZone => self.change_tz(s, -30),
                Key::Right if rows[self.sel] == Row::TimeZone => self.change_tz(s, 30),
                Key::Enter => return self.activate(rows[self.sel], s),
                _ => {}
            },
            UiEvent::PointerMove { x, y } => {
                self.hover_back = widgets::back_button().contains(x, y);
                if let Some(i) = (0..rows.len()).find(|&i| self.row(i).contains(x, y)) { self.sel = i; }
            }
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { let _ = super::save_profile(s); return Action::Back; }
                for (i, &r) in rows.iter().enumerate() {
                    if r == Row::TimeZone {
                        if self.minus(i).contains(x, y) { self.change_tz(s, -30); }
                        if self.plus(i).contains(x, y) { self.change_tz(s, 30); }
                    } else if self.row(i).contains(x, y) { return self.activate(r, s); }
                }
            }
            _ => {}
        }
        Action::None
    }

    fn second(&mut self, _s: &mut Shared, d: &mut Damage) {
        if let Install::Running = self.install {
            let (_, done, ok) = crate::host::install_progress();
            if done { self.install = Install::Finished(ok); }
            d.add(self.all());
        } else {
            d.add(Rect::new(0, 130, self.w, 120));
        }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Settings", self.color(), self.hover_back);
        let dt = super::clock::from_unix(s.local_time);
        let mut t = FmtBuf::<64>::new();
        let _ = write!(t, "{:02}:{:02}:{:02}", dt.hour, dt.min, dt.sec);
        c.text_centered(&font::HUGE, s.w / 2, 140, t.as_str(), INK);
        t.clear();
        let _ = write!(t, "{}, {} {} {}", super::clock::WEEKDAYS[dt.weekday as usize], dt.day, super::clock::MONTHS[(dt.month - 1) as usize], dt.year);
        c.text_centered(&font::MEDIUM, s.w / 2, 210, t.as_str(), INK_SOFT);

        let rows = self.rows();
        for (i, &row) in rows.iter().enumerate() {
            let r = self.row(i);
            if !r.inflate(10).overlaps(&c.clip) { continue; }
            c.shadow(r, 30, 8, 40);
            c.fill_round_rect(r, 30, if row == Row::Install { hex(0xFFF4D6) } else { WHITE });
            if i == self.sel { c.stroke_round_rect(r.inflate(6), 36, 5, ACCENT); }
            let (label, hint) = match row {
                Row::Wifi => ("Wi-Fi", String::from(crate::host::wifi_current().as_deref().unwrap_or("not connected"))),
                Row::TimeZone => ("Time zone", String::new()),
                Row::Install => ("Install MiiOS on this laptop", String::from("from this USB stick")),
                Row::Restart => ("Restart", String::from("saves your pals first")),
                Row::TurnOff => ("Turn off", String::from("saves your pals first")),
            };
            c.text(&font::LARGE, r.x + 40, r.y + 32, label, INK);
            if row == Row::TimeZone {
                t.clear();
                let m = s.tz_minutes.abs();
                let _ = write!(t, "UTC{}{:02}:{:02}", if s.tz_minutes < 0 { '-' } else { '+' }, m / 60, m % 60);
                c.text(&font::LARGE, r.right() - 470, r.y + 32, t.as_str(), INK_SOFT);
                widgets::button(c, self.minus(i), "-", false);
                widgets::button(c, self.plus(i), "+", false);
            } else {
                let w = font::MEDIUM.text_width(&hint) as i32;
                c.text(&font::MEDIUM, r.right() - w - 40, r.y + 38, &hint, INK_SOFT);
            }
        }
        let mut info = FmtBuf::<128>::new();
        match crate::fs::info() {
            Some(i) => { let _ = write!(info, "Saving to: {}", i.disk); }
            None => { let _ = write!(info, "No storage: changes last until you turn off. Set up a disk in Files."); }
        }
        c.text(&font::MEDIUM, self.x(), self.h - 80, info.as_str(), INK_SOFT);
        c.text(&font::SMALL, self.x(), self.h - 40, "Up/Down: choose   Left/Right: time zone   Enter: open   Esc: back", INK_SOFT);

        // ---- Installer dialog ----
        if matches!(self.install, Install::Off) { return; }
        let dlg = Rect::new(s.w / 2 - 520, s.h / 2 - 260, 1040, 520);
        c.fill_rect(Rect::new(0, 0, s.w, s.h), with_alpha(0x303858, 60));
        c.shadow(dlg, 40, 16, 80);
        c.fill_round_rect(dlg, 40, WHITE);
        let cx = s.w / 2;
        match &self.install {
            Install::Pick(sel) => {
                c.text_centered(&font::LARGE, cx, dlg.y + 40, "Install MiiOS: pick the laptop's drive", INK);
                if self.targets.is_empty() {
                    c.text_centered(&font::MEDIUM, cx, dlg.y + 200, "No internal drive found.", INK_SOFT);
                }
                for (i, (_dev, desc)) in self.targets.iter().enumerate().take(4) {
                    let r = Rect::new(dlg.x + 60, dlg.y + 110 + i as i32 * 84, dlg.w - 120, 70);
                    c.fill_round_rect(r, 20, if i == *sel { mix(self.color(), WHITE, 90) } else { hex(0xF4F6FB) });
                    if i == *sel { c.stroke_round_rect(r, 20, 4, ACCENT); }
                    c.text(&font::LARGE, r.x + 28, r.y + 19, desc, INK);
                }
                c.text_centered(&font::MEDIUM, cx, dlg.bottom() - 60, "Up/Down: choose   Enter: next   Esc: cancel", INK_SOFT);
            }
            Install::Confirm(i, typed) => {
                let desc = self.targets.get(*i).map(|t| t.1.as_str()).unwrap_or("");
                c.text_centered(&font::LARGE, cx, dlg.y + 50, &alloc::format!("ERASE {}?", desc), hex(0xE0455E));
                c.text_centered(&font::MEDIUM, cx, dlg.y + 130, "Everything on this drive (Windows, Linux, files) will be deleted", INK);
                c.text_centered(&font::MEDIUM, cx, dlg.y + 166, "and replaced with MiiOS. Your pals and notes come along.", INK);
                c.text_centered(&font::MEDIUM, cx, dlg.y + 230, "Type ERASE and press Enter to install, or Esc to go back.", INK_SOFT);
                let field = Rect::new(cx - 160, dlg.y + 290, 320, 72);
                c.fill_round_rect(field, 22, hex(0xF1F4FA));
                c.text_centered(&font::LARGE, cx, field.y + 20, typed, INK);
            }
            Install::Running => {
                let (line, _, _) = crate::host::install_progress();
                c.text_centered(&font::LARGE, cx, dlg.y + 60, "Installing MiiOS...", INK);
                c.text_centered(&font::MEDIUM, cx, dlg.y + 140, "This takes a few minutes. Don't turn the laptop off.", INK_SOFT);
                let dots = (crate::time::uptime_us() / 400_000 % 4) as usize;
                c.text_centered(&font::MEDIUM, cx, dlg.y + 260, &alloc::format!("{}{}", line, ".".repeat(dots)), INK);
            }
            Install::Finished(ok) => {
                if *ok {
                    c.text_centered(&font::LARGE, cx, dlg.y + 80, "MiiOS is installed!", hex(0x2BB7A3));
                    c.text_centered(&font::MEDIUM, cx, dlg.y + 170, "Unplug the USB stick, then press Enter to restart.", INK);
                    c.text_centered(&font::MEDIUM, cx, dlg.y + 210, "The laptop will start MiiOS from its own drive.", INK_SOFT);
                } else {
                    let (line, _, _) = crate::host::install_progress();
                    c.text_centered(&font::LARGE, cx, dlg.y + 80, "The install didn't finish", hex(0xE0455E));
                    c.text_centered(&font::MEDIUM, cx, dlg.y + 170, &line, INK);
                    c.text_centered(&font::MEDIUM, cx, dlg.y + 230, "Nothing was changed on the USB stick. Esc: close", INK_SOFT);
                }
            }
            Install::Off => {}
        }
    }

    fn color(&self) -> Color { PASTELS[5] }
}
