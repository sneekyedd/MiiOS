//! The plaza: your 3D Miis living all over the home screen.
//!
//! The screen is treated as a floor seen from above at an angle ("2.5D"):
//! the app tiles are low stages standing on it, and a Mii lower on the
//! screen is closer to you (so it's drawn in front). Miis can:
//!   * walk anywhere, finding a path around the tiles (a grid search),
//!   * climb up onto a tile, walk and dance on top, sit on its front edge
//!     swinging their legs, nap on it, leap to the next tile, scale its side,
//!     and jump off (with real gravity),
//!   * decorate the tiles (flowers, balloons, gifts, stars...), which are kept,
//!   * play with each other (chat, high fives, hugs, tag, catch, dancing...),
//!   * react to you: look at the pointer, greet you when clicked, giggle when
//!     tickled, get picked up and dropped, cheer when you open an app.
//!
//! Each Mii has a queue of small *steps* ("walk there", "climb", "do this
//! move for 2 s", "say this"). A *behaviour* is a recipe that fills the queue;
//! idle Miis pick a new behaviour every few seconds (see `BEHAVIOURS`).
//! Every step plays a *move* from `moves.rs` (80 of them).
//!
//! Battery-friendly: a Mii standing still needs no redraw; only things that
//! change are redrawn; the 3D pictures are made on other CPU threads.

extern crate alloc;
use super::moves::{eval, Mv};
use crate::gfx::color::*;
use crate::gfx::math::{fx, sin};
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::host::{self, MiiPose, Sfx};
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------- types

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surf { Floor, Tile(usize), Air, Held }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Emote { Heart, Note, Bang, Question, Zzz, Anger, Sweat, Sparkle, Dots }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prop { None, Balloon(u8), Ball, Flower(u8), Can, Juggle, Bubbles }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DecorKind { Flower, Balloon, Star, Gift, Mushroom, Plant, Heart }
const DECOR_KINDS: [DecorKind; 7] = [DecorKind::Flower, DecorKind::Balloon, DecorKind::Star, DecorKind::Gift, DecorKind::Mushroom, DecorKind::Plant, DecorKind::Heart];

#[derive(Clone, Copy, Debug)]
struct Decor { kind: DecorKind, x: i32, y: i32, color: u8, born: u64, size: i32 }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PartKind { Confetti, Heart, Note, Zzz, Sparkle, Bubble, Dust, Tear }

#[derive(Clone, Copy, Debug)]
struct Particle { kind: PartKind, x: i32, y: i32, vx: i32, vy: i32, life: i32, color: u8 } // positions in 1/16 px

#[derive(Clone, Debug)]
enum Step {
    /// Walk along a path (points in pixels) using a travelling move.
    Walk { path: VecDeque<(i32, i32)>, mv: Mv, speed: i32 },
    /// Play a move for some frames.
    Do { mv: Mv, frames: i32 },
    /// Turn to face a direction (1/256 turn) or you.
    Face(i32),
    Say(String),
    Emote(Emote, i32),
    /// Climb straight up `dy` pixels onto a tile (the front face).
    ClimbOnto { tile: usize, dy: i32 },
    /// Climb up a tile's side edge to its top corner.
    ClimbSide { tile: usize, left: bool },
    /// Jump in an arc to (x, y), landing on `on`.
    JumpTo { x: i32, y: i32, on: Surf, frames: i32 },
    Hold(Prop),
    Decorate(DecorKind),
    Sound(Sfx, i32),
    Burst(PartKind, i32),
    Wait(i32),
    /// Float upward with a balloon, then the balloon pops.
    BalloonRide,
}

struct Agent {
    mii: usize,
    x: i32, // feet, in 1/16 px
    y: i32,
    vx: i32, vy: i32,
    on: Surf,
    yaw: i32, yaw_to: i32,
    mv: Mv, t: i32, len: i32,
    steps: VecDeque<Step>,
    cur: Option<Step>,
    say: Option<(String, i32)>,
    chirps: i32,
    emote: Option<(Emote, i32)>,
    prop: Prop,
    look: i32,      // head turn towards the pointer (dial units)
    dizzy: i32,     // shaken while carried
    asleep: bool,
    busy_until: u64,
    drawn: Option<(Rect, MiiPose)>, // last thing we drew (to know what to redraw)
    shy: bool,      // personality: shy Miis keep a bit more distance
}

pub struct Plaza {
    agents: Vec<Agent>,
    names: Vec<String>,
    rng: u64,
    w: i32, h: i32,
    top: i32,      // highest feet position (just under the greeting)
    floor: i32,    // lowest feet position
    size: i32,     // Mii height in pixels
    tiles: Vec<Rect>,
    decor: Vec<Decor>,
    parts: Vec<Particle>,
    frame: u64,
    idle: u64,     // frames since you last did anything
    ptr: (i32, i32),
    held: Option<usize>,
    shake: i32,
    clicks: (usize, u64, u32),
    grid: Vec<bool>, gw: i32, gh: i32,
    decor_loaded: bool,
    welcome: Vec<String>,
}

const G: i32 = 16; // sub-pixel units per pixel
const CELL: i32 = 24; // path-finding grid cell (pixels)
const GRAVITY: i32 = 9; // 1/16 px per frame per frame
const PASTEL: [Color; 8] = PASTELS;
const BRIGHT: [Color; 8] = [hex(0xFF6F91), hex(0xFFC94D), hex(0x7ED957), hex(0x35B6F2), hex(0x9C8CF0), hex(0xFF9F45), hex(0x2BB7A3), hex(0xF06CC0)];

fn ang_diff(from: i32, to: i32) -> i32 { (to - from + 128).rem_euclid(256) - 128 }

// ---------------------------------------------------------------- set-up

impl Plaza {
    pub fn new(seed: u64) -> Plaza {
        Plaza { agents: Vec::new(), names: Vec::new(), rng: seed | 1, w: 0, h: 0, top: 0, floor: 0, size: 0, tiles: Vec::new(),
                decor: Vec::new(), parts: Vec::new(), frame: 0, idle: 0, ptr: (-100, -100), held: None, shake: 0, clicks: (usize::MAX, 0, 0),
                grid: Vec::new(), gw: 0, gh: 0, decor_loaded: false, welcome: Vec::new() }
    }

    fn rand(&mut self, n: i32) -> i32 {
        let mut x = self.rng; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.rng = x;
        (x % n.max(1) as u64) as i32
    }
    fn chance(&mut self, pct: i32) -> bool { self.rand(100) < pct }

    pub fn active(&self) -> bool { !self.agents.is_empty() }

    /// The Mii that is "you" (for greetings).
    pub fn me_name(&self) -> Option<String> { host::mii_me().and_then(|i| self.names.get(i).cloned()) }

    /// Lay out the world for this screen, these tiles and the current list of Miis.
    pub fn sync(&mut self, w: i32, h: i32, top: i32, tiles: &[Rect]) {
        let names = host::miis();
        let size = (h * 15 / 100).max(90);
        let layout_changed = w != self.w || h != self.h || tiles != &self.tiles[..];
        if names == self.names && !layout_changed { return; }
        if layout_changed {
            self.w = w; self.h = h; self.top = top; self.floor = h - 8; self.size = size;
            self.tiles = tiles.to_vec();
            self.build_grid();
        }
        if !self.decor_loaded { self.load_decor(); self.decor_loaded = true; }
        // Keep Miis we already have (by name), add new ones walking in, drop removed ones.
        let old_names = core::mem::take(&mut self.names);
        let mut agents = Vec::new();
        let mut old: Vec<Option<Agent>> = core::mem::take(&mut self.agents).into_iter().map(Some).collect();
        for (i, n) in names.iter().enumerate().take(12) {
            if let Some(k) = old_names.iter().position(|o| o == n) {
                if let Some(mut a) = old.iter().position(|a| a.as_ref().map_or(false, |a| a.mii == k)).and_then(|p| old[p].take()) {
                    a.mii = i;
                    agents.push(a);
                    continue;
                }
            }
            // A new Mii: walks in from the side (unless it's the very first start).
            let fresh = !old_names.is_empty();
            let x = if fresh { if self.rand(2) == 0 { -self.size / 2 } else { w + self.size / 2 } } else { { let x = w * 15 / 100 + (i as i32 * 2 + 1) * (w * 70 / 100) / (names.len() as i32 * 2).max(1); if (x - w / 2).abs() < w / 8 { x - w / 5 } else { x } } };
            let y = self.floor - self.rand(h / 10);
            let shy = n.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32)) % 3 == 0;
            agents.push(Agent { mii: i, x: x * G, y: y * G, vx: 0, vy: 0, on: Surf::Floor, yaw: 0, yaw_to: 0, mv: Mv::Stand, t: 0, len: 120,
                steps: VecDeque::new(), cur: None, say: None, chirps: 0, emote: None, prop: Prop::None, look: 0, dizzy: 0, asleep: false,
                busy_until: 0, drawn: None, shy });
            if fresh { self.welcome.push(n.clone()); }
        }
        self.agents = agents;
        self.names = names;
        let welcome = core::mem::take(&mut self.welcome);
        for n in welcome { self.welcome_party(&n); }
    }

    /// Path-finding grid: which cells a Mii's feet may stand in (not inside a tile).
    fn build_grid(&mut self) {
        self.gw = self.w / CELL + 1;
        self.gh = self.h / CELL + 1;
        self.grid = vec![true; (self.gw * self.gh) as usize];
        for gy in 0..self.gh {
            for gx in 0..self.gw {
                let (x, y) = (gx * CELL + CELL / 2, gy * CELL + CELL / 2);
                let blocked = y < self.top || y > self.floor || x < self.size / 3 || x > self.w - self.size / 3
                    || self.tiles.iter().any(|t| t.inflate(6).contains(x, y));
                self.grid[(gy * self.gw + gx) as usize] = !blocked;
            }
        }
    }

    fn free(&self, x: i32, y: i32) -> bool {
        let (gx, gy) = (x / CELL, y / CELL);
        gx >= 0 && gy >= 0 && gx < self.gw && gy < self.gh && self.grid[(gy * self.gw + gx) as usize]
    }

    /// A random free spot on the floor.
    fn floor_spot(&mut self) -> (i32, i32) {
        for _ in 0..60 {
            let (x, y) = (self.size / 2 + self.rand(self.w - self.size), self.top + self.rand(self.floor - self.top));
            if self.free(x, y) { return (x, y); }
        }
        (self.w / 2, self.floor - 4)
    }

    /// Breadth-first search on the grid from a to b; returns pixel waypoints.
    fn path(&self, a: (i32, i32), b: (i32, i32)) -> VecDeque<(i32, i32)> {
        let idx = |x: i32, y: i32| (y * self.gw + x) as usize;
        let (sx, sy) = ((a.0 / CELL).clamp(0, self.gw - 1), (a.1 / CELL).clamp(0, self.gh - 1));
        let (tx, ty) = ((b.0 / CELL).clamp(0, self.gw - 1), (b.1 / CELL).clamp(0, self.gh - 1));
        let mut prev = vec![u32::MAX; self.grid.len()];
        let mut q = VecDeque::new();
        prev[idx(sx, sy)] = idx(sx, sy) as u32;
        q.push_back((sx, sy));
        let mut found = false;
        while let Some((x, y)) = q.pop_front() {
            if (x, y) == (tx, ty) { found = true; break; }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= self.gw || ny >= self.gh { continue; }
                let k = idx(nx, ny);
                if prev[k] != u32::MAX || !self.grid[k] { continue; }
                prev[k] = idx(x, y) as u32;
                q.push_back((nx, ny));
            }
        }
        let mut out = VecDeque::new();
        if !found { out.push_back(b); return out; }
        let mut cells = Vec::new();
        let mut k = idx(tx, ty);
        while k != idx(sx, sy) { cells.push(k); k = prev[k] as usize; }
        cells.reverse();
        // Keep only the corners of the path (fewer, straighter walks).
        let mut last_dir = (0, 0);
        let mut pts: Vec<(i32, i32)> = Vec::new();
        let mut px = (sx, sy);
        for &c in &cells {
            let (cx, cy) = (c as i32 % self.gw, c as i32 / self.gw);
            let dir = (cx - px.0, cy - px.1);
            if dir != last_dir && !pts.is_empty() { /* corner */ } else if !pts.is_empty() { pts.pop(); }
            pts.push((cx * CELL + CELL / 2, cy * CELL + CELL / 2));
            last_dir = dir; px = (cx, cy);
        }
        if let Some(l) = pts.last_mut() { *l = b; } else { pts.push(b); }
        out.extend(pts);
        out
    }

    // ---------------------------------------------------------------- geometry

    fn sprite_rect(&self, a: &Agent, frame_lift: i32) -> Rect {
        let s = self.size;
        let (x, y) = (a.x / G, a.y / G - frame_lift * s / 256);
        Rect::new(x - s / 2, y - s, s, s)
    }
    fn tile_stand_y(&self, i: usize) -> i32 { self.tiles[i].bottom() - 14 }

    /// The surface under a point: a tile (standing on it) or the floor.
    fn surface_at(&self, x: i32, y: i32) -> (Surf, i32) {
        for (i, t) in self.tiles.iter().enumerate() {
            if x > t.x + 10 && x < t.right() - 10 && y >= t.y + 20 && y <= t.bottom() + 8 { return (Surf::Tile(i), y.min(self.tile_stand_y(i))); }
        }
        (Surf::Floor, y.clamp(self.top, self.floor))
    }

    // ---------------------------------------------------------------- behaviours

    fn idle_agents(&self) -> Vec<usize> {
        (0..self.agents.len()).filter(|&k| { let a = &self.agents[k]; a.cur.is_none() && a.steps.is_empty() && a.on != Surf::Held && a.on != Surf::Air && a.busy_until <= self.frame }).collect()
    }

    fn push(&mut self, k: usize, s: Step) { self.agents[k].steps.push_back(s); }

    fn walk_to(&mut self, k: usize, to: (i32, i32), mv: Mv) {
        let a = &self.agents[k];
        let from = (a.x / G, a.y / G);
        let speed = match mv { Mv::Run => 40, Mv::Skip | Mv::Hop => 30, Mv::Tiptoe => 12, Mv::Moonwalk => 18, Mv::March => 22, _ => 22 } * self.size / 120;
        let path = if let Surf::Tile(_) = a.on { let mut p = VecDeque::new(); p.push_back(to); p } else { self.path(from, to) };
        self.push(k, Step::Walk { path, mv, speed });
    }

    fn say(&mut self, k: usize, text: &str) { self.push(k, Step::Say(String::from(text))); }

    /// Pick something for idle Mii `k` to do. Returns the behaviour's name.
    fn choose(&mut self, k: usize) -> &'static str {
        let on_tile = matches!(self.agents[k].on, Surf::Tile(_));
        let hour = (host_local_hour() + 24) % 24;
        let night = (0..6).contains(&hour);
        let sleepy = night || self.idle > 60 * 600; // late at night, or nobody around for 10 minutes
        if self.agents[k].asleep {
            if !sleepy || self.chance(25) { self.agents[k].asleep = false; self.push(k, Step::Do { mv: Mv::Stretch, frames: 100 }); self.say(k, "*yawn* Good morning!"); return "wake up"; }
            return "keep sleeping";
        }
        if sleepy && self.chance(12) {
            self.agents[k].asleep = true;
            if on_tile { self.push(k, Step::Do { mv: Mv::Nap, frames: 60 * 30 }); } else { self.push(k, Step::Do { mv: Mv::Doze, frames: 60 * 10 }); }
            self.push(k, Step::Burst(PartKind::Zzz, 4));
            return "fall asleep";
        }
        let others: Vec<usize> = self.idle_agents().into_iter().filter(|&o| o != k).collect();
        let roll = self.rand(100);
        if on_tile {
            return match roll {
                0..=14 => self.b_jump_off(k),
                15..=27 => self.b_sit_edge(k),
                28..=37 => self.b_walk_on_tile(k),
                38..=47 => self.b_decorate(k),
                48..=55 => self.b_stage_dance(k),
                56..=62 => self.b_leap(k),
                63..=68 => self.b_nap_on_tile(k),
                69..=74 => self.b_wave_from_tile(k),
                75..=79 => self.b_water_flower(k),
                80..=84 => self.b_acrobatics(k),
                _ => self.b_idle_move(k),
            };
        }
        if !others.is_empty() && roll < 22 {
            let o = others[self.rand(others.len() as i32) as usize];
            return self.b_social(k, o);
        }
        match roll {
            22..=38 => self.b_wander(k),
            39..=50 => self.b_climb_tile(k),
            51..=54 => self.b_climb_side(k),
            55..=64 => self.b_idle_move(k),
            65..=71 => self.b_emote(k),
            72..=77 => self.b_dance(k),
            78..=81 => self.b_acrobatics(k),
            82..=84 => self.b_balloon_ride(k),
            85..=87 => self.b_juggle(k),
            88..=90 => self.b_bubbles(k),
            91..=93 => self.b_exercise(k),
            94..=96 => self.b_kick_ball(k),
            _ => self.b_wave_at_you(k),
        }
    }

    // ---- moving around
    fn b_wander(&mut self, k: usize) -> &'static str {
        let spot = self.floor_spot();
        let mvs = [Mv::Walk, Mv::Walk, Mv::Walk, Mv::Skip, Mv::Run, Mv::Tiptoe, Mv::Hop, Mv::March, Mv::Moonwalk];
        let mv = mvs[self.rand(mvs.len() as i32) as usize];
        if mv == Mv::Tiptoe { self.push(k, Step::Emote(Emote::Dots, 80)); }
        self.walk_to(k, spot, mv);
        self.push(k, Step::Face(0));
        if mv == Mv::Moonwalk { self.say(k, "Smooth!"); }
        "wander"
    }
    fn b_climb_tile(&mut self, k: usize) -> &'static str {
        if self.tiles.is_empty() { return self.b_wander(k); }
        let i = self.rand(self.tiles.len() as i32) as usize;
        let t = self.tiles[i];
        let x = t.x + 30 + self.rand((t.w - 60).max(1));
        let below = (x, (t.bottom() + 16).min(self.floor));
        self.walk_to(k, below, Mv::Walk);
        self.push(k, Step::Face(128)); // turn to the tile
        self.push(k, Step::ClimbOnto { tile: i, dy: 30 });
        self.push(k, Step::Face(0));
        if self.chance(40) { self.push(k, Step::Do { mv: Mv::Cheer, frames: 70 }); }
        "climb onto an app"
    }
    fn b_climb_side(&mut self, k: usize) -> &'static str {
        if self.tiles.is_empty() { return self.b_wander(k); }
        let i = self.rand(self.tiles.len() as i32) as usize;
        let t = self.tiles[i];
        let left = self.chance(50);
        let x = if left { t.x - self.size / 5 } else { t.right() + self.size / 5 };
        self.walk_to(k, (x, (t.bottom() + 6).min(self.floor)), Mv::Walk);
        self.push(k, Step::ClimbSide { tile: i, left });
        self.push(k, Step::Do { mv: Mv::Hang, frames: 90 });
        self.say(k, "Whoa, high up!");
        self.push(k, Step::JumpTo { x, y: (t.bottom() + 12).min(self.floor), on: Surf::Floor, frames: 40 });
        "climb up the side of an app"
    }
    fn b_jump_off(&mut self, k: usize) -> &'static str {
        let Surf::Tile(i) = self.agents[k].on else { return "" };
        let t = self.tiles[i];
        let x = (self.agents[k].x / G + self.rand(81) - 40).clamp(self.size / 2, self.w - self.size / 2);
        let y = (t.bottom() + 20 + self.rand(40)).min(self.floor);
        let (x, y) = if self.free(x, y) { (x, y) } else { self.floor_spot() };
        self.push(k, Step::Do { mv: Mv::Crouch, frames: 14 });
        if self.chance(25) { self.say(k, "Cannonball!"); }
        self.push(k, Step::JumpTo { x, y, on: Surf::Floor, frames: 34 });
        "jump off an app"
    }
    fn b_leap(&mut self, k: usize) -> &'static str {
        let Surf::Tile(i) = self.agents[k].on else { return "" };
        let t = self.tiles[i];
        let near: Vec<usize> = (0..self.tiles.len()).filter(|&j| j != i && (self.tiles[j].x - t.x).abs() <= t.w + 60 && (self.tiles[j].y - t.y).abs() <= t.h + 60).collect();
        if near.is_empty() { return self.b_jump_off(k); }
        let j = near[self.rand(near.len() as i32) as usize];
        let u = self.tiles[j];
        let (x, y) = (u.x + u.w / 2, self.tile_stand_y(j));
        self.push(k, Step::Do { mv: Mv::Crouch, frames: 14 });
        self.push(k, Step::JumpTo { x, y, on: Surf::Tile(j), frames: 40 });
        self.push(k, Step::Do { mv: Mv::Proud, frames: 70 });
        "leap to the next app"
    }
    fn b_walk_on_tile(&mut self, k: usize) -> &'static str {
        let Surf::Tile(i) = self.agents[k].on else { return "" };
        let t = self.tiles[i];
        let x = t.x + 30 + self.rand((t.w - 60).max(1));
        self.walk_to(k, (x, self.tile_stand_y(i)), Mv::Walk);
        self.push(k, Step::Face(0));
        "stroll across an app"
    }
    fn b_sit_edge(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Face(0));
        let f = 60 * (6 + self.rand(10));
        self.push(k, Step::Do { mv: Mv::SitEdge, frames: f });
        "sit on the edge and swing legs"
    }
    fn b_nap_on_tile(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Do { mv: Mv::Yawn, frames: 90 });
        self.push(k, Step::Do { mv: Mv::Nap, frames: 60 * 12 });
        self.push(k, Step::Burst(PartKind::Zzz, 5));
        "nap on an app"
    }
    fn b_stage_dance(&mut self, k: usize) -> &'static str {
        let mvs = [Mv::DanceBounce, Mv::Disco, Mv::Twist, Mv::SwayDance];
        let m = mvs[self.rand(4) as usize];
        self.push(k, Step::Burst(PartKind::Note, 4));
        self.push(k, Step::Do { mv: m, frames: 300 });
        self.push(k, Step::Do { mv: Mv::Bow, frames: 75 });
        self.say(k, "Thank you, thank you!");
        "dance on stage"
    }
    fn b_wave_from_tile(&mut self, k: usize) -> &'static str { self.push(k, Step::Face(0)); self.push(k, Step::Do { mv: Mv::BigWave, frames: 130 }); self.say(k, "Hey! Up here!"); "wave from up high" }
    fn b_decorate(&mut self, k: usize) -> &'static str {
        let kind = DECOR_KINDS[self.rand(DECOR_KINDS.len() as i32) as usize];
        let phrase = match kind { DecorKind::Flower => "A flower for this app!", DecorKind::Balloon => "Balloons make it a party!", DecorKind::Star => "This app gets a gold star!",
            DecorKind::Gift => "A present!", DecorKind::Mushroom => "Look, a mushroom!", DecorKind::Plant => "Every desk needs a plant.", DecorKind::Heart => "I love this app!" };
        self.push(k, Step::Do { mv: Mv::Plant, frames: 90 });
        self.push(k, Step::Decorate(kind));
        self.say(k, phrase);
        "decorate an app"
    }
    fn b_water_flower(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Hold(Prop::Can));
        self.push(k, Step::Do { mv: Mv::Water, frames: 140 });
        self.push(k, Step::Hold(Prop::None));
        self.push(k, Step::Burst(PartKind::Sparkle, 6));
        "water the plants"
    }
    fn b_idle_move(&mut self, k: usize) -> &'static str {
        let mvs = [Mv::LookAround, Mv::Stretch, Mv::Yawn, Mv::ScratchHead, Mv::TapFoot, Mv::Shrug, Mv::Nod, Mv::HeadTilt, Mv::Sigh,
                   Mv::Shiver, Mv::Sneeze, Mv::CheckWatch, Mv::Sway, Mv::BounceIdle, Mv::Think];
        let m = mvs[self.rand(mvs.len() as i32) as usize];
        let f = self.rand(3) * 24 - 24;
        self.push(k, Step::Face(f));
        self.push(k, Step::Do { mv: m, frames: m.length() });
        match m {
            Mv::Sneeze => { self.say(k, "Achoo!"); }
            Mv::CheckWatch => { self.say(k, "Is it time yet?"); }
            Mv::Think => { self.push(k, Step::Emote(Emote::Question, 90)); }
            Mv::Shiver => { self.say(k, "Brrr!"); }
            Mv::Sigh => { self.push(k, Step::Emote(Emote::Dots, 80)); }
            _ => {}
        }
        "fidget"
    }
    fn b_emote(&mut self, k: usize) -> &'static str {
        let opts: [(Mv, Option<Emote>, &str); 12] = [
            (Mv::Cheer, Some(Emote::Sparkle), "Yay!"), (Mv::Clap, None, ""), (Mv::Laugh, None, "Hahaha!"), (Mv::Bow, None, ""),
            (Mv::Salute, None, "Reporting for duty!"), (Mv::ThumbsUp, None, "Nice!"), (Mv::Facepalm, Some(Emote::Sweat), "Oops..."),
            (Mv::HeartHands, Some(Emote::Heart), ""), (Mv::Stomp, Some(Emote::Anger), "Hmph!"), (Mv::Surprised, Some(Emote::Bang), "Whoa!"),
            (Mv::Proud, None, "Looking good!"), (Mv::Shy, None, "Hehe..."),
        ];
        let (m, e, s) = opts[self.rand(opts.len() as i32) as usize];
        self.push(k, Step::Face(0));
        if let Some(e) = e { self.push(k, Step::Emote(e, 100)); }
        if !s.is_empty() { self.say(k, s); }
        if m == Mv::HeartHands { self.push(k, Step::Burst(PartKind::Heart, 5)); }
        self.push(k, Step::Do { mv: m, frames: m.length() });
        "show a feeling"
    }
    fn b_dance(&mut self, k: usize) -> &'static str {
        let mvs = [Mv::DanceBounce, Mv::Disco, Mv::Robot, Mv::Twist, Mv::ChickenDance, Mv::Spin, Mv::Floss, Mv::Conga, Mv::SwayDance];
        let m = mvs[self.rand(mvs.len() as i32) as usize];
        self.push(k, Step::Face(0));
        self.push(k, Step::Burst(PartKind::Note, 3));
        self.push(k, Step::Do { mv: m, frames: 240 });
        "dance"
    }
    fn b_acrobatics(&mut self, k: usize) -> &'static str {
        let mvs = [Mv::Cartwheel, Mv::Backflip, Mv::Frontflip, Mv::Handstand, Mv::TwirlJump, Mv::Somersault];
        let m = mvs[self.rand(mvs.len() as i32) as usize];
        self.push(k, Step::Face(if m == Mv::Cartwheel { 0 } else { 64 }));
        self.push(k, Step::Do { mv: Mv::Crouch, frames: 14 });
        self.push(k, Step::Sound(Sfx::Jump, 256));
        self.push(k, Step::Do { mv: m, frames: m.length() });
        self.push(k, Step::Sound(Sfx::Land, 256));
        self.push(k, Step::Face(0));
        self.say(k, "Ta-da!");
        self.push(k, Step::Burst(PartKind::Sparkle, 6));
        "acrobatics"
    }
    fn b_balloon_ride(&mut self, k: usize) -> &'static str {
        let c = self.rand(8) as u8;
        self.push(k, Step::Hold(Prop::Balloon(c)));
        self.push(k, Step::Do { mv: Mv::HoldBalloon, frames: 60 });
        self.say(k, "Wheee!");
        self.push(k, Step::BalloonRide);
        "balloon ride"
    }
    fn b_juggle(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Face(0));
        self.push(k, Step::Hold(Prop::Juggle));
        self.push(k, Step::Do { mv: Mv::Juggle, frames: 300 });
        self.push(k, Step::Hold(Prop::None));
        self.push(k, Step::Do { mv: Mv::Bow, frames: 75 });
        "juggle"
    }
    fn b_bubbles(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Hold(Prop::Bubbles));
        for _ in 0..4 { self.push(k, Step::Burst(PartKind::Bubble, 4)); self.push(k, Step::Do { mv: Mv::Sing, frames: 50 }); }
        self.push(k, Step::Hold(Prop::None));
        "blow bubbles"
    }
    fn b_exercise(&mut self, k: usize) -> &'static str {
        let m = if self.chance(50) { Mv::Squats } else { Mv::JumpingJacks };
        self.push(k, Step::Face(0));
        self.say(k, "One, two, one, two!");
        self.push(k, Step::Do { mv: m, frames: 300 });
        self.push(k, Step::Emote(Emote::Sweat, 90));
        self.push(k, Step::Do { mv: Mv::Sigh, frames: 100 });
        "exercise"
    }
    fn b_kick_ball(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Hold(Prop::Ball));
        self.push(k, Step::Face(64));
        self.push(k, Step::Do { mv: Mv::Kick, frames: 75 });
        self.push(k, Step::Hold(Prop::None));
        self.push(k, Step::Sound(Sfx::Click, 180));
        self.push(k, Step::Burst(PartKind::Dust, 4));
        self.say(k, "Goal!");
        self.push(k, Step::Do { mv: Mv::Cheer, frames: 90 });
        "kick a ball"
    }
    fn b_wave_at_you(&mut self, k: usize) -> &'static str {
        self.push(k, Step::Face(0));
        self.push(k, Step::Do { mv: Mv::Wave, frames: 150 });
        let me = self.me_name().unwrap_or_else(|| String::from("there"));
        let line = format!("Hi {}!", me);
        self.push(k, Step::Say(line));
        "wave at you"
    }

    // ---- with another Mii
    fn b_social(&mut self, k: usize, o: usize) -> &'static str {
        let (ox, oy) = (self.agents[o].x / G, self.agents[o].y / G);
        let side = if self.agents[k].x < self.agents[o].x { -1 } else { 1 };
        let meet = ((ox + side * self.size * 6 / 10).clamp(self.size / 2, self.w - self.size / 2), oy);
        let meet = if self.free(meet.0, meet.1) { meet } else { (ox, oy) };
        let face_o = if side < 0 { 64 } else { 192 };
        let face_k = if side < 0 { 192 } else { 64 };
        let on = self.names.get(self.agents[o].mii).cloned().unwrap_or_default();
        let kn = self.names.get(self.agents[k].mii).cloned().unwrap_or_default();
        let shy = self.agents[o].shy;
        self.walk_to(k, meet, Mv::Walk);
        self.push(k, Step::Face(face_o));
        self.agents[o].busy_until = self.frame + 60 * 12;
        self.agents[o].steps.push_back(Step::Face(face_k));
        let kind = self.rand(12);
        let name: &'static str = match kind {
            0 => { self.say(k, &format!("Hi {}!", on)); self.push(k, Step::Do { mv: Mv::Wave, frames: 120 });
                   self.agents[o].steps.push_back(Step::Wait(60)); self.agents[o].steps.push_back(Step::Say(format!("Hey {}!", kn))); "say hello" }
            1 => { self.say(k, "High five!"); self.push(k, Step::Do { mv: Mv::HighFive, frames: 75 }); self.push(k, Step::Sound(Sfx::Click, 300));
                   self.agents[o].steps.push_back(Step::Wait(30)); self.agents[o].steps.push_back(Step::Do { mv: Mv::HighFive, frames: 75 }); "high five" }
            2 => { self.push(k, Step::Do { mv: Mv::Hug, frames: 120 }); self.push(k, Step::Burst(PartKind::Heart, 4));
                   self.agents[o].steps.push_back(Step::Do { mv: Mv::Hug, frames: 120 }); "hug" }
            3 => { self.push(k, Step::Burst(PartKind::Note, 4)); self.push(k, Step::Do { mv: Mv::DanceBounce, frames: 240 });
                   self.agents[o].steps.push_back(Step::Do { mv: Mv::DanceBounce, frames: 240 }); "dance together" }
            4 => { self.say(k, "Psst..."); self.push(k, Step::Do { mv: Mv::Whisper, frames: 100 });
                   self.agents[o].steps.push_back(Step::Wait(80)); self.agents[o].steps.push_back(Step::Do { mv: Mv::Laugh, frames: 120 }); self.agents[o].steps.push_back(Step::Say(String::from("No way! Hahaha!"))); "whisper a secret" }
            5 => { self.say(k, "Tag, you're it!"); self.push(k, Step::Do { mv: Mv::Point, frames: 40 });
                   let away = self.floor_spot(); self.walk_to(k, away, Mv::Run); self.agents[o].steps.push_back(Step::Wait(40));
                   self.agents[o].steps.push_back(Step::Say(String::from("Hey, come back!"))); let p = self.path((ox, oy), away); self.agents[o].steps.push_back(Step::Walk { path: p, mv: Mv::Run, speed: 36 * self.size / 120 }); "play tag" }
            6 => { self.say(k, "Catch!"); self.push(k, Step::Hold(Prop::Ball)); self.push(k, Step::Do { mv: Mv::Throw, frames: 75 }); self.push(k, Step::Hold(Prop::None));
                   self.agents[o].steps.push_back(Step::Wait(50)); self.agents[o].steps.push_back(Step::Hold(Prop::Ball)); self.agents[o].steps.push_back(Step::Do { mv: Mv::Catch, frames: 75 });
                   self.agents[o].steps.push_back(Step::Hold(Prop::None)); self.agents[o].steps.push_back(Step::Say(String::from("Got it!"))); "play catch" }
            7 => { self.push(k, Step::Burst(PartKind::Note, 6)); self.push(k, Step::Do { mv: Mv::Sing, frames: 200 });
                   self.agents[o].steps.push_back(Step::Do { mv: Mv::Sing, frames: 200 }); "sing a duet" }
            8 => { self.say(k, "Want to race? Go!"); let goal = self.floor_spot(); self.walk_to(k, goal, Mv::Run); self.push(k, Step::Do { mv: Mv::Cheer, frames: 90 });
                   let p = self.path((ox, oy), (goal.0 + self.size / 2, goal.1)); self.agents[o].steps.push_back(Step::Walk { path: p, mv: Mv::Run, speed: 34 * self.size / 120 });
                   self.agents[o].steps.push_back(Step::Do { mv: Mv::Sigh, frames: 100 }); "race" }
            9 => { self.say(k, "Follow me!"); let spot = self.floor_spot(); self.walk_to(k, spot, Mv::March);
                   let p = self.path((ox, oy), (spot.0 - self.size * 2 / 3, spot.1)); self.agents[o].steps.push_back(Step::Wait(40)); self.agents[o].steps.push_back(Step::Walk { path: p, mv: Mv::March, speed: 22 * self.size / 120 }); "follow the leader" }
            10 => { self.push(k, Step::Do { mv: Mv::Laugh, frames: 120 }); self.agents[o].steps.push_back(Step::Do { mv: Mv::Laugh, frames: 120 }); self.say(k, "Hahaha!"); "laugh together" }
            _ => { if shy { self.agents[o].steps.push_back(Step::Do { mv: Mv::Shy, frames: 120 }); self.agents[o].steps.push_back(Step::Say(String::from("Oh! H-hi..."))); }
                   self.say(k, "How's your day?"); self.push(k, Step::Do { mv: Mv::Nod, frames: 60 }); self.agents[o].steps.push_back(Step::Wait(70));
                   self.agents[o].steps.push_back(Step::Say(String::from("Pretty good!"))); self.agents[o].steps.push_back(Step::Do { mv: Mv::ThumbsUp, frames: 60 }); "chat" }
        };
        self.push(k, Step::Face(0));
        self.agents[o].steps.push_back(Step::Face(0));
        name
    }

    fn welcome_party(&mut self, name: &str) {
        let Some(newk) = self.agents.iter().position(|a| self.names.get(a.mii).map(|n| n == name).unwrap_or(false)) else { return };
        let spot = self.floor_spot();
        self.walk_to(newk, spot, Mv::Walk);
        self.push(newk, Step::Face(0));
        self.push(newk, Step::Do { mv: Mv::Wave, frames: 150 });
        self.say(newk, "Hi everyone! I'm new here!");
        for k in 0..self.agents.len() {
            if k == newk { continue; }
            self.agents[k].steps.clear();
            self.agents[k].asleep = false;
            self.push(k, Step::Wait(90 + k as i32 * 10));
            self.push(k, Step::Face(0));
            self.push(k, Step::Do { mv: Mv::Cheer, frames: 100 });
            self.push(k, Step::Say(format!("Welcome, {}!", name)));
        }
        self.push(newk, Step::Burst(PartKind::Confetti, 30));
        self.push(newk, Step::Sound(Sfx::Cheer, 256));
    }

    // ---------------------------------------------------------------- reactions to you

    /// You came back to the home screen.
    pub fn greet_all(&mut self, d: &mut Damage) {
        let _ = d;
        let me = self.me_name();
        for k in 0..self.agents.len() {
            if self.agents[k].asleep || self.agents[k].on == Surf::Air || self.agents[k].on == Surf::Held { continue; }
            if !self.agents[k].steps.is_empty() || self.agents[k].cur.is_some() { continue; }
            if self.rand(100) < 60 {
                let w = self.rand(30);
                self.push(k, Step::Wait(w));
                self.push(k, Step::Face(0));
                self.push(k, Step::Do { mv: Mv::Wave, frames: 150 });
                if k == 0 { let l = match &me { Some(n) => format!("Welcome back, {}!", n), None => String::from("Welcome back!") }; self.push(k, Step::Say(l)); }
            }
        }
    }

    /// You opened an app: nearby Miis cheer it on.
    pub fn app_opened(&mut self, tile: Rect) {
        for k in 0..self.agents.len() {
            let a = &self.agents[k];
            let (x, y) = (a.x / G, a.y / G);
            if (x - (tile.x + tile.w / 2)).abs() < tile.w && (y - (tile.y + tile.h / 2)).abs() < tile.h * 2 && a.on != Surf::Held && !a.asleep {
                self.agents[k].steps.clear();
                self.push(k, Step::Face(0));
                self.push(k, Step::Do { mv: Mv::Cheer, frames: 60 });
            }
        }
    }

    /// The selected tile changed: sometimes a nearby Mii points at it.
    pub fn tile_selected(&mut self, tile: Rect) {
        if !self.chance(30) { return; }
        let cx = tile.x + tile.w / 2;
        if let Some(k) = (0..self.agents.len()).filter(|&k| self.agents[k].cur.is_none() && self.agents[k].steps.is_empty() && !self.agents[k].asleep && self.agents[k].on == Surf::Floor)
            .min_by_key(|&k| (self.agents[k].x / G - cx).abs()) {
            let face = if self.agents[k].x / G < cx { 48 } else { 208 };
            self.push(k, Step::Face(face));
            self.push(k, Step::Do { mv: Mv::Point, frames: 60 });
            self.push(k, Step::Face(0));
        }
    }

    pub fn pointer(&mut self, x: i32, y: i32, d: &mut Damage) {
        let moved = (x - self.ptr.0).abs() + (y - self.ptr.1).abs();
        self.ptr = (x, y);
        self.idle = 0;
        if let Some(k) = self.held {
            self.shake += moved;
            d.add(self.area(k));
            self.agents[k].x = x * G;
            self.agents[k].y = (y + self.size * 7 / 10) * G;
        }
    }

    fn agent_at(&self, x: i32, y: i32) -> Option<usize> {
        let mut order: Vec<usize> = (0..self.agents.len()).collect();
        order.sort_by_key(|&k| -self.agents[k].y);
        order.into_iter().find(|&k| {
            let r = self.sprite_rect(&self.agents[k], 0);
            Rect::new(r.x + r.w / 4, r.y + r.h / 8, r.w / 2, r.h * 7 / 8).contains(x, y)
        })
    }

    /// A click: on a Mii? Then it reacts (and you can drag it). Returns true if handled.
    pub fn click(&mut self, x: i32, y: i32, d: &mut Damage) -> bool {
        self.idle = 0;
        let Some(k) = self.agent_at(x, y) else { return false };
        d.add(self.area(k));
        // Clicked several times quickly? That tickles!
        let quick = self.clicks.0 == k && self.frame - self.clicks.1 < 40;
        self.clicks = (k, self.frame, if quick { self.clicks.2 + 1 } else { 1 });
        let a = &mut self.agents[k];
        a.steps.clear(); a.cur = None;
        if a.asleep {
            a.asleep = false;
            a.steps.push_back(Step::Emote(Emote::Bang, 60));
            a.steps.push_back(Step::Do { mv: Mv::Surprised, frames: 60 });
            a.steps.push_back(Step::Say(String::from("Huh?! I'm awake!")));
            return true;
        }
        // Pick it up (it dangles from the pointer until you let go).
        self.held = Some(k);
        self.shake = 0;
        a.on = Surf::Held;
        a.mv = Mv::Carried; a.t = 0; a.len = 120;
        a.yaw_to = 0;
        host::sound(Sfx::Pop, 300);
        if self.clicks.2 >= 3 { a.say = Some((String::from("Hehehe, that tickles!"), 150)); a.chirps = 4; }
        true
    }

    /// The mouse button was released: drop a carried Mii, or greet you if it was just a click.
    pub fn release(&mut self, x: i32, y: i32) {
        let Some(k) = self.held.take() else { return };
        let me = self.me_name();
        let shaken = self.shake > self.size * 6;
        let was_click = self.shake < 8;
        let lines = ["Hi!", "Hello!", "Oh, hi there!", "What's up?", "Hey!"];
        let line = lines[self.rand(5) as usize];
        let (surf, land_y) = self.surface_at(x, y + self.size * 7 / 10);
        let a = &mut self.agents[k];
        if was_click {
            // Just a click: hop and say hi.
            a.on = Surf::Floor;
            let (s2, ly) = (surf, land_y);
            a.on = s2; a.y = ly * G;
            a.steps.push_back(Step::Face(0));
            a.steps.push_back(Step::Sound(Sfx::Jump, 300));
            a.steps.push_back(Step::Do { mv: Mv::Cheer, frames: 50 });
            a.steps.push_back(Step::Say(match &me { Some(n) if line == "Hi!" => format!("Hi, {}!", n), _ => String::from(line) }));
            a.steps.push_back(Step::Do { mv: Mv::Wave, frames: 150 });
            return;
        }
        // Dropped: fall to whatever is below, land with a squash.
        a.on = Surf::Air;
        a.vx = 0; a.vy = 0;
        a.steps.push_back(Step::JumpTo { x: x, y: land_y, on: surf, frames: 0 });
        if shaken {
            a.dizzy = 200;
            a.steps.push_back(Step::Emote(Emote::Sparkle, 120));
            a.steps.push_back(Step::Do { mv: Mv::Dizzy, frames: 200 });
            a.steps.push_back(Step::Say(String::from("Whoa... the room is spinning...")));
        } else {
            a.steps.push_back(Step::Say(String::from(["Thanks for the lift!", "Wheee!", "Nice view from up there!"][(self.frame % 3) as usize])));
        }
    }

    // ---------------------------------------------------------------- simulation

    /// Once a second: idle Miis decide what to do next; hourly cheers.
    pub fn second(&mut self, d: &mut Damage, local_time: i64) {
        let _ = d;
        set_local_time(local_time);
        if local_time.rem_euclid(3600) == 0 && !self.agents.is_empty() {
            for k in 0..self.agents.len() { if !self.agents[k].asleep { self.push(k, Step::Face(0)); self.push(k, Step::Do { mv: Mv::Cheer, frames: 90 }); } }
            self.push(0, Step::Say(String::from("Another hour! Hooray!")));
            self.push(0, Step::Burst(PartKind::Confetti, 30));
        }
        let idle = self.idle_agents();
        for k in idle {
            // Each idle Mii has a fair chance to start something every second: lively, not frantic.
            let p = if self.agents[k].asleep { 8 } else { 30 };
            if self.rand(100) < p { self.choose(k); }
        }
    }

    /// Everything a Mii covers on screen right now (picture, bubble, emote, prop).
    fn area(&self, k: usize) -> Rect {
        let a = &self.agents[k];
        let f = eval(a.mv, a.t, a.len);
        let r = self.sprite_rect(a, f.lift).inflate(4);
        let mut out = r.union(&Rect::new(r.x, r.y - self.size / 3, r.w, self.size / 3));
        // The shadow stays on the ground (below the Mii while it jumps).
        let (cx, gy) = (r.x + r.w / 2, a.y / G);
        out = out.union(&Rect::new(cx - self.size / 5 - 4, gy - self.size / 26 - 8, self.size * 2 / 5 + 8, self.size / 13 + 14));
        if let Some(b) = self.bubble_rect(k) { out = out.union(&b.inflate(26)); } // + shadow and tail
        if matches!(a.prop, Prop::Balloon(_)) { out = out.union(&Rect::new(r.x, r.y - self.size, r.w, self.size)); }
        out
    }

    fn bubble_rect(&self, k: usize) -> Option<Rect> {
        let a = &self.agents[k];
        let (text, _) = a.say.as_ref()?;
        let f = eval(a.mv, a.t, a.len);
        let r = self.sprite_rect(a, f.lift);
        let tw = font::MEDIUM.text_width(text) as i32 + 36;
        let x = (r.x + r.w / 2 - tw / 2).clamp(6, self.w - tw - 6);
        Some(Rect::new(x, (r.y + r.h / 12 - 56).max(4), tw, 48))
    }

    /// One 60 Hz step of the whole world. True while anything moves.
    pub fn tick(&mut self, d: &mut Damage) -> bool {
        self.frame += 1;
        self.idle += 1;
        let mut moving = false;
        for k in 0..self.agents.len() {
            let before = self.area(k);
            let changed = self.step_agent(k);
            if changed {
                moving = true;
                // Only redraw if what we'd draw actually changed.
                let a = &self.agents[k];
                let f = eval(a.mv, a.t, a.len);
                let mut pose = f.pose; pose.yaw = (a.yaw + f.spin).rem_euclid(256); pose.j[crate::host::j::HEAD_TURN] = pose.j[crate::host::j::HEAD_TURN].saturating_add(a.look as i8);
                let rect = self.sprite_rect(a, f.lift);
                let after = self.area(k);
                if a.drawn != Some((rect, pose)) || a.say.is_some() || a.emote.is_some() || after != before {
                    d.add(before);
                    d.add(after);
                }
                self.agents[k].drawn = Some((rect, pose));
            }
        }
        // Particles.
        if !self.parts.is_empty() {
            moving = true;
            let old = self.parts_area();
            for p in self.parts.iter_mut() {
                p.x += p.vx; p.y += p.vy; p.life -= 1;
                match p.kind { PartKind::Confetti | PartKind::Dust | PartKind::Tear => p.vy += 1, _ => {} }
            }
            self.parts.retain(|p| p.life > 0);
            d.add(old);
            d.add(self.parts_area());
        }
        // Decorations popping in.
        for dec in &self.decor { if self.frame - dec.born < 30 { moving = true; d.add(decor_rect(dec)); } }
        if host::miis_rendering() { moving = true; for k in 0..self.agents.len() { d.add(self.area(k)); } }
        moving
    }

    /// Advance one Mii by a frame. True if it changed in any way.
    fn step_agent(&mut self, k: usize) -> bool {
        let mut changed = false;
        // Look at the pointer when it's near (a little head turn).
        {
            let (px, py) = self.ptr;
            let a = &self.agents[k];
            let (ax, ay) = (a.x / G, a.y / G - self.size / 2);
            let near = (px - ax).abs() < self.size * 2 && (py - ay).abs() < self.size * 2 && !a.asleep && a.on != Surf::Held;
            let want = if near { ((ax - px) * 40 / (self.size * 2)).clamp(-35, 35) } else { 0 };
            if self.agents[k].look != want { self.agents[k].look += (want - self.agents[k].look).signum() * 2; changed = true; }
        }
        // Turn smoothly.
        {
            let a = &mut self.agents[k];
            let dy = ang_diff(a.yaw, a.yaw_to);
            if dy != 0 { a.yaw = (a.yaw + dy.clamp(-8, 8)).rem_euclid(256); changed = true; }
        }
        // Bubbles and emotes count down.
        {
            let a = &mut self.agents[k];
            if let Some((_, t)) = &mut a.say { *t -= 1; if *t <= 0 { a.say = None; } changed = true; }
            if let Some((_, t)) = &mut a.emote { *t -= 1; if *t <= 0 { a.emote = None; } changed = true; }
            if a.chirps > 0 && self.frame % 7 == 0 { a.chirps -= 1; host::sound(Sfx::Chirp, 200 + ((self.frame * 37 + k as u64 * 11) % 160) as i32); }
            if a.dizzy > 0 { a.dizzy -= 1; }
        }
        if self.agents[k].on == Surf::Held {
            let a = &mut self.agents[k];
            a.t += 1; changed = true;
            return changed;
        }
        // Start the next step if needed.
        if self.agents[k].cur.is_none() {
            if let Some(s) = self.agents[k].steps.pop_front() { self.start_step(k, s); changed = true; }
        }
        let Some(step) = self.agents[k].cur.take() else {
            // Nothing to do: stand still (no redraws needed).
            let a = &mut self.agents[k];
            if a.mv != Mv::Stand { a.mv = Mv::Stand; a.t = 0; a.len = 120; changed = true; }
            return changed;
        };
        let (keep, c2) = self.run_step(k, step);
        changed |= c2;
        if let Some(s) = keep { self.agents[k].cur = Some(s); }
        changed
    }

    fn start_step(&mut self, k: usize, s: Step) {
        let me = self.me_name();
        let a = &mut self.agents[k];
        match &s {
            Step::Do { mv, frames } => { a.mv = *mv; a.t = 0; a.len = (*frames).max(1); }
            Step::Walk { mv, .. } => { a.mv = *mv; a.t = 0; a.len = mv.length(); }
            Step::ClimbOnto { .. } | Step::ClimbSide { .. } => { a.mv = Mv::Climb; a.t = 0; a.len = 120; host::sound(Sfx::Step, 300); }
            _ => {}
        }
        let _ = me;
        a.cur = Some(s);
    }

    /// Run one frame of a step. Returns (step to keep running, changed?).
    fn run_step(&mut self, k: usize, step: Step) -> (Option<Step>, bool) {
        let size = self.size;
        match step {
            Step::Do { mv, frames } => {
                let a = &mut self.agents[k];
                a.t += 1;
                if a.t >= frames { return (None, true); }
                // Sound on the beat for some moves.
                if mv == Mv::Clap && a.t % 18 == 9 { host::sound(Sfx::Click, 280); }
                if (mv == Mv::Stomp || mv == Mv::March) && a.t % 16 == 0 { host::sound(Sfx::Step, 200); }
                if mv == Mv::Nap && a.t % 150 == 1 { host::sound(Sfx::Snore, 256); self.burst(k, PartKind::Zzz, 1); }
                (Some(Step::Do { mv, frames }), true)
            }
            Step::Walk { mut path, mv, speed } => {
                let Some(&(tx, ty)) = path.front() else { return (None, true) };
                let a = &mut self.agents[k];
                let (dx, dy) = (tx * G - a.x, ty * G - a.y);
                let dist = isqrt_i((dx as i64 * dx as i64 + dy as i64 * dy as i64) as u64);
                // `speed` is in 1/16 px per frame (so 22 = ~1.4 px/frame for a 120 px Mii).
                if dist <= speed { a.x = tx * G; a.y = ty * G; path.pop_front(); }
                else { a.x += dx * speed / dist.max(1); a.y += dy * speed / dist.max(1); }
                // Face where we're going: mostly sideways, turned a little towards you.
                let side = if dx.abs() * 2 >= dy.abs() { if dx > 0 { 52 } else { 204 } } else if dy > 0 { 0 } else { 128 };
                a.yaw_to = if mv == Mv::Moonwalk { (side + 128) % 256 } else { side };
                a.t += 1;
                if a.t % 18 == 0 { host::sound(Sfx::Step, 220 + (a.t % 36) * 2); }
                if path.is_empty() { return (None, true); }
                (Some(Step::Walk { path, mv, speed }), true)
            }
            Step::Face(yaw) => { self.agents[k].yaw_to = yaw.rem_euclid(256); (None, true) }
            Step::Say(text) => {
                let a = &mut self.agents[k];
                a.chirps = (text.len() as i32 / 4).clamp(2, 7);
                a.say = Some((text, 170));
                (None, true)
            }
            Step::Emote(e, t) => { self.agents[k].emote = Some((e, t)); host::sound(Sfx::Pop, 380); (None, true) }
            Step::Hold(p) => { self.agents[k].prop = p; (None, true) }
            Step::Sound(s, p) => { host::sound(s, p); (None, false) }
            Step::Burst(kind, n) => { self.burst(k, kind, n); (None, true) }
            Step::Wait(n) => { if n <= 0 { (None, false) } else { (Some(Step::Wait(n - 1)), false) } }
            Step::ClimbOnto { tile, dy } => {
                let target = (self.tiles[tile].bottom() - 14) * G;
                let a = &mut self.agents[k];
                a.mv = Mv::Climb; a.t += 1;
                let _ = dy;
                a.y -= G * 3 / 4;
                if a.y <= target {
                    a.y = target; a.on = Surf::Tile(tile); a.mv = Mv::Land; a.t = 0; a.len = 14;
                    host::sound(Sfx::Land, 330);
                    return (None, true);
                }
                (Some(Step::ClimbOnto { tile, dy }), true)
            }
            Step::ClimbSide { tile, left } => {
                let t = self.tiles[tile];
                let a = &mut self.agents[k];
                a.mv = Mv::Climb; a.t += 1;
                a.yaw_to = if left { 64 } else { 192 };
                a.y -= G * 3 / 4;
                if a.y / G <= t.y + size * 8 / 10 { return (None, true); }
                (Some(Step::ClimbSide { tile, left }), true)
            }
            Step::JumpTo { x, y, on, frames } => {
                let a = &mut self.agents[k];
                if a.on != Surf::Air {
                    // Take off: choose a speed that lands exactly on target after `frames`.
                    let f = frames.max(20);
                    a.on = Surf::Air;
                    a.vx = (x * G - a.x) / f;
                    a.vy = ((y * G - a.y) - GRAVITY * f * f / 2) / f;
                    a.mv = Mv::Jump; a.t = 0; a.len = 120;
                    a.yaw_to = if a.vx > 0 { 52 } else if a.vx < 0 { 204 } else { a.yaw_to };
                    host::sound(Sfx::Jump, 256);
                }
                a.vy += GRAVITY;
                a.x += a.vx; a.y += a.vy;
                a.t += 1;
                if a.vy > 0 { a.mv = Mv::Fall; }
                if a.vy > 0 && a.y >= y * G {
                    a.x = if frames == 0 { a.x } else { x * G }; a.y = y * G; a.on = on; a.vx = 0; a.vy = 0;
                    a.mv = Mv::Land; a.t = 0; a.len = 14;
                    host::sound(Sfx::Land, 256);
                    self.burst(k, PartKind::Dust, 4);
                    self.agents[k].steps.push_front(Step::Do { mv: Mv::Land, frames: 14 });
                    return (None, true);
                }
                (Some(Step::JumpTo { x, y, on, frames }), true)
            }
            Step::Decorate(kind) => {
                let a = &self.agents[k];
                let (x, y) = (a.x / G + if a.yaw < 128 { size / 3 } else { -size / 3 }, a.y / G);
                let color = self.rand(8) as u8;
                if self.decor.len() >= 16 { self.decor.remove(0); }
                self.decor.push(Decor { kind, x, y, color, born: self.frame, size: size * 4 / 10 });
                self.save_decor();
                host::sound(Sfx::Pop, 256);
                self.burst(k, PartKind::Sparkle, 5);
                (None, true)
            }
            Step::BalloonRide => {
                let a = &mut self.agents[k];
                if a.on != Surf::Air { a.on = Surf::Air; a.t = 0; a.mv = Mv::HoldBalloon; a.len = 120; }
                a.t += 1;
                a.y -= G / 2 + G / 4;
                a.x += sin(a.t * 2) * G / 256;
                let high = a.y / G < self.top + size;
                if high || a.t > 300 {
                    // Pop! Down we go.
                    a.prop = Prop::None;
                    host::sound(Sfx::Pop, 180);
                    let (x, y) = (a.x / G, a.y / G);
                    let (surf, ly) = self.surface_at(x, (y + size).min(self.floor));
                    self.burst(k, PartKind::Confetti, 10);
                    self.agents[k].on = Surf::Floor;
                    self.agents[k].steps.push_front(Step::Say(String::from("Oops! My balloon!")));
                    self.agents[k].steps.push_front(Step::JumpTo { x, y: ly.max(y + 4), on: surf, frames: 0 });
                    self.agents[k].on = Surf::Air;
                    self.agents[k].vx = 0; self.agents[k].vy = 0;
                    return (None, true);
                }
                (Some(Step::BalloonRide), true)
            }
        }
    }

    fn burst(&mut self, k: usize, kind: PartKind, n: i32) {
        if self.parts.len() > 120 { return; }
        let (ax, ay) = (self.agents[k].x, self.agents[k].y);
        let (x, y) = (ax / G * 16, (ay / G - self.size * 9 / 10) * 16);
        for _ in 0..n {
            let (vx, vy, life) = match kind {
                PartKind::Confetti => (self.rand(81) - 40, -40 - self.rand(40), 120),
                PartKind::Dust => (self.rand(41) - 20, -self.rand(12), 30),
                PartKind::Zzz => (8 + self.rand(8), -10, 110),
                PartKind::Heart | PartKind::Note | PartKind::Sparkle => (self.rand(31) - 15, -14 - self.rand(10), 90),
                PartKind::Bubble => (12 + self.rand(16), -8 - self.rand(10), 140),
                PartKind::Tear => (self.rand(11) - 5, 0, 40),
            };
            let y0 = if kind == PartKind::Dust { ay / G * 16 } else { y };
            let color = self.rand(8) as u8;
            let dx = self.rand(200) - 100;
            self.parts.push(Particle { kind, x: x + dx, y: y0, vx, vy, life, color });
        }
    }

    fn parts_area(&self) -> Rect {
        let mut r = Rect::default();
        for p in &self.parts { r = r.union(&Rect::new(p.x / 16 - 14, p.y / 16 - 14, 28, 28)); }
        r
    }

    // ---------------------------------------------------------------- saving decorations

    fn save_decor(&self) {
        let mut v = Vec::new();
        v.extend_from_slice(b"DEC1");
        for d in &self.decor {
            v.push(DECOR_KINDS.iter().position(|k| *k == d.kind).unwrap_or(0) as u8);
            v.push(d.color);
            v.extend_from_slice(&(d.x as i16).to_le_bytes());
            v.extend_from_slice(&(d.y as i16).to_le_bytes());
        }
        let _ = crate::fs::write("decor.dat", &v);
    }
    fn load_decor(&mut self) {
        let Ok(b) = crate::fs::read("decor.dat") else { return };
        if b.len() < 4 || &b[..4] != b"DEC1" { return; }
        for c in b[4..].chunks_exact(6) {
            let kind = DECOR_KINDS[c[0] as usize % DECOR_KINDS.len()];
            let x = i16::from_le_bytes([c[2], c[3]]) as i32;
            let y = i16::from_le_bytes([c[4], c[5]]) as i32;
            if x > 0 && y > 0 && x < self.w && y < self.h { self.decor.push(Decor { kind, x, y, color: c[1], born: 0, size: self.size * 4 / 10 }); }
        }
    }

    // ---------------------------------------------------------------- drawing

    pub fn draw(&self, c: &mut Canvas) {
        // Decorations first (they sit on the tiles), then Miis from back to front.
        for dec in &self.decor {
            if !decor_rect(dec).overlaps(&c.clip) { continue; }
            let grow = ((self.frame.saturating_sub(dec.born)) as i32 * 256 / 30).min(256);
            draw_decor(c, dec, self.frame, if dec.born == 0 { 256 } else { crate::gfx::math::ease_out_back(grow) });
        }
        let mut order: Vec<usize> = (0..self.agents.len()).collect();
        order.sort_by_key(|&k| self.agents[k].y);
        if let Some(h) = self.held { order.retain(|&k| k != h); order.push(h); }
        for &k in &order {
            let area = self.area(k);
            let (rect, pose, lift) = {
                let a = &self.agents[k];
                let f = eval(a.mv, a.t, a.len);
                let mut pose = f.pose;
                pose.yaw = (a.yaw + f.spin).rem_euclid(256);
                let hj = crate::host::j::HEAD_TURN;
                pose.j[hj] = (pose.j[hj] as i32 + a.look).clamp(-120, 120) as i8;
                (self.sprite_rect(a, f.lift), pose, f.lift)
            };
            if !area.overlaps(&c.clip) { continue; }
            let a = &self.agents[k];
            // Soft shadow on the ground (stays put while jumping).
            if a.on != Surf::Held {
                let shy = a.y / G;
                c.fill_ellipse_fx(fx(rect.x + rect.w / 2), fx(shy - 2), fx(self.size / 5 - lift.abs() * self.size / 2560), fx(self.size / 26 + 2), with_alpha(0x303858, 34));
            }
            // A balloon on a string behind the Mii.
            if let Prop::Balloon(col) = a.prop {
                let (hx, hy) = (rect.x + rect.w * 62 / 100, rect.y + rect.h * 18 / 100);
                let (bx, by) = (hx + sin(self.frame as i32 * 2) * 6 / 256, hy - self.size * 55 / 100);
                c.line(hx, hy, bx, by + self.size / 8, 2, INK_SOFT);
                c.fill_ellipse_fx(fx(bx), fx(by), fx(self.size / 7), fx(self.size / 6), BRIGHT[col as usize % 8]);
                c.fill_ellipse_fx(fx(bx - self.size / 24), fx(by - self.size / 20), fx(self.size / 28), fx(self.size / 22), with_alpha(WHITE, 140));
            }
            host::mii_draw(c, a.mii, pose, rect);
            match a.prop {
                Prop::Ball => { c.fill_circle(rect.x + rect.w * 66 / 100, rect.bottom() - self.size / 14, self.size / 14, WHITE); c.ring_fx(fx(rect.x + rect.w * 66 / 100), fx(rect.bottom() - self.size / 14), fx(self.size / 14), fx(2), INK); }
                Prop::Can => { let (x, y) = (rect.x + rect.w * 70 / 100, rect.y + rect.h * 55 / 100); c.fill_round_rect(Rect::new(x - 12, y - 10, 24, 20), 6, hex(0x35B6F2)); c.line(x + 10, y - 4, x + 26, y - 14, 4, hex(0x35B6F2)); }
                Prop::Juggle => {
                    for b in 0..3 {
                        let ph = self.frame as i32 * 4 + b * 85;
                        let (bx, by) = (rect.x + rect.w / 2 + sin(ph) * self.size / 5 / 256, rect.y + rect.h * 25 / 100 - (sin(ph * 2) * self.size / 6 / 256).abs());
                        c.fill_circle(bx, by, self.size / 22, BRIGHT[b as usize]);
                    }
                }
                _ => {}
            }
            // Emote icon above the head.
            if let Some((e, t)) = a.emote {
                let (ex, ey) = (rect.x + rect.w * 68 / 100, rect.y + rect.h / 10 - (t % 30 < 15) as i32 * 2);
                draw_emote(c, e, ex, ey, self.size / 6);
            }
        }
        // Particles.
        for p in &self.parts {
            let (x, y) = (p.x / 16, p.y / 16);
            if !Rect::new(x - 14, y - 14, 28, 28).overlaps(&c.clip) { continue; }
            let fade = (p.life * 256 / 40).min(256) as u8;
            match p.kind {
                PartKind::Confetti => c.fill_rect(Rect::new(x, y, 7, 4 + (p.life % 4)), BRIGHT[p.color as usize % 8]),
                PartKind::Dust => c.fill_circle(x, y, 4 + (30 - p.life) / 6, with_alpha(0xD8DDE8, fade)),
                PartKind::Zzz => { c.text(&font::MEDIUM, x, y, "z", with_alpha(INK_SOFT, fade)); }
                PartKind::Heart => draw_heart(c, x, y, 9, hex(0xFF6F91)),
                PartKind::Note => { c.fill_ellipse_fx(fx(x), fx(y), fx(5), fx(4), INK); c.line(x + 4, y, x + 4, y - 14, 2, INK); }
                PartKind::Sparkle => { c.line(x - 6, y, x + 6, y, 2, hex(0xFFC94D)); c.line(x, y - 6, x, y + 6, 2, hex(0xFFC94D)); }
                PartKind::Bubble => c.ring_fx(fx(x), fx(y), fx(7), fx(2), with_alpha(0x7FC4F5, fade.max(80))),
                PartKind::Tear => c.fill_ellipse_fx(fx(x), fx(y), fx(3), fx(5), hex(0x7FC4F5)),
            }
        }
        // Speech bubbles on top of everything.
        for &k in &order {
            let Some(b) = self.bubble_rect(k) else { continue };
            if !b.inflate(14).overlaps(&c.clip) { continue; }
            let a = &self.agents[k];
            let r = self.sprite_rect(a, 0);
            c.shadow(b, 22, 5, 45);
            c.fill_round_rect(b, 22, WHITE);
            let tip = (r.x + r.w / 2).clamp(b.x + 24, b.right() - 24);
            c.fill_triangle_fx([(fx(tip - 10), fx(b.bottom() - 2)), (fx(tip + 8), fx(b.bottom() - 2)), (fx(tip - 2), fx(b.bottom() + 12))], WHITE);
            if let Some((text, _)) = &a.say { c.text(&font::MEDIUM, b.x + 18, b.y + 12, text, INK); }
        }
    }
}

// ---------------------------------------------------------------- little drawings

fn decor_rect(d: &Decor) -> Rect { Rect::new(d.x - d.size, d.y - d.size * 3, d.size * 2, d.size * 3 + 6) }

fn draw_heart(c: &mut Canvas, x: i32, y: i32, s: i32, col: Color) {
    c.fill_circle(x - s / 2, y - s / 3, s * 6 / 10, col);
    c.fill_circle(x + s / 2, y - s / 3, s * 6 / 10, col);
    c.fill_triangle_fx([(fx(x - s * 11 / 10), fx(y - s / 6)), (fx(x + s * 11 / 10), fx(y - s / 6)), (fx(x), fx(y + s))], col);
}

fn draw_decor(c: &mut Canvas, d: &Decor, frame: u64, grow: i32) {
    let s = d.size * grow / 256;
    if s < 2 { return; }
    let (x, y) = (d.x, d.y);
    let col = BRIGHT[d.color as usize % 8];
    let sway = sin(frame as i32 + d.x) * s / 2048;
    match d.kind {
        DecorKind::Flower => {
            c.line(x, y, x + sway, y - s * 2, 3, hex(0x5CB85C));
            c.fill_ellipse_fx(fx(x + 6), fx(y - s), fx(s / 3), fx(s / 6), hex(0x7ED957));
            let (fx0, fy0) = (x + sway, y - s * 2);
            for i in 0..5 {
                let a = i * 51;
                c.fill_circle(fx0 + sin(a) * s / 2 / 256, fy0 - sin(a + 64) * s / 2 / 256, s / 3, col);
            }
            c.fill_circle(fx0, fy0, s / 4, hex(0xFFE066));
        }
        DecorKind::Balloon => {
            let (bx, by) = (x + sway * 2, y - s * 3 + s / 2);
            c.line(x, y, bx, by + s / 2, 2, INK_SOFT);
            c.fill_ellipse_fx(fx(bx), fx(by), fx(s * 5 / 10), fx(s * 6 / 10), col);
            c.fill_ellipse_fx(fx(bx - s / 6), fx(by - s / 5), fx(s / 9), fx(s / 7), with_alpha(WHITE, 150));
        }
        DecorKind::Star => {
            let cy = y - s;
            let pts: Vec<(i32, i32)> = (0..10).map(|i| { let r = if i % 2 == 0 { s } else { s * 45 / 100 }; let a = i * 256 / 10 + (frame as i32 / 4) % 256 * 0; (x + sin(a) * r / 256, cy - sin(a + 64) * r / 256) }).collect();
            for i in 0..10 { let (a, b) = (pts[i], pts[(i + 1) % 10]); c.fill_triangle_fx([(fx(x), fx(cy)), (fx(a.0), fx(a.1)), (fx(b.0), fx(b.1))], hex(0xFFC94D)); }
        }
        DecorKind::Gift => {
            let r = Rect::new(x - s * 6 / 10, y - s, s * 12 / 10, s);
            c.fill_round_rect(r, s / 8, col);
            c.fill_rect(Rect::new(x - s / 10, r.y, s / 5, r.h), WHITE);
            c.fill_ellipse_fx(fx(x - s / 4), fx(r.y - s / 8), fx(s / 4), fx(s / 7), WHITE);
            c.fill_ellipse_fx(fx(x + s / 4), fx(r.y - s / 8), fx(s / 4), fx(s / 7), WHITE);
        }
        DecorKind::Mushroom => {
            c.fill_round_rect(Rect::new(x - s / 5, y - s * 6 / 10, s * 2 / 5, s * 6 / 10), s / 8, hex(0xFFF1D6));
            let saved = c.clip;
            c.clip = saved.intersect(&Rect::new(x - s, y - s * 2, s * 2, s * 13 / 10));
            c.fill_ellipse_fx(fx(x), fx(y - s * 6 / 10), fx(s * 7 / 10), fx(s * 6 / 10), hex(0xFF6F6F));
            c.clip = saved;
            for (dx, dy) in [(-s / 3, -s), (s / 4, -s * 9 / 10), (0, -s * 12 / 10)] { c.fill_circle(x + dx, y + dy, s / 9 + 1, WHITE); }
        }
        DecorKind::Plant => {
            for (i, a) in [-40i32, 0, 40].iter().enumerate() {
                let (lx, ly) = (x + a * s / 100 + sway, y - s * 9 / 10 - (i as i32 % 2) * s / 4);
                c.fill_ellipse_fx(fx(lx), fx(ly), fx(s / 5), fx(s / 2), hex(0x5CB85C));
            }
            c.fill_round_rect(Rect::new(x - s / 2, y - s * 5 / 10, s, s / 2), s / 8, hex(0xD9825B));
        }
        DecorKind::Heart => draw_heart(c, x, y - s, s * 7 / 10, hex(0xFF6F91)),
    }
}

fn draw_emote(c: &mut Canvas, e: Emote, x: i32, y: i32, s: i32) {
    match e {
        Emote::Heart => draw_heart(c, x, y, s / 2, hex(0xFF6F91)),
        Emote::Note => { c.fill_ellipse_fx(fx(x), fx(y), fx(s / 5), fx(s / 6), INK); c.line(x + s / 5, y, x + s / 5, y - s / 2, 3, INK); }
        Emote::Bang => { c.fill_round_rect(Rect::new(x - 4, y - s / 2, 8, s * 6 / 10 - 8), 4, hex(0xE0455E)); c.fill_circle(x, y + s / 4, 5, hex(0xE0455E)); }
        Emote::Question => { c.text(&font::LARGE, x - 8, y - s / 2, "?", ACCENT_DARK); }
        Emote::Zzz => { c.text(&font::LARGE, x, y - s / 2, "z", INK_SOFT); }
        Emote::Anger => { let r = s / 4; for (a, b) in [(-1, -1), (1, 1)] { c.line(x - r * a, y - r * b, x + r * a, y + r * b, 4, hex(0xE0455E)); } c.line(x - r, y + r, x + r, y - r, 4, hex(0xE0455E)); }
        Emote::Sweat => c.fill_ellipse_fx(fx(x), fx(y), fx(s / 8), fx(s / 5), hex(0x7FC4F5)),
        Emote::Sparkle => { for (dx, dy) in [(0, 0), (s / 3, -s / 4), (-s / 4, s / 5)] { c.line(x + dx - 6, y + dy, x + dx + 6, y + dy, 3, hex(0xFFC94D)); c.line(x + dx, y + dy - 6, x + dx, y + dy + 6, 3, hex(0xFFC94D)); } }
        Emote::Dots => { for i in 0..3 { c.fill_circle(x - s / 4 + i * s / 4, y, 4, INK_SOFT); } }
    }
}

fn isqrt_i(n: u64) -> i32 { crate::gfx::math::isqrt(n) as i32 }

// The plaza needs the local hour (sleepy at night). Kept here as a tiny global.
static LOCAL_TIME: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(12 * 3600);
fn set_local_time(t: i64) { LOCAL_TIME.store(t, core::sync::atomic::Ordering::Relaxed); }
fn host_local_hour() -> i32 { (LOCAL_TIME.load(core::sync::atomic::Ordering::Relaxed).rem_euclid(86400) / 3600) as i32 }

/// How many different things Miis can do (for the README).
pub const BEHAVIOURS: [&str; 38] = [
    "wander (walk, skip, run, tiptoe, bunny hop, march, moonwalk)", "climb onto an app", "climb up the side of an app", "hang from an app",
    "jump off an app", "leap to the next app", "stroll across an app", "sit on an app's edge swinging legs", "nap on an app", "dance on an app like a stage",
    "wave from up high", "decorate an app (flower, balloon, star, gift, mushroom, plant, heart)", "water the plants", "fidget (15 idle moves)",
    "show a feeling (12 emotes)", "dance (9 dances)", "acrobatics (cartwheel, flips, handstand, twirl, somersault)", "balloon ride (and the balloon pops)",
    "juggle", "blow bubbles", "exercise (squats, jumping jacks)", "kick a ball", "wave at you",
    "say hello to another Mii", "high five", "hug", "dance together", "whisper a secret", "play tag", "play catch", "sing a duet", "race",
    "follow the leader", "laugh together", "chat", "welcome party for a new Mii", "hourly celebration", "fall asleep at night / when you're away",
];
