//! "Files": your saved notes. Open one in Write, make a new one, delete.
//! If there's no MiiOS storage yet, it offers to set up a disk (after a very
//! clear warning, because that ERASES the disk).

extern crate alloc;
use super::widgets;
use super::{Action, App, AppId, Shared, UiEvent};
use crate::fs::{self, miifs::Entry};
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

const ROW_H: i32 = 64;
const BUTTONS: [&str; 3] = ["New note", "Open", "Delete"];

#[derive(PartialEq, Eq, Clone, Copy)]
enum Confirm { None, Delete, Erase(usize) }

pub struct Files {
    entries: Vec<Entry>,
    sel: usize,
    btn: Option<usize>,      // keyboard focus on a button (None = the list)
    confirm: Confirm,
    typed: FmtBuf<8>,        // for typing ERASE
    msg: FmtBuf<80>,
    hover_back: bool,
    w: i32,
    h: i32,
    was_probing: bool,
}

impl Files {
    pub fn new(w: i32, h: i32) -> Files {
        Files { entries: Vec::new(), sel: 0, btn: None, confirm: Confirm::None, typed: FmtBuf::new(), msg: FmtBuf::new(),
                hover_back: false, w, h, was_probing: true }
    }

    fn refresh(&mut self) {
        self.entries = fs::list().into_iter().filter(|e| e.name().ends_with(".txt")).collect();
        if self.sel >= self.entries.len() { self.sel = self.entries.len().saturating_sub(1); }
    }

    fn list_x(&self) -> i32 { (self.w - 1100) / 2 }
    fn row_rect(&self, i: usize) -> Rect { Rect::new(self.list_x(), 300 + i as i32 * (ROW_H + 10), 1100, ROW_H) }
    fn button_rect(&self, i: usize) -> Rect { Rect::new(self.list_x() + i as i32 * 220, self.h - 130, 200, 64) }
    fn erase_rect(&self, i: usize) -> Rect { Rect::new(self.list_x(), 330 + i as i32 * 90, 1100, 76) }
    fn all(&self) -> Rect { Rect::new(0, 110, self.w, self.h - 110) }

    fn set_msg(&mut self, m: &str) { self.msg.clear(); let _ = self.msg.write_str(m); }

    fn open_selected(&mut self, s: &mut Shared) -> Action {
        let Some(e) = self.entries.get(self.sel) else { return Action::None };
        s.open_doc = Some(String::from(e.name()));
        Action::Open(AppId::Write, self.row_rect(self.sel))
    }

    fn new_note(&mut self, s: &mut Shared) -> Action {
        let mut n = 1;
        let name = loop {
            let mut f = FmtBuf::<24>::new();
            let _ = write!(f, "note-{}.txt", n);
            if !self.entries.iter().any(|e| e.name() == f.as_str()) { break String::from(f.as_str()); }
            n += 1;
        };
        s.open_doc = Some(name);
        Action::Open(AppId::Write, self.button_rect(0))
    }

    fn press(&mut self, i: usize, s: &mut Shared, d: &mut Damage) -> Action {
        d.add(self.all());
        match i {
            0 => self.new_note(s),
            1 => self.open_selected(s),
            _ => { if !self.entries.is_empty() { self.confirm = Confirm::Delete; } Action::None }
        }
    }
}

impl App for Files {
    fn enter(&mut self, s: &mut Shared) {
        self.w = s.w; self.h = s.h;
        self.refresh();
        self.confirm = Confirm::None;
        self.msg.clear();
    }

    fn event(&mut self, e: &UiEvent, s: &mut Shared, d: &mut Damage) -> Action {
        // ---- Confirmation dialogs take all keys ----
        if self.confirm != Confirm::None {
            if let UiEvent::Key { key, .. } = *e {
                d.add(self.all());
                match (self.confirm, key) {
                    (_, Key::Escape) => { self.confirm = Confirm::None; self.typed.clear(); }
                    (Confirm::Delete, Key::Enter | Key::Char('y')) => {
                        let name = String::from(self.entries[self.sel].name());
                        match fs::delete(&name) { Ok(()) => self.set_msg("Deleted."), Err(er) => self.set_msg(er) }
                        self.confirm = Confirm::None;
                        self.refresh();
                    }
                    (Confirm::Delete, Key::Char('n')) => self.confirm = Confirm::None,
                    (Confirm::Erase(i), Key::Enter) => {
                        if self.typed.as_str() == "ERASE" {
                            match fs::prepare_disk(i) { Ok(()) => self.set_msg("Disk ready! Your files will be saved there."), Err(er) => self.set_msg(er) }
                            let _ = super::save_profile(s);
                            self.refresh();
                        } else { self.set_msg("Cancelled (you need to type ERASE)."); }
                        self.confirm = Confirm::None;
                        self.typed.clear();
                    }
                    (Confirm::Erase(_), Key::Char(c)) => { let _ = self.typed.write_char(c.to_ascii_uppercase()); }
                    (Confirm::Erase(_), Key::Backspace) => { let t = String::from(self.typed.as_str()); self.typed.clear(); let _ = self.typed.write_str(&t[..t.len().saturating_sub(1)]); }
                    _ => {}
                }
            }
            return Action::None;
        }
        match *e {
            UiEvent::Key { key, .. } => {
                d.add(self.all());
                match key {
                    Key::Escape => return Action::Back,
                    Key::Up => { if self.btn.is_some() { self.btn = None; } else if self.sel > 0 { self.sel -= 1; } }
                    Key::Down => { if self.sel + 1 < self.entries.len() && self.btn.is_none() { self.sel += 1; } else { self.btn = Some(self.btn.unwrap_or(0)); } }
                    Key::Left => { if let Some(b) = self.btn { self.btn = Some(b.saturating_sub(1)); } }
                    Key::Right => { if let Some(b) = self.btn { self.btn = Some((b + 1).min(2)); } }
                    Key::Enter => return match self.btn { Some(b) => self.press(b, s, d), None => self.open_selected(s) },
                    Key::Delete => { if !self.entries.is_empty() && fs::mounted() { self.confirm = Confirm::Delete; } }
                    Key::Char('n') if fs::mounted() => return self.new_note(s),
                    Key::Char(c @ '1'..='9') if !fs::mounted() => {
                        let i = c as usize - '1' as usize;
                        if i < fs::other_disks().len() { self.confirm = Confirm::Erase(i); self.typed.clear(); }
                    }
                    _ => {}
                }
            }
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
            }
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { return Action::Back; }
                d.add(self.all());
                if fs::mounted() {
                    for i in 0..self.entries.len() {
                        if self.row_rect(i).contains(x, y) {
                            if self.sel == i { return self.open_selected(s); } // second click opens
                            self.sel = i; self.btn = None;
                        }
                    }
                    for i in 0..3 { if self.button_rect(i).contains(x, y) { return self.press(i, s, d); } }
                } else {
                    for i in 0..fs::other_disks().len() { if self.erase_rect(i).contains(x, y) { self.confirm = Confirm::Erase(i); self.typed.clear(); } }
                }
            }
            _ => {}
        }
        Action::None
    }

    fn second(&mut self, _s: &mut Shared, d: &mut Damage) {
        // Storage may come online a moment after boot.
        let p = fs::probing();
        if p != self.was_probing { self.was_probing = p; self.refresh(); d.add(self.all()); }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Files", self.color(), self.hover_back);
        let x = self.list_x();
        // ---- Storage summary ----
        let card = Rect::new(x, 140, 1100, 110);
        c.fill_round_rect(card, 28, WHITE);
        let mut t = FmtBuf::<128>::new();
        match fs::info() {
            Some(i) => {
                let _ = write!(t, "{}", i.disk);
                c.text(&font::LARGE, card.x + 32, card.y + 20, t.as_str(), INK);
                let bar = Rect::new(card.x + 32, card.y + 70, 700, 16);
                c.fill_round_rect(bar, 8, hex(0xE6EAF2));
                let used_w = (bar.w as u64 * i.used_kib / i.total_kib.max(1)).max(16) as i32;
                c.fill_round_rect(Rect::new(bar.x, bar.y, used_w.min(bar.w), bar.h), 8, ACCENT);
                t.clear();
                let _ = write!(t, "{} KB of {} MB used", i.used_kib, i.total_kib / 1024);
                c.text(&font::MEDIUM, bar.right() + 24, bar.y - 4, t.as_str(), INK_SOFT);
            }
            None if fs::probing() => { c.text(&font::LARGE, card.x + 32, card.y + 38, "Looking for disks...", INK_SOFT); }
            None => {
                c.text(&font::LARGE, card.x + 32, card.y + 20, "No MiiOS storage yet", INK);
                c.text(&font::MEDIUM, card.x + 32, card.y + 66, "Notes and pals stay in memory until you set up a disk below.", INK_SOFT);
            }
        }

        if fs::mounted() {
            // ---- File list ----
            if self.entries.is_empty() {
                c.text_centered(&font::LARGE, s.w / 2, 360, "No notes yet. Press N or click \"New note\".", INK_SOFT);
            }
            for (i, e) in self.entries.iter().enumerate().take(((self.h - 450) / (ROW_H + 10)) as usize) {
                let r = self.row_rect(i);
                if !r.inflate(8).overlaps(&c.clip) { continue; }
                let sel = i == self.sel && self.btn.is_none();
                c.fill_round_rect(r, 20, if sel { mix(self.color(), WHITE, 100) } else { WHITE });
                if sel { c.stroke_round_rect(r, 20, 4, ACCENT); }
                super::icons::write(c, r.x + 44, r.y + r.h / 2, 44);
                c.text(&font::LARGE, r.x + 90, r.y + 16, e.name(), INK);
                let dt = super::clock::from_unix(e.mtime + s.tz_minutes as i64 * 60);
                let mut m = FmtBuf::<48>::new();
                let _ = write!(m, "{} bytes   {} {} {:02}:{:02}", e.size, dt.day, super::clock::MONTHS[(dt.month - 1) as usize % 12], dt.hour, dt.min);
                let mw = font::MEDIUM.text_width(m.as_str()) as i32;
                c.text(&font::MEDIUM, r.right() - mw - 28, r.y + 20, m.as_str(), INK_SOFT);
            }
            for i in 0..3 { widgets::button(c, self.button_rect(i), BUTTONS[i], self.btn == Some(i)); }
        } else if !fs::probing() {
            // ---- Offer to set up a disk ----
            let disks = fs::other_disks();
            if disks.is_empty() {
                c.text(&font::MEDIUM, x, 300, "No disk found. (USB sticks need a USB storage driver, which MiiOS doesn't have yet.)", INK_SOFT);
            } else {
                c.text(&font::MEDIUM, x, 290, "Set up a disk for MiiOS (press its number or click it):", INK_SOFT);
                for (i, (name, mib)) in disks.iter().enumerate() {
                    let r = self.erase_rect(i);
                    c.fill_round_rect(r, 24, WHITE);
                    let mut l = FmtBuf::<96>::new();
                    let _ = write!(l, "{}.  {}  ({} GB)", i + 1, name, mib / 1024);
                    c.text(&font::LARGE, r.x + 28, r.y + 22, l.as_str(), INK);
                    c.text(&font::MEDIUM, r.right() - 260, r.y + 26, "ERASES this disk", hex(0xE0455E));
                }
            }
        }
        c.text(&font::MEDIUM, x, self.h - 40, self.msg.as_str(), INK_SOFT);

        // ---- Confirmation dialog ----
        if self.confirm != Confirm::None {
            let dlg = Rect::new(s.w / 2 - 420, s.h / 2 - 150, 840, 300);
            c.fill_rect(Rect::new(0, 0, s.w, s.h), with_alpha(0x303858, 60));
            c.shadow(dlg, 36, 16, 80);
            c.fill_round_rect(dlg, 36, WHITE);
            match self.confirm {
                Confirm::Delete => {
                    let mut l = FmtBuf::<80>::new();
                    let _ = write!(l, "Delete {}?", self.entries.get(self.sel).map(|e| e.name()).unwrap_or(""));
                    c.text_centered(&font::LARGE, s.w / 2, dlg.y + 70, l.as_str(), INK);
                    c.text_centered(&font::MEDIUM, s.w / 2, dlg.y + 150, "Enter = yes, delete it      Esc = no, keep it", INK_SOFT);
                }
                Confirm::Erase(i) => {
                    let name = fs::other_disks().get(i).map(|d| d.0.clone()).unwrap_or_default();
                    let mut l = FmtBuf::<96>::new();
                    let _ = write!(l, "ERASE {}?", name);
                    c.text_centered(&font::LARGE, s.w / 2, dlg.y + 40, l.as_str(), hex(0xE0455E));
                    c.text_centered(&font::MEDIUM, s.w / 2, dlg.y + 100, "Everything on this disk (Windows, Linux, files...) will be lost.", INK);
                    c.text_centered(&font::MEDIUM, s.w / 2, dlg.y + 140, "Type ERASE and press Enter to continue, or Esc to cancel.", INK_SOFT);
                    let field = Rect::new(s.w / 2 - 150, dlg.y + 190, 300, 64);
                    c.fill_round_rect(field, 20, hex(0xF1F4FA));
                    c.text_centered(&font::LARGE, s.w / 2, field.y + 16, self.typed.as_str(), INK);
                }
                Confirm::None => {}
            }
        }
    }

    fn color(&self) -> Color { PASTELS[4] }
}
