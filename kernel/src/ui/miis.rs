//! "Miis" (Linux edition): your 3D Miis. See them up close, pick which one is
//! you, remove one, or add new ones: pick a .glb file from this computer
//! (Downloads, Desktop, Documents) or a USB stick. Miis you download in the
//! web browser (e.g. from the Mii Creator website) move in by themselves.

extern crate alloc;
use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::fx;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::host::{self, MiiPose};
use crate::input::Key;
use super::moves::Mv;
use alloc::string::String;
use alloc::vec::Vec;

const COLS: usize = 4;
const WAVE: i32 = 150; // frames one hello-wave lasts
const BUTTONS: [&str; 4] = ["Add a Mii file", "This is me", "Remove", "Make a Mii online"];
const USB: &str = "usb"; // the picker's last row: look on USB sticks
const ROWS: usize = 9;    // picker rows shown at once

pub struct Miis {
    names: Vec<String>,
    sel: usize,
    btn: Option<usize>,
    yaw: i32,
    wave: i32,
    t: i32,
    confirm_remove: bool,
    importing: bool,
    picker: Option<(Vec<(String, String)>, usize)>, // (label, path) and the selected row
    msg: String,
    hover_back: bool,
    w: i32,
    h: i32,
}

impl Miis {
    pub fn new(w: i32, h: i32) -> Miis {
        Miis { names: Vec::new(), sel: 0, btn: None, yaw: 0, wave: 0, t: 0, confirm_remove: false, importing: false, picker: None,
               msg: String::new(), hover_back: false, w, h }
    }
    fn stage(&self) -> Rect { Rect::new(40, 120, self.w * 34 / 100, self.h - 260) }
    fn x0(&self) -> i32 { self.stage().right() + 40 }
    fn card_w(&self) -> i32 { (self.w - self.x0() - 60 - 3 * 20) / COLS as i32 }
    fn card(&self, i: usize) -> Rect {
        let cw = self.card_w();
        let ch = cw * 5 / 4;
        Rect::new(self.x0() + (i % COLS) as i32 * (cw + 20), 140 + (i / COLS) as i32 * (ch + 20), cw, ch)
    }
    fn button(&self, i: usize) -> Rect {
        let bw = (self.w - self.x0() - 60 - 3 * 20) / 4;
        Rect::new(self.x0() + i as i32 * (bw + 20), self.h - 120, bw, 64)
    }
    fn all(&self) -> Rect { Rect::new(0, 110, self.w, self.h - 110) }
    fn panel(&self) -> Rect { let (w, h) = (1000.min(self.w - 80), 150 + ROWS as i32 * 64 + 90); Rect::new((self.w - w) / 2, (self.h - h) / 2 + 30, w, h) }
    fn row(&self, k: usize) -> Rect { let p = self.panel(); Rect::new(p.x + 30, p.y + 130 + k as i32 * 64, p.w - 60, 56) }
    /// First row shown (the list scrolls to keep the selected row visible).
    fn first_row(&self) -> usize { self.picker.as_ref().map_or(0, |(_, sel)| sel.saturating_sub(ROWS - 1)) }

    /// Add the picked file (or look on USB sticks).
    fn pick(&mut self, d: &mut Damage) {
        let Some((files, sel)) = self.picker.take() else { return };
        d.add(self.all());
        let Some((_, path)) = files.get(sel) else { return };
        if path == USB {
            host::mii_import_start();
            self.importing = true;
            self.msg = String::from("Looking on USB sticks...");
            return;
        }
        match host::mii_add_file(path) {
            Ok(m) => {
                host::sound(host::Sfx::Cheer, 256);
                self.names = host::miis();
                self.sel = self.names.len().saturating_sub(1);
                self.wave = WAVE;
                self.msg = m;
            }
            Err(e) => self.msg = e,
        }
    }

    fn picker_event(&mut self, e: &UiEvent, d: &mut Damage) -> Action {
        d.add(self.panel().inflate(20));
        let n = self.picker.as_ref().map_or(0, |p| p.0.len());
        match *e {
            UiEvent::Key { key, .. } => match key {
                Key::Escape | Key::Backspace => { self.picker = None; d.add(self.all()); }
                Key::Up => if let Some((_, s)) = self.picker.as_mut() { *s = s.saturating_sub(1); },
                Key::Down => if let Some((_, s)) = self.picker.as_mut() { *s = (*s + 1).min(n.saturating_sub(1)); },
                Key::Enter | Key::Char(' ') => self.pick(d),
                _ => {}
            },
            UiEvent::Click { x, y } => {
                let first = self.first_row();
                if !self.panel().contains(x, y) { self.picker = None; d.add(self.all()); return Action::None; }
                for k in 0..ROWS.min(n - first) {
                    if self.row(k).contains(x, y) { if let Some((_, s)) = self.picker.as_mut() { *s = first + k; } self.pick(d); break; }
                }
            }
            UiEvent::PointerMove { x, y } => {
                let first = self.first_row();
                for k in 0..ROWS.min(n.saturating_sub(first)) {
                    if self.row(k).contains(x, y) { if let Some((_, s)) = self.picker.as_mut() { *s = first + k; } }
                }
            }
            UiEvent::Scroll { dy } => if let Some((_, s)) = self.picker.as_mut() { *s = ((*s as i32 + dy.signum()).max(0) as usize).min(n.saturating_sub(1)); },
            _ => {}
        }
        Action::None
    }

    fn draw_picker(&self, c: &mut Canvas) {
        let Some((files, sel)) = &self.picker else { return };
        let p = self.panel();
        c.fill_rect(Rect::new(0, 0, self.w, self.h), with_alpha(0x2A3550, 70));
        c.shadow(p, 32, 10, 60);
        c.fill_round_rect(p, 32, WHITE);
        c.text(&font::LARGE, p.x + 34, p.y + 30, "Add a Mii", INK);
        c.text(&font::MEDIUM, p.x + 34, p.y + 82, "Pick a .glb Mii file from this computer:", INK_SOFT);
        let first = self.first_row();
        for k in 0..ROWS.min(files.len() - first) {
            let i = first + k;
            let r = self.row(k);
            if i == *sel { c.fill_round_rect(r, 18, mix(self.color(), WHITE, 110)); c.stroke_round_rect(r, 18, 3, ACCENT); }
            c.text(&font::MEDIUM, r.x + 20, r.y + 14, &files[i].0, if files[i].1 == USB { ACCENT_DARK } else { INK });
        }
        if files.len() == 1 {
            c.text(&font::MEDIUM, p.x + 34, p.y + 130 + 80, "No Mii files found in Downloads, Desktop or Documents.", INK_SOFT);
        }
        c.text(&font::MEDIUM, p.x + 34, p.bottom() - 76, "Tip: Miis you download in the web browser (e.g. from the Mii Creator)", INK_SOFT);
        c.text(&font::MEDIUM, p.x + 34, p.bottom() - 46, "move in by themselves. Esc closes this list.", INK_SOFT);
    }

    fn press(&mut self, i: usize, d: &mut Damage) -> Action {
        d.add(self.all());
        self.confirm_remove = self.confirm_remove && i == 2;
        match i {
            0 => {
                let mut files = host::mii_files();
                files.push((String::from("Look on a USB stick..."), String::from(USB)));
                self.picker = Some((files, 0));
                self.msg.clear();
            }
            1 => if let Some(n) = self.names.get(self.sel) { host::mii_set_me(self.sel); self.msg = alloc::format!("Hi, {}! The home screen will greet you.", n); },
            2 => if let Some(n) = self.names.get(self.sel).cloned() {
                if self.confirm_remove {
                    host::mii_remove(self.sel);
                    self.names = host::miis();
                    self.sel = self.sel.min(self.names.len().saturating_sub(1));
                    self.msg = alloc::format!("{} moved out. (The file is kept in ~/MiiOS/miis/.removed)", n);
                    self.confirm_remove = false;
                } else {
                    self.confirm_remove = true;
                    self.msg = alloc::format!("Remove {}? Press Remove again to confirm.", n);
                }
            },
            _ => return Action::Launch("mii-creator"),
        }
        Action::None
    }
}

impl App for Miis {
    fn enter(&mut self, s: &mut Shared) {
        self.w = s.w; self.h = s.h;
        host::miis_rescan();
        self.names = host::miis();
        self.sel = self.sel.min(self.names.len().saturating_sub(1));
        self.msg.clear();
        self.wave = WAVE;
    }

    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        if self.picker.is_some() { return self.picker_event(e, d); }
        let n = self.names.len();
        match *e {
            UiEvent::Key { key, .. } => {
                d.add(self.all());
                match key {
                    Key::Escape => return Action::Back,
                    Key::Left => { if let Some(b) = self.btn { self.btn = Some(b.saturating_sub(1)); } else if self.sel > 0 { self.sel -= 1; self.wave = WAVE; } }
                    Key::Right => { if let Some(b) = self.btn { self.btn = Some((b + 1).min(3)); } else if self.sel + 1 < n { self.sel += 1; self.wave = WAVE; } }
                    Key::Up => { if self.btn.is_some() { self.btn = None; } else if self.sel >= COLS { self.sel -= COLS; } }
                    Key::Down => { if self.sel + COLS < n && self.btn.is_none() { self.sel += COLS; } else { self.btn = Some(self.btn.unwrap_or(0)); } }
                    Key::Enter | Key::Char(' ') => return match self.btn { Some(b) => self.press(b, d), None => { self.wave = WAVE; Action::None } },
                    Key::Char('i') => return self.press(0, d),
                    Key::Char('m') => return self.press(1, d),
                    Key::Delete => return self.press(2, d),
                    _ => {}
                }
            }
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { return Action::Back; }
                d.add(self.all());
                for i in 0..n { if self.card(i).contains(x, y) { self.sel = i; self.btn = None; self.wave = WAVE; } }
                if self.stage().contains(x, y) { self.wave = WAVE; }
                for i in 0..4 { if self.button(i).contains(x, y) { return self.press(i, d); } }
            }
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
            }
            _ => {}
        }
        Action::None
    }

    /// The big Mii slowly turns around (and waves when you pick it).
    fn tick(&mut self, _s: &mut Shared, d: &mut Damage) -> bool {
        if self.names.is_empty() { return false; }
        self.t += 1;
        if self.wave > 0 { self.wave -= 1; } else if self.t % 2 == 0 { self.yaw = (self.yaw + 1) & 255; }
        if self.wave > 0 && self.yaw != 0 { self.yaw = (self.yaw + if self.yaw < 128 { -8 } else { 8 }).clamp(0, 255) % 256; if self.yaw < 8 || self.yaw > 248 { self.yaw = 0; } }
        d.add(self.stage());
        if host::miis_rendering() { d.add(self.all()); } // small pictures still arriving
        true
    }

    fn second(&mut self, _s: &mut Shared, d: &mut Damage) {
        if self.importing {
            if let Some(m) = host::mii_import_result() {
                self.importing = false;
                self.msg = m;
                self.picker = None;
                host::miis_rescan();
                self.names = host::miis();
                d.add(self.all());
            }
        }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Miis", self.color(), self.hover_back);
        let st = self.stage();
        let me = host::mii_me();
        if self.names.is_empty() {
            c.text_centered(&font::LARGE, s.w / 2, s.h / 2 - 60, "No Miis yet!", INK);
            c.text_centered(&font::MEDIUM, s.w / 2, s.h / 2 - 10, "Press \"Make a Mii online\" and download it as a 3D model (.glb):", INK_SOFT);
            c.text_centered(&font::MEDIUM, s.w / 2, s.h / 2 + 24, "it moves in by itself. Or press \"Add a Mii file\".", INK_SOFT);
        } else {
            // ---- Stage with the selected Mii ----
            if st.inflate(10).overlaps(&c.clip) {
                c.fill_ellipse_fx(fx(st.x + st.w / 2), fx(st.bottom() - 30), fx(st.w * 40 / 100), fx(44), mix(self.color(), WHITE, 60));
                c.fill_ellipse_fx(fx(st.x + st.w / 2), fx(st.bottom() - 36), fx(st.w * 38 / 100), fx(36), WHITE);
                let mh = st.h - 120;
                let r = Rect::new(st.x + st.w / 2 - mh * 3 / 8, st.bottom() - 40 - mh, mh * 3 / 4, mh);
                let f = if self.wave > 0 { super::moves::eval(Mv::Wave, WAVE - self.wave, WAVE) } else { super::moves::eval(Mv::Stand, self.t, 240) };
                let mut pose = f.pose;
                pose.yaw = self.yaw;
                host::mii_draw(c, self.sel, pose, r);
                if let Some(n) = self.names.get(self.sel) {
                    c.text_centered(&font::HUGE, st.x + st.w / 2, st.bottom() + 6, n, INK);
                }
                if me == Some(self.sel) { c.text_centered(&font::MEDIUM, st.x + st.w / 2, st.y + 4, "This is you!", ACCENT_DARK); }
            }
            // ---- All your Miis ----
            for i in 0..self.names.len().min(8) {
                let r = self.card(i);
                if !r.inflate(12).overlaps(&c.clip) { continue; }
                let sel = i == self.sel && self.btn.is_none();
                c.shadow(r, 26, 6, 40);
                c.fill_round_rect(r, 26, if sel { mix(self.color(), WHITE, 110) } else { WHITE });
                if sel { c.stroke_round_rect(r.inflate(6), 32, 5, ACCENT); }
                let ph = r.h - 50;
                host::mii_draw(c, i, MiiPose::default(), Rect::new(r.x + r.w / 2 - ph * 3 / 8, r.y + 8, ph * 3 / 4, ph));
                let label = if me == Some(i) { alloc::format!("{} (you)", self.names[i]) } else { self.names[i].clone() };
                c.text_centered(&font::MEDIUM, r.x + r.w / 2, r.bottom() - 38, &label, INK);
            }
        }
        for i in 0..4 { widgets::button(c, self.button(i), BUTTONS[i], self.btn == Some(i)); }
        c.text(&font::MEDIUM, self.x0(), self.h - 40, &self.msg, INK_SOFT);
        self.draw_picker(c);
    }

    fn color(&self) -> Color { PASTELS[0] }
}
