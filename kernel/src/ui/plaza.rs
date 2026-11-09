//! The plaza: your 3D Miis living along the bottom of the home screen.
//!
//! Each Mii has a tiny "mind" (a state machine):
//!   * **Idle**: stands and faces you. Once a second it may decide to walk
//!     somewhere, wave, or greet a Mii standing nearby.
//!   * **Walk**: turns towards where it's going, walks there (legs and arms
//!     swinging), then turns back to face you.
//! Click a Mii and it spins round to face you, hops, waves and says hi.
//!
//! Battery-friendly: standing Miis need no animation at all, so decisions
//! happen in the once-a-second tick; only moving Miis run at 60 frames/s.
//! The pictures come from the 3D renderer via `crate::host::mii_draw`.

extern crate alloc;
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Rect};
use crate::host::{self, MiiPose};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const LINES: [&str; 8] = ["Hi, {me}!", "Hey {me}! Good to see you!", "What are we doing today?", "I like it here!",
                          "Let's play in Mii Hub!", "Nice day, isn't it?", "Hehe!", "Wanna hang out, {me}?"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum State { Idle, Walk }

struct Walker {
    mii: usize,
    x: i32,          // centre, in 1/256 pixel
    target: i32,     // pixel x it walks to
    yaw: i32,        // facing, 1/256 turns (0 = you, 64 = right, 192 = left)
    yaw_to: i32,
    state: State,
    walk: i32,       // step cycle phase
    wave: i32,       // frames of waving left
    hop: i32,        // frames of jump left
    say: Option<(String, i32)>, // speech bubble + frames left
}

pub struct Plaza {
    walkers: Vec<Walker>,
    names: Vec<String>,
    rng: u64,
    w: i32,
    floor: i32,
    size: i32,
}

/// Shortest signed difference between two angles (1/256 turns).
fn ang_diff(from: i32, to: i32) -> i32 { (to - from + 128).rem_euclid(256) - 128 }

impl Plaza {
    pub fn new(seed: u64) -> Plaza { Plaza { walkers: Vec::new(), names: Vec::new(), rng: seed | 1, w: 0, floor: 0, size: 0 } }

    fn rand(&mut self, n: u32) -> i32 {
        let mut x = self.rng; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.rng = x;
        (x % n.max(1) as u64) as i32
    }

    pub fn active(&self) -> bool { !self.walkers.is_empty() }

    /// Lay out for the screen and the current list of Miis.
    pub fn sync(&mut self, w: i32, floor: i32, size: i32) {
        let names = host::miis();
        if names == self.names && w == self.w && size == self.size { return; }
        self.w = w; self.floor = floor; self.size = size;
        let n = names.len().min(8);
        self.names = names;
        self.walkers.clear();
        for i in 0..n {
            // Spread out, but keep the middle (the clock) a little clearer.
            let slot = (i as i32 * 2 + 1) * w / (n as i32 * 2);
            let x = if (slot - w / 2).abs() < w / 10 { slot + if slot < w / 2 { -w / 8 } else { w / 8 } } else { slot };
            self.walkers.push(Walker { mii: i, x: x << 8, target: x, yaw: 0, yaw_to: 0, state: State::Idle, walk: 0, wave: 0, hop: 0, say: None });
        }
    }

    fn sprite_w(&self) -> i32 { self.size * 3 / 4 }
    fn rect(&self, k: usize) -> Rect {
        let wk = &self.walkers[k];
        let hop = if wk.hop > 0 { let t = 24 - wk.hop; (t * (24 - t)) * self.size / 1200 } else { 0 };
        Rect::new((wk.x >> 8) - self.sprite_w() / 2, self.floor - self.size - hop, self.sprite_w(), self.size)
    }
    fn bubble(&self, k: usize) -> Option<Rect> {
        let (text, _) = self.walkers[k].say.as_ref()?;
        let r = self.rect(k);
        let tw = font::MEDIUM.text_width(text) as i32 + 40;
        let x = (r.x + r.w / 2 - tw / 2).clamp(8, self.w - tw - 8);
        Some(Rect::new(x, r.y - 30, tw, 52))
    }
    /// Everything walker `k` covers on screen right now.
    fn area(&self, k: usize) -> Rect {
        let r = self.rect(k).inflate(4);
        match self.bubble(k) { Some(b) => r.union(&b.inflate(4)), None => r }
    }

    pub fn me_name(&self) -> Option<String> { host::mii_me().and_then(|i| self.names.get(i).cloned()) }

    fn line(&mut self) -> String {
        let me = self.me_name().unwrap_or_else(|| String::from("friend"));
        let l = LINES[self.rand(LINES.len() as u32) as usize];
        l.replace("{me}", &me)
    }

    /// Wave hello to you (e.g. when you come back to the home screen).
    pub fn greet_all(&mut self, d: &mut Damage) {
        for k in 0..self.walkers.len() {
            d.add(self.area(k));
            let w = &mut self.walkers[k];
            w.yaw_to = 0;
            w.wave = 70 + (k as i32 * 9);
        }
    }

    /// A click: did it land on a Mii? If so, that Mii greets you.
    pub fn click(&mut self, x: i32, y: i32, d: &mut Damage) -> bool {
        // Front-most first (later ones are drawn on top).
        for k in (0..self.walkers.len()).rev() {
            if !self.rect(k).contains(x, y) { continue; }
            d.add(self.area(k));
            let text = self.line();
            let w = &mut self.walkers[k];
            w.state = State::Idle;
            w.target = w.x >> 8;
            w.yaw_to = 0;
            w.hop = 24;
            w.wave = 90;
            w.say = Some((text, 200));
            return true;
        }
        false
    }

    /// Once a second: idle Miis decide what to do next.
    pub fn second(&mut self, d: &mut Damage) {
        let n = self.walkers.len();
        for k in 0..n {
            if self.walkers[k].state != State::Idle || self.walkers[k].wave > 0 || self.walkers[k].hop > 0 { continue; }
            let roll = self.rand(100);
            if roll < 14 {
                // Walk somewhere.
                let margin = self.sprite_w() / 2 + 10;
                let to = margin + self.rand((self.w - 2 * margin).max(1) as u32);
                let w = &mut self.walkers[k];
                if (to - (w.x >> 8)).abs() > 60 {
                    w.target = to;
                    w.state = State::Walk;
                    w.yaw_to = if to > (w.x >> 8) { 64 } else { 192 };
                    d.add(self.area(k));
                }
            } else if roll < 17 {
                // Say hi to a Mii standing nearby.
                let me_x = self.walkers[k].x >> 8;
                if let Some(o) = (0..n).find(|&o| o != k && ((self.walkers[o].x >> 8) - me_x).abs() < self.sprite_w() * 2) {
                    let other = self.names.get(self.walkers[o].mii).cloned().unwrap_or_default();
                    let face = if (self.walkers[o].x >> 8) > me_x { 64 } else { 192 };
                    d.add(self.area(k));
                    let w = &mut self.walkers[k];
                    w.yaw_to = face;
                    w.wave = 50;
                    w.say = Some((format!("Hi, {}!", other), 150));
                }
            } else if roll < 19 {
                d.add(self.area(k));
                self.walkers[k].wave = 60; // a friendly wave at you
            }
        }
    }

    /// One 60 Hz animation step. True while anything moves.
    pub fn tick(&mut self, d: &mut Damage) -> bool {
        let mut moving = false;
        for k in 0..self.walkers.len() {
            let before = self.area(k);
            let mut changed = false;
            {
                let w = &mut self.walkers[k];
                if w.state == State::Walk {
                    let dx = (w.target << 8) - w.x;
                    let step = 2 << 8; // 2 pixels per frame
                    if dx.abs() <= step { w.x = w.target << 8; w.state = State::Idle; w.yaw_to = 0; }
                    else { w.x += step * dx.signum(); }
                    w.walk = (w.walk + 9) & 255;
                    changed = true;
                }
                let dy = ang_diff(w.yaw, w.yaw_to);
                if dy != 0 { w.yaw = (w.yaw + dy.clamp(-10, 10)).rem_euclid(256); changed = true; }
                if w.wave > 0 { w.wave -= 1; changed = true; }
                if w.hop > 0 { w.hop -= 1; changed = true; }
                if let Some((_, t)) = &mut w.say {
                    *t -= 1;
                    if *t <= 0 { w.say = None; changed = true; }
                }
                // After greeting another Mii, look back at you.
                if w.state == State::Idle && w.wave == 0 && w.say.is_none() && w.yaw_to != 0 { w.yaw_to = 0; changed = true; }
            }
            if changed {
                moving = true;
                d.add(before);
                d.add(self.area(k));
            }
            if self.walkers[k].say.is_some() { moving = true; }
        }
        moving
    }

    pub fn draw(&self, c: &mut Canvas) {
        // Draw back-to-front by x so overlapping Miis look tidy.
        let mut order: Vec<usize> = (0..self.walkers.len()).collect();
        order.sort_by_key(|&k| self.walkers[k].x);
        for &k in &order {
            let w = &self.walkers[k];
            let r = self.rect(k);
            if !self.area(k).overlaps(&c.clip) { continue; }
            // soft shadow on the ground
            c.fill_ellipse_fx(crate::gfx::math::fx(r.x + r.w / 2), crate::gfx::math::fx(self.floor - 2),
                              crate::gfx::math::fx(r.w / 3), crate::gfx::math::fx(self.size / 30 + 3), with_alpha(0x303858, 40));
            let pose = MiiPose {
                yaw: w.yaw,
                walk: if w.state == State::Walk { Some(w.walk) } else { None },
                wave: if w.wave > 0 { Some((90 - w.wave) * 6) } else { None },
            };
            host::mii_draw(c, w.mii, pose, r);
            if let (Some(b), Some((text, _))) = (self.bubble(k), &w.say) {
                c.shadow(b, 22, 6, 50);
                c.fill_round_rect(b, 22, WHITE);
                let tip_x = (r.x + r.w / 2).clamp(b.x + 24, b.right() - 24);
                c.line(tip_x - 10, b.bottom() - 2, tip_x, b.bottom() + 12, 6, WHITE);
                c.text(&font::MEDIUM, b.x + 20, b.y + 14, text, INK);
            }
        }
    }
}
