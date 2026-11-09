//! Wi-Fi: see nearby networks, type a password, connect.
//!
//! The actual work is done by Linux's NetworkManager (see `host`); this app
//! just shows the list and asks for passwords. Scanning and connecting run in
//! the background, so the screen never freezes while waiting.

extern crate alloc;
use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::fx;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::host::{self, Network};
use crate::input::Key;
use alloc::string::String;
use alloc::vec::Vec;

const ROW_H: i32 = 72;

pub struct Wifi {
    nets: Vec<Network>,
    sel: usize,
    scanning: bool,
    since_scan: u32,
    asking: Option<String>, // SSID we're asking a password for
    password: String,
    show_pw: bool,
    connecting: Option<String>,
    msg: String,
    hover_back: bool,
    w: i32,
    h: i32,
}

impl Wifi {
    pub fn new(w: i32, h: i32) -> Wifi {
        Wifi { nets: Vec::new(), sel: 0, scanning: false, since_scan: 0, asking: None, password: String::new(),
               show_pw: false, connecting: None, msg: String::new(), hover_back: false, w, h }
    }
    fn x(&self) -> i32 { (self.w - 1100) / 2 }
    fn row(&self, i: usize) -> Rect { Rect::new(self.x(), 290 + i as i32 * (ROW_H + 10), 1100, ROW_H) }
    fn max_rows(&self) -> usize { ((self.h - 400) / (ROW_H + 10)).max(1) as usize }
    fn all(&self) -> Rect { Rect::new(0, 110, self.w, self.h - 110) }

    fn scan(&mut self) { host::wifi_scan_start(); self.scanning = true; self.since_scan = 0; }

    fn choose(&mut self, i: usize) {
        let Some(n) = self.nets.get(i).cloned() else { return };
        if n.connected { self.msg = String::from("Already connected."); return; }
        if n.secure { self.asking = Some(n.ssid); self.password.clear(); }
        else { self.connect(n.ssid, String::new()); }
    }

    fn connect(&mut self, ssid: String, pw: String) {
        host::wifi_connect_start(&ssid, &pw);
        self.msg = alloc::format!("Connecting to {}...", ssid);
        self.connecting = Some(ssid);
        self.asking = None;
        self.password.clear();
    }
}

impl App for Wifi {
    fn enter(&mut self, s: &mut Shared) {
        self.w = s.w; self.h = s.h;
        self.msg.clear();
        self.scan();
    }

    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        d.add(self.all());
        if let Some(ssid) = self.asking.clone() {
            if let UiEvent::Key { key, .. } = *e {
                match key {
                    Key::Escape => { self.asking = None; self.password.clear(); }
                    Key::Enter => { let pw = core::mem::take(&mut self.password); self.connect(ssid, pw); }
                    Key::Tab => self.show_pw = !self.show_pw,
                    Key::Backspace => { self.password.pop(); }
                    Key::Char(c) if self.password.len() < 63 => self.password.push(c),
                    _ => {}
                }
            }
            return Action::None;
        }
        match *e {
            UiEvent::Key { key, .. } => match key {
                Key::Escape => return Action::Back,
                Key::Up => { if self.sel > 0 { self.sel -= 1; } }
                Key::Down => { if self.sel + 1 < self.nets.len().min(self.max_rows()) { self.sel += 1; } }
                Key::Enter => self.choose(self.sel),
                Key::Char('r') => self.scan(),
                _ => {}
            },
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { return Action::Back; }
                for i in 0..self.nets.len().min(self.max_rows()) {
                    if self.row(i).contains(x, y) { self.sel = i; self.choose(i); }
                }
            }
            UiEvent::PointerMove { x, y } => {
                self.hover_back = widgets::back_button().contains(x, y);
                for i in 0..self.nets.len().min(self.max_rows()) { if self.row(i).contains(x, y) { self.sel = i; } }
            }
            _ => {}
        }
        Action::None
    }

    fn second(&mut self, _s: &mut Shared, d: &mut Damage) {
        if let Some(n) = host::wifi_networks() {
            self.nets = n;
            self.scanning = false;
            if self.sel >= self.nets.len() { self.sel = 0; }
            d.add(self.all());
        }
        if self.connecting.is_some() {
            if let Some(r) = host::wifi_connect_result() {
                let ssid = self.connecting.take().unwrap_or_default();
                self.msg = match r { Ok(()) => alloc::format!("Connected to {}!", ssid), Err(e) => alloc::format!("Couldn't connect: {}", e) };
                self.scan();
            }
            d.add(self.all());
        }
        self.since_scan += 1;
        if self.since_scan >= 15 && self.asking.is_none() && self.connecting.is_none() { self.scan(); }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Wi-Fi", self.color(), self.hover_back);
        let x = self.x();
        let card = Rect::new(x, 140, 1100, 110);
        c.fill_round_rect(card, 28, WHITE);
        super::icons::wifi(c, card.x + 60, card.y + 52, 70);
        match host::wifi_current() {
            Some(ssid) => {
                c.text(&font::LARGE, card.x + 120, card.y + 20, &ssid, INK);
                c.text(&font::MEDIUM, card.x + 120, card.y + 62, "Connected", hex(0x2BB7A3));
            }
            None => {
                c.text(&font::LARGE, card.x + 120, card.y + 20, "Not connected", INK);
                c.text(&font::MEDIUM, card.x + 120, card.y + 62, "Pick a network below.", INK_SOFT);
            }
        }
        if self.scanning && self.nets.is_empty() {
            c.text_centered(&font::LARGE, s.w / 2, 340, "Looking for networks...", INK_SOFT);
        } else if let Some(p) = host::wifi_problem() {
            c.text_centered(&font::LARGE, s.w / 2, 340, &p, hex(0xE0455E));
            c.text_centered(&font::MEDIUM, s.w / 2, 390, "Press R to try again.", INK_SOFT);
        } else if self.nets.is_empty() {
            c.text_centered(&font::LARGE, s.w / 2, 340, "No networks found. Press R to look again.", INK_SOFT);
        }
        for (i, n) in self.nets.iter().take(self.max_rows()).enumerate() {
            let r = self.row(i);
            if !r.inflate(8).overlaps(&c.clip) { continue; }
            let sel = i == self.sel;
            c.fill_round_rect(r, 22, if sel { mix(self.color(), WHITE, 90) } else { WHITE });
            if sel { c.stroke_round_rect(r, 22, 4, ACCENT); }
            // signal bars
            let bars = (n.signal as i32 + 24) / 25;
            for b in 0..4 {
                let bh = 10 + b * 9;
                c.fill_round_rect(Rect::new(r.x + 30 + b * 13, r.y + 52 - bh, 9, bh), 3, if b < bars { INK } else { hex(0xD5DAE6) });
            }
            c.text(&font::LARGE, r.x + 100, r.y + 20, &n.ssid, INK);
            let mut tx = r.right() - 40;
            if n.secure {
                // a little padlock
                let (lx, ly) = (tx - 14, r.y + 26);
                c.ring_fx(fx(lx + 10), fx(ly + 8), fx(7), fx(4), INK_SOFT);
                c.fill_round_rect(Rect::new(lx, ly + 10, 20, 16), 4, INK_SOFT);
                tx -= 50;
            }
            if n.connected { c.text(&font::MEDIUM, tx - 120, r.y + 24, "Connected", hex(0x2BB7A3)); }
        }
        c.text(&font::MEDIUM, x, self.h - 70, &self.msg, INK_SOFT);
        c.text(&font::SMALL, x, self.h - 36, "Up/Down: choose   Enter: connect   R: scan again   Esc: back", INK_SOFT);

        if let Some(ssid) = &self.asking {
            let dlg = Rect::new(s.w / 2 - 440, s.h / 2 - 160, 880, 320);
            c.fill_rect(Rect::new(0, 0, s.w, s.h), with_alpha(0x303858, 60));
            c.shadow(dlg, 36, 16, 80);
            c.fill_round_rect(dlg, 36, WHITE);
            c.text_centered(&font::LARGE, s.w / 2, dlg.y + 40, &alloc::format!("Password for {}", ssid), INK);
            let field = Rect::new(dlg.x + 60, dlg.y + 110, dlg.w - 120, 72);
            c.fill_round_rect(field, 22, hex(0xF1F4FA));
            let shown: String = if self.show_pw { self.password.clone() } else { "*".repeat(self.password.len()) };
            let end = c.text(&font::LARGE, field.x + 24, field.y + 20, &shown, INK);
            c.fill_rect(Rect::new(end + 2, field.y + 18, 3, 36), ACCENT);
            c.text_centered(&font::MEDIUM, s.w / 2, dlg.y + 220, "Enter: connect    Tab: show/hide    Esc: cancel", INK_SOFT);
        }
    }

    fn color(&self) -> Color { PASTELS[7] }
}
