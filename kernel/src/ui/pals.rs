//! "Pals": the avatar maker.
//!
//! Left: your pal, big, on a little stage, plus a row of all your pals.
//! Right: category tabs (Face, Hair, Eyes...), a grid of choices (each one a
//! mini preview of *your* pal with that option), colour swatches, buttons.
//!
//! Keyboard: Tab / Shift+Tab = category, arrows = move (choices apply as
//! you land on them), Enter = press a button, PageUp/PageDown = switch pal,
//! Esc = done. Mouse: hover and click.

use super::anim::{Spring, Tween};
use super::avatar::{self, Avatar, Pose, Rng};
use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cat { Name, Face, Hair, Eyes, Brows, Nose, Mouth, Glasses, Cheeks, Body }
const CATS: [Cat; 10] = [Cat::Name, Cat::Face, Cat::Hair, Cat::Eyes, Cat::Brows, Cat::Nose, Cat::Mouth, Cat::Glasses, Cat::Cheeks, Cat::Body];
const CAT_NAMES: [&str; 10] = ["Name", "Face", "Hair", "Eyes", "Brows", "Nose", "Mouth", "Glasses", "Cheeks", "Body"];
const BUTTONS: [&str; 3] = ["Surprise me!", "New pal", "Done"];
const GRID_COLS: usize = 4;

impl Cat {
    fn options(self) -> u8 {
        match self {
            Cat::Name => 0, Cat::Face => avatar::FACE_STYLES, Cat::Hair => avatar::HAIR_STYLES, Cat::Eyes => avatar::EYE_STYLES,
            Cat::Brows => avatar::BROW_STYLES, Cat::Nose => avatar::NOSE_STYLES, Cat::Mouth => avatar::MOUTH_STYLES,
            Cat::Glasses => avatar::GLASSES_STYLES, Cat::Cheeks => avatar::CHEEK_STYLES, Cat::Body => avatar::BODY_STYLES,
        }
    }
    fn get(self, a: &Avatar) -> u8 {
        match self {
            Cat::Name => 0, Cat::Face => a.face, Cat::Hair => a.hair, Cat::Eyes => a.eyes, Cat::Brows => a.brows,
            Cat::Nose => a.nose, Cat::Mouth => a.mouth, Cat::Glasses => a.glasses, Cat::Cheeks => a.cheeks, Cat::Body => a.body,
        }
    }
    fn set(self, a: &mut Avatar, v: u8) {
        match self {
            Cat::Name => {}, Cat::Face => a.face = v, Cat::Hair => a.hair = v, Cat::Eyes => a.eyes = v, Cat::Brows => a.brows = v,
            Cat::Nose => a.nose = v, Cat::Mouth => a.mouth = v, Cat::Glasses => a.glasses = v, Cat::Cheeks => a.cheeks = v, Cat::Body => a.body = v,
        }
    }
    fn palette(self) -> Option<&'static [Color; 8]> {
        match self {
            Cat::Face => Some(&avatar::SKIN), Cat::Hair | Cat::Brows => Some(&avatar::HAIR), Cat::Eyes => Some(&avatar::EYES),
            Cat::Glasses => Some(&avatar::FRAMES), Cat::Body => Some(&avatar::SHIRT), _ => None,
        }
    }
    fn get_color(self, a: &Avatar) -> u8 {
        match self { Cat::Face => a.skin, Cat::Hair | Cat::Brows => a.hair_color, Cat::Eyes => a.eye_color, Cat::Glasses => a.glasses_color, Cat::Body => a.shirt, _ => 0 }
    }
    fn set_color(self, a: &mut Avatar, v: u8) {
        match self { Cat::Face => a.skin = v, Cat::Hair | Cat::Brows => a.hair_color = v, Cat::Eyes => a.eye_color = v, Cat::Glasses => a.glasses_color = v, Cat::Body => a.shirt = v, _ => {} }
    }
}

/// Which row the keyboard focus is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row { Tabs, Grid, Swatches, Buttons }

struct Lay { left_w: i32, x0: i32, rw: i32, tab_y: i32, grid_y: i32, cell_w: i32, cell_h: i32, sw_y: i32, btn_y: i32, stage_cx: i32, feet_y: i32, body_h: i32 }

fn lay(w: i32, h: i32) -> Lay {
    let left_w = w * 36 / 100;
    let x0 = left_w + 30;
    let rw = w - x0 - 60;
    let gap = 24;
    let cell_w = (rw - gap * (GRID_COLS as i32 - 1)) / GRID_COLS as i32;
    let cell_h = (h * 23 / 100).min(cell_w);
    let grid_y = 216;
    let sw_y = grid_y + 2 * cell_h + gap + 36;
    Lay { left_w, x0, rw, tab_y: 132, grid_y, cell_w, cell_h, sw_y, btn_y: h - 120, stage_cx: left_w / 2 + 20, feet_y: h - 170, body_h: h * 50 / 100 }
}

pub struct Pals {
    cat: usize,
    row: Row,
    col: usize,
    bounce: Spring,
    wave: Tween,
    blink: Tween,
    hover_back: bool,
    rng: Rng,
    lay: Lay,
}

impl Pals {
    pub fn new(w: i32, h: i32) -> Pals {
        Pals { cat: 1, row: Row::Grid, col: 0, bounce: Spring::new(ONE), wave: Tween::default(), blink: Tween::default(),
               hover_back: false, rng: Rng::new(crate::arch::cpu::rdtsc()), lay: lay(w, h) }
    }

    fn c(&self) -> Cat { CATS[self.cat] }

    // ---- geometry ----
    fn tab_rect(&self, i: usize) -> Rect {
        let mut x = self.lay.x0;
        for j in 0..i { x += tab_w(j) + 10; }
        Rect::new(x, self.lay.tab_y, tab_w(i), 52)
    }
    fn cell_rect(&self, i: usize) -> Rect {
        let l = &self.lay;
        let (col, row) = ((i % GRID_COLS) as i32, (i / GRID_COLS) as i32);
        Rect::new(l.x0 + col * (l.cell_w + 24), l.grid_y + row * (l.cell_h + 24), l.cell_w, l.cell_h)
    }
    fn swatch_rect(&self, i: usize) -> Rect { Rect::new(self.lay.x0 + i as i32 * 92, self.lay.sw_y, 72, 72) }
    fn button_rect(&self, i: usize) -> Rect {
        let w = [260, 200, 180][i];
        let mut x = self.lay.x0 + self.lay.rw;
        for j in (i..3).rev() { x -= [260, 200, 180][j] + if j < 2 { 20 } else { 0 }; }
        Rect::new(x, self.lay.btn_y, w, 68)
    }
    fn roster_rect(&self, i: usize) -> Rect { Rect::new(56 + i as i32 * 86, 130, 76, 76) }
    fn stage_rect(&self) -> Rect { Rect::new(0, 220, self.lay.left_w, self.lay.feet_y + 110 - 220) }
    fn grid_area(&self) -> Rect { Rect::new(self.lay.x0 - 20, self.lay.grid_y - 20, self.lay.rw + 40, self.lay.sw_y + 100 - self.lay.grid_y) }

    fn row_len(&self, r: Row) -> usize {
        match r {
            Row::Tabs => CATS.len(),
            Row::Grid => self.c().options() as usize,
            Row::Swatches => if self.c().palette().is_some() { 8 } else { 0 },
            Row::Buttons => 3,
        }
    }

    fn focus_rect(&self) -> Rect {
        match self.row {
            Row::Tabs => self.tab_rect(self.col),
            Row::Grid => self.cell_rect(self.col),
            Row::Swatches => self.swatch_rect(self.col),
            Row::Buttons => self.button_rect(self.col),
        }.inflate(14)
    }

    /// Something about the pal changed: pop the preview and redraw what depends on it.
    fn changed(&mut self, s: &Shared, d: &mut Damage) {
        self.bounce.value = ONE + 18;
        d.add(self.stage_rect());
        d.add(self.grid_area());
        d.add(Rect::new(0, 120, self.lay.left_w, 100));
        let _ = s;
    }

    fn set_cat(&mut self, i: usize, d: &mut Damage) {
        self.cat = i % CATS.len();
        d.add(Rect::new(self.lay.x0 - 20, self.lay.tab_y - 20, self.lay.rw + 40, 92));
        d.add(self.grid_area());
    }

    /// Move the keyboard focus; landing on a choice applies it right away.
    fn move_focus(&mut self, row: Row, col: usize, s: &mut Shared, d: &mut Damage) {
        let len = self.row_len(row);
        if len == 0 { return; }
        d.add(self.focus_rect());
        self.row = row;
        self.col = col.min(len - 1);
        d.add(self.focus_rect());
        self.apply_focus(s, d);
    }

    fn apply_focus(&mut self, s: &mut Shared, d: &mut Damage) {
        let cat = self.c();
        let a = s.pals.current_mut();
        match self.row {
            Row::Tabs => { if self.col != self.cat { self.set_cat(self.col, d); } }
            Row::Grid => { if cat.get(a) != self.col as u8 { cat.set(a, self.col as u8); self.changed(s, d); } }
            Row::Swatches => { if cat.get_color(a) != self.col as u8 { cat.set_color(a, self.col as u8); self.changed(s, d); } }
            Row::Buttons => {}
        }
    }

    fn press_button(&mut self, i: usize, s: &mut Shared, d: &mut Damage) -> Action {
        match i {
            0 => { // Surprise me: random look, keep the name
                let name = *s.pals.current_mut();
                let mut a = Avatar::random(&mut self.rng);
                a.name = name.name; a.name_len = name.name_len;
                *s.pals.current_mut() = a;
                self.changed(s, d);
            }
            1 => { // New pal
                if s.pals.add(Avatar::random(&mut self.rng)) { self.changed(s, d); d.add(Rect::new(0, 110, self.lay.left_w, 110)); }
            }
            _ => return Action::Back,
        }
        Action::None
    }

    fn switch_pal(&mut self, i: usize, s: &mut Shared, d: &mut Damage) {
        if i < s.pals.count && i != s.pals.current {
            s.pals.current = i;
            self.changed(s, d);
            self.wave.start(70);
        }
    }

    fn hit(&self, x: i32, y: i32, s: &Shared) -> Option<(Row, usize)> {
        for i in 0..CATS.len() { if self.tab_rect(i).contains(x, y) { return Some((Row::Tabs, i)); } }
        for i in 0..self.row_len(Row::Grid) { if self.cell_rect(i).contains(x, y) { return Some((Row::Grid, i)); } }
        for i in 0..self.row_len(Row::Swatches) { if self.swatch_rect(i).contains(x, y) { return Some((Row::Swatches, i)); } }
        for i in 0..3 { if self.button_rect(i).contains(x, y) { return Some((Row::Buttons, i)); } }
        let _ = s;
        None
    }
}

fn tab_w(i: usize) -> i32 { CAT_NAMES[i].len() as i32 * 12 + 40 }

impl App for Pals {
    fn enter(&mut self, s: &mut Shared) {
        self.lay = lay(s.w, s.h);
        self.wave.start(80);
    }

    fn event(&mut self, e: &UiEvent, s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key, mods } => {
                // Typing a name.
                if self.c() == Cat::Name {
                    let a = s.pals.current_mut();
                    match key {
                        Key::Char(ch) if ch.is_ascii_graphic() || ch == ' ' => {
                            if (a.name_len as usize) < 12 {
                                let n = a.name_len as usize;
                                a.name[n] = ch as u8;
                                a.name_len += 1;
                                self.changed(s, d);
                            }
                            return Action::None;
                        }
                        Key::Backspace => { if a.name_len > 0 { a.name_len -= 1; self.changed(s, d); } return Action::None; }
                        _ => {}
                    }
                }
                match key {
                    Key::Tab => {
                        let n = CATS.len();
                        let next = if mods.shift { (self.cat + n - 1) % n } else { (self.cat + 1) % n };
                        self.set_cat(next, d);
                        if self.row == Row::Tabs { self.move_focus(Row::Tabs, next, s, d); }
                        else if self.row == Row::Grid || self.row == Row::Swatches {
                            // Keep focus on the current choice of the new category.
                            let a = s.pals.current_mut();
                            let v = self.c().get(a) as usize;
                            d.add(self.focus_rect());
                            if self.row_len(Row::Grid) > 0 { self.row = Row::Grid; self.col = v; } else { self.row = Row::Tabs; self.col = self.cat; }
                            d.add(self.focus_rect());
                        }
                    }
                    Key::Escape => return Action::Back,
                    Key::PageUp => { let i = s.pals.current; if i > 0 { self.switch_pal(i - 1, s, d); } }
                    Key::PageDown => { let i = s.pals.current; self.switch_pal(i + 1, s, d); }
                    Key::Left => { if self.col > 0 { self.move_focus(self.row, self.col - 1, s, d); } }
                    Key::Right => { self.move_focus(self.row, self.col + 1, s, d); }
                    Key::Up | Key::Down => {
                        let up = key == Key::Up;
                        let (row, col) = match (self.row, up) {
                            (Row::Grid, true) if self.col >= GRID_COLS => (Row::Grid, self.col - GRID_COLS),
                            (Row::Grid, true) => (Row::Tabs, self.cat),
                            (Row::Grid, false) if self.col + GRID_COLS < self.row_len(Row::Grid) => (Row::Grid, self.col + GRID_COLS),
                            (Row::Grid, false) => if self.row_len(Row::Swatches) > 0 { (Row::Swatches, self.c().get_color(s.pals.current_mut()) as usize) } else { (Row::Buttons, 2) },
                            (Row::Tabs, false) => if self.row_len(Row::Grid) > 0 { (Row::Grid, self.c().get(s.pals.current_mut()) as usize) } else { (Row::Buttons, 2) },
                            (Row::Tabs, true) => (Row::Tabs, self.col),
                            (Row::Swatches, true) => (Row::Grid, self.c().get(s.pals.current_mut()) as usize),
                            (Row::Swatches, false) => (Row::Buttons, 2),
                            (Row::Buttons, true) => if self.row_len(Row::Swatches) > 0 { (Row::Swatches, 0) } else if self.row_len(Row::Grid) > 0 { (Row::Grid, 0) } else { (Row::Tabs, self.cat) },
                            (Row::Buttons, false) => (Row::Buttons, self.col),
                        };
                        self.move_focus(row, col, s, d);
                    }
                    Key::Enter | Key::Char(' ') => {
                        match self.row {
                            Row::Buttons => return self.press_button(self.col, s, d),
                            Row::Tabs => { let v = self.c().get(s.pals.current_mut()) as usize; if self.row_len(Row::Grid) > 0 { self.move_focus(Row::Grid, v, s, d); } }
                            _ => { self.wave.start(60); }
                        }
                    }
                    _ => {}
                }
            }
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
                if let Some((row, col)) = self.hit(x, y, s) {
                    if (row, col) != (self.row, self.col) {
                        d.add(self.focus_rect());
                        self.row = row; self.col = col;
                        d.add(self.focus_rect());
                    }
                }
            }
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { return Action::Back; }
                for i in 0..s.pals.count { if self.roster_rect(i).contains(x, y) { self.switch_pal(i, s, d); return Action::None; } }
                if self.stage_rect().contains(x, y) { self.wave.start(60); return Action::None; }
                if let Some((row, col)) = self.hit(x, y, s) {
                    self.row = row; self.col = col;
                    if row == Row::Buttons { return self.press_button(col, s, d); }
                    self.apply_focus(s, d);
                }
            }
            UiEvent::Scroll { dy } => {
                let n = CATS.len();
                self.set_cat(if dy > 0 { (self.cat + 1) % n } else { (self.cat + n - 1) % n }, d);
            }
            _ => {}
        }
        Action::None
    }

    fn tick(&mut self, _s: &mut Shared, d: &mut Damage) -> bool {
        let mut moving = false;
        if self.bounce.step() { moving = true; d.add(self.stage_rect()); }
        if self.wave.step() { moving = true; d.add(self.stage_rect()); }
        if self.blink.step() { moving = true; d.add(self.stage_rect()); }
        moving
    }

    fn second(&mut self, s: &mut Shared, d: &mut Damage) {
        if s.local_time % 3 == 0 { self.blink.start(8); d.add(self.stage_rect()); }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        let l = &self.lay;
        widgets::app_background(c, self.color());
        widgets::header(c, "Pals", self.color(), self.hover_back);
        let me = s.pals.current();

        // ---- Roster of pals ----
        for i in 0..s.pals.count {
            let r = self.roster_rect(i);
            if !r.inflate(8).overlaps(&c.clip) { continue; }
            let sel = i == s.pals.current;
            c.fill_circle(r.x + r.w / 2, r.y + r.h / 2, r.w / 2, if sel { ACCENT } else { WHITE });
            c.fill_circle(r.x + r.w / 2, r.y + r.h / 2, r.w / 2 - 4, mix(PASTELS[i % 8], WHITE, 80));
            avatar::draw_head(c, r.x + r.w / 2, r.y + r.h / 2 + 4, r.w * 62 / 100, &s.pals.list[i], Pose::default());
        }

        // ---- Stage + big preview ----
        let st = self.stage_rect();
        if st.overlaps(&c.clip) {
            c.fill_ellipse_fx(fx(l.stage_cx), fx(l.feet_y), fx(l.left_w * 36 / 100), fx(46), mix(self.color(), WHITE, 60));
            c.fill_ellipse_fx(fx(l.stage_cx), fx(l.feet_y - 6), fx(l.left_w * 34 / 100), fx(38), WHITE);
            let pose = Pose { blink: self.blink.running, wave: if self.wave.running { self.wave.t.max(1) } else { 0 }, bounce: 0 };
            let h = fx_round(fx_mul(fx(l.body_h), self.bounce.value));
            avatar::draw_body(c, l.stage_cx, l.feet_y, h, me, pose);
            c.text_centered(&font::LARGE, l.stage_cx, l.feet_y + 56, me.name(), INK);
        }

        // ---- Tabs ----
        for i in 0..CATS.len() {
            let r = self.tab_rect(i);
            if !r.inflate(16).overlaps(&c.clip) { continue; }
            let on = i == self.cat;
            c.fill_round_rect(r, 26, if on { ACCENT } else { WHITE });
            c.text_centered(&font::MEDIUM, r.x + r.w / 2, r.y + 14, CAT_NAMES[i], if on { WHITE } else { INK });
        }

        // ---- Choices ----
        let cat = self.c();
        if cat == Cat::Name {
            let r = Rect::new(l.x0, l.grid_y + 40, l.rw, 120);
            c.fill_round_rect(r, 36, WHITE);
            let end = c.text(&font::HUGE, r.x + 40, r.y + 28, me.name(), INK);
            c.fill_round_rect(Rect::new(end + 4, r.y + 30, 5, 62), 2, ACCENT);
            c.text(&font::MEDIUM, l.x0 + 8, r.bottom() + 30, "Type a name (up to 12 letters). Backspace deletes.", INK_SOFT);
        } else {
            let cur = cat.get(me);
            for i in 0..cat.options() as usize {
                let r = self.cell_rect(i);
                if !r.inflate(16).overlaps(&c.clip) { continue; }
                let chosen = i as u8 == cur;
                c.shadow(r, 28, 6, 40);
                c.fill_round_rect(r, 28, if chosen { mix(self.color(), WHITE, 120) } else { WHITE });
                let mut a = *me;
                cat.set(&mut a, i as u8);
                if cat == Cat::Body {
                    avatar::draw_body(c, r.x + r.w / 2, r.bottom() - 18, r.h * 82 / 100, &a, Pose::default());
                } else {
                    avatar::draw_head(c, r.x + r.w / 2, r.y + r.h / 2 + 6, r.h * 56 / 100, &a, Pose::default());
                }
                if chosen { c.fill_circle(r.right() - 26, r.y + 26, 12, ACCENT); }
            }
        }

        // ---- Colour swatches ----
        if let Some(pal) = cat.palette() {
            let cur = cat.get_color(me) as usize;
            c.text(&font::MEDIUM, l.x0, l.sw_y - 34, "Colour", INK_SOFT);
            for (i, &col) in pal.iter().enumerate() {
                let r = self.swatch_rect(i);
                if !r.inflate(16).overlaps(&c.clip) { continue; }
                let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
                c.fill_circle(cx, cy + 2, r.w / 2, with_alpha(0x303858, 40));
                c.fill_circle(cx, cy, r.w / 2, WHITE);
                c.fill_circle(cx, cy, r.w / 2 - 6, col);
                if i == cur { c.fill_circle(cx, cy, 8, WHITE); }
            }
        }

        // ---- Buttons ----
        for i in 0..3 { widgets::button(c, self.button_rect(i), BUTTONS[i], i == 2); }

        // ---- Keyboard focus ring ----
        let f = self.focus_rect();
        if f.inflate(8).overlaps(&c.clip) {
            let rad = match self.row { Row::Swatches => f.w / 2, Row::Tabs | Row::Buttons => f.h / 2, Row::Grid => 40 };
            c.stroke_round_rect(f.inflate(-4), rad, 5, with_alpha(ACCENT, 200));
        }
        c.text(&font::SMALL, l.x0, s.h - 36, "Tab: category   Arrows: choose   PgUp/PgDn: switch pal   Esc: done", INK_SOFT);
    }

    fn color(&self) -> Color { PASTELS[0] }
}
