//! Milestone 3 test screen: keyboard + mouse. Type text, move the pointer,
//! hover and click the tiles.
use crate::gfx::color::*;
use crate::gfx::{cursor, font, Canvas, Rect};
use crate::input::{Event, Key};

pub struct Demo {
    pub mx: i32, pub my: i32,
    pub buttons: (bool, bool, bool),
    pub text: [u8; 96], pub len: usize,
    pub last_key: [u8; 40], pub last_len: usize,
    pub hover: Option<usize>, pub clicked: Option<usize>,
    pub w: i32, pub h: i32,
}

fn tile(i: usize) -> Rect { Rect::new(64 + i as i32 * 300, 380, 260, 180) }
const NAMES: [&str; 6] = ["Home", "Pals", "Write", "Files", "Clock", "Settings"];

impl Demo {
    pub fn new(w: i32, h: i32) -> Demo {
        Demo { mx: w / 2, my: h / 2, buttons: (false, false, false), text: [0; 96], len: 0,
               last_key: [0; 40], last_len: 0, hover: None, clicked: None, w, h }
    }

    fn status_rect() -> Rect { Rect::new(0, 160, 1920, 200) }

    /// Handle one event; mark what changed on screen in `damage`.
    pub fn handle(&mut self, e: Event, damage: &mut crate::gfx::screen::Damage) {
        match e {
            Event::Key { key, pressed, mods } => {
                if !pressed { return; }
                let mut s = crate::util::FmtBuf::<40>::new();
                let _ = core::fmt::write(&mut s, format_args!("{:?}{}{}", key, if mods.shift { " +shift" } else { "" }, if mods.ctrl { " +ctrl" } else { "" }));
                self.last_len = s.len(); self.last_key[..s.len()].copy_from_slice(s.as_bytes());
                match key {
                    Key::Char(c) if self.len < self.text.len() && c.is_ascii() && !c.is_ascii_control() => { self.text[self.len] = c as u8; self.len += 1; }
                    Key::Backspace if self.len > 0 => self.len -= 1,
                    _ => {}
                }
                damage.add(Self::status_rect());
            }
            Event::Mouse { dx, dy, left, right, middle, .. } => {
                damage.add(cursor::rect(self.mx, self.my));
                // Simple acceleration: fast flicks travel further.
                let accel = |d: i32| if d.abs() > 6 { d * 2 } else { d };
                self.mx = (self.mx + accel(dx)).clamp(0, self.w - 1);
                self.my = (self.my + accel(dy)).clamp(0, self.h - 1);
                damage.add(cursor::rect(self.mx, self.my));
                let hover = (0..6).find(|&i| tile(i).contains(self.mx, self.my));
                if hover != self.hover {
                    for t in [self.hover, hover].into_iter().flatten() { damage.add(tile(t).inflate(16)); }
                    self.hover = hover;
                }
                if left && !self.buttons.0 && hover.is_some() {
                    for t in [self.clicked, hover].into_iter().flatten() { damage.add(tile(t).inflate(16)); }
                    self.clicked = hover;
                }
                if (left, right, middle) != self.buttons { damage.add(Self::status_rect()); }
                self.buttons = (left, right, middle);
            }
        }
    }

    pub fn draw(&self, c: &mut Canvas) {
        c.fill_rect(Rect::new(0, 0, c.width, c.height), BG);
        c.text(&font::LARGE, 64, 48, "Milestone 3: keyboard + mouse", INK);
        c.text(&font::MEDIUM, 64, 96, "Type something. Move the pointer over the tiles and click one.", INK_SOFT);

        c.fill_round_rect(Rect::new(64, 170, 1792, 64), 20, WHITE);
        let typed = core::str::from_utf8(&self.text[..self.len]).unwrap_or("");
        let end = c.text(&font::LARGE, 88, 186, typed, INK);
        c.fill_rect(Rect::new(end + 2, 186, 3, 32), ACCENT);
        let lk = core::str::from_utf8(&self.last_key[..self.last_len]).unwrap_or("");
        c.text(&font::MEDIUM, 64, 260, "last key: ", INK_SOFT);
        c.text(&font::MEDIUM, 64 + 120, 260, lk, INK);
        let b = self.buttons;
        c.text(&font::MEDIUM, 64, 300, "buttons:", INK_SOFT);
        for (i, (on, name)) in [(b.0, "left"), (b.2, "middle"), (b.1, "right")].iter().enumerate() {
            let r = Rect::new(184 + i as i32 * 130, 294, 118, 36);
            c.fill_round_rect(r, 18, if *on { ACCENT } else { WHITE });
            c.text_centered(&font::MEDIUM, r.x + r.w / 2, r.y + 6, name, if *on { WHITE } else { INK_SOFT });
        }

        for i in 0..6 {
            let r = tile(i);
            let lift = if self.hover == Some(i) { 6 } else { 0 };
            let r = r.offset(0, -lift);
            c.shadow(r, 36, 10 + lift, 70);
            c.fill_round_rect(r, 36, WHITE);
            c.fill_round_rect(r.inflate(-14), 26, PASTELS[i]);
            c.text_centered(&font::MEDIUM, r.x + r.w / 2, r.bottom() - 52, NAMES[i], INK);
            if self.clicked == Some(i) { c.stroke_round_rect(r.inflate(10), 44, 6, ACCENT); }
        }
        cursor::draw(c, self.mx, self.my);
    }
}
