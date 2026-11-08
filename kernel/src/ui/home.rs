//! The home screen: a greeting, a grid of rounded app tiles, and a footer
//! with the clock.
//!
//! Navigation: arrow keys move the selection, Enter opens. The mouse (or
//! TrackPoint) selects by hovering and opens with a click.
//!
//! Animation: the selection ring glides between tiles on springs, the
//! selected tile grows a little, and at start-up the tiles pop in one by one.
//! Every animated thing reports the rectangle it touches, so only those
//! parts of the screen are redrawn.

use super::anim::{Spring, Tween};
use super::{icons, Action, App, AppId, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use core::fmt::Write;

pub const COLS: usize = 4;
pub const ROWS: usize = 3;
const N: usize = COLS * ROWS;

#[derive(Clone, Copy)]
pub struct TileInfo { pub app: Option<AppId>, pub name: &'static str, pub color: Color }

pub const TILES: [TileInfo; N] = [
    TileInfo { app: Some(AppId::Pals), name: "Pals", color: PASTELS[0] },
    TileInfo { app: Some(AppId::Write), name: "Write", color: PASTELS[2] },
    TileInfo { app: Some(AppId::Files), name: "Files", color: PASTELS[4] },
    TileInfo { app: Some(AppId::Clock), name: "Clock", color: PASTELS[1] },
    TileInfo { app: Some(AppId::Settings), name: "Settings", color: PASTELS[5] },
    TileInfo { app: Some(AppId::About), name: "About", color: PASTELS[3] },
    TileInfo { app: None, name: "", color: 0 }, TileInfo { app: None, name: "", color: 0 },
    TileInfo { app: None, name: "", color: 0 }, TileInfo { app: None, name: "", color: 0 },
    TileInfo { app: None, name: "", color: 0 }, TileInfo { app: None, name: "", color: 0 },
];

pub struct Layout { pub tile_w: i32, pub tile_h: i32, pub gap: i32, pub x0: i32, pub y0: i32, pub footer_y: i32, pub radius: i32 }

pub fn layout(w: i32, h: i32) -> Layout {
    let footer_h = h / 6;
    let header_h = h / 8;
    let gap = w / 48;
    let avail_h = h - footer_h - header_h - gap * 2;
    let tile_h = (avail_h - gap * (ROWS as i32 - 1)) / ROWS as i32;
    let tile_w = (tile_h * 16 / 10).min((w - gap * 6) / COLS as i32);
    let grid_w = tile_w * COLS as i32 + gap * (COLS as i32 - 1);
    Layout { tile_w, tile_h, gap, x0: (w - grid_w) / 2, y0: header_h + gap / 2, footer_y: h - footer_h, radius: tile_h / 6 }
}

impl Layout {
    pub fn tile(&self, i: usize) -> Rect {
        let (col, row) = ((i % COLS) as i32, (i / COLS) as i32);
        Rect::new(self.x0 + col * (self.tile_w + self.gap), self.y0 + row * (self.tile_h + self.gap), self.tile_w, self.tile_h)
    }
    /// Everything a tile might touch when animated (growth, shadow, ring).
    fn tile_damage(&self, i: usize) -> Rect { self.tile(i).inflate(self.gap * 3 / 4 + 4) }
}

pub struct Home {
    pub sel: usize,
    ring: [Spring; 4],     // x, y, w, h of the selection ring (fixed point)
    grow: [Spring; N],     // per-tile scale, 256 = normal size
    intro: Tween,          // tiles popping in at start-up
    ring_rect: Rect,       // where the ring was last drawn (for damage)
    lay: Layout,
    last_min: i64,
    pub avatar_hook: Option<fn(&mut Canvas, i32, i32, i32, u64)>, // draws the user's avatar (milestone 7)
}

impl Home {
    pub fn new(w: i32, h: i32) -> Home {
        let lay = layout(w, h);
        let t = lay.tile(0).inflate(10);
        let mut intro = Tween::default();
        intro.start(40);
        Home {
            sel: 0,
            ring: [Spring::new(fx(t.x)), Spring::new(fx(t.y)), Spring::new(fx(t.w)), Spring::new(fx(t.h))],
            grow: [Spring::new(ONE); N],
            intro,
            ring_rect: t,
            lay,
            last_min: -1,
            avatar_hook: None,
        }
    }

    fn select(&mut self, i: usize, d: &mut Damage) {
        if i == self.sel { return; }
        d.add(self.lay.tile_damage(self.sel));
        d.add(self.lay.tile_damage(i));
        self.grow[self.sel].set(ONE);
        self.sel = i;
        self.grow[i].set(ONE + 16); // +6%
        let t = self.lay.tile(i).inflate(10);
        self.ring[0].set(fx(t.x)); self.ring[1].set(fx(t.y));
        self.ring[2].set(fx(t.w)); self.ring[3].set(fx(t.h));
    }

    fn open(&self, i: usize) -> Action {
        match TILES[i].app { Some(a) => Action::Open(a, self.lay.tile(i)), None => Action::None }
    }

    fn tile_at(&self, x: i32, y: i32) -> Option<usize> { (0..N).find(|&i| self.lay.tile(i).contains(x, y)) }

    /// Scale of tile i right now (intro pop-in × selection growth).
    fn scale(&self, i: usize) -> Fx {
        let g = self.grow[i].value;
        if !self.intro.running { return g; }
        // Each tile starts a little after the previous one.
        let delay = i as i32 * 2;
        let t = ((self.intro.t - delay).max(0) * 256 / 18).min(256);
        fx_mul(g, ease_out_back(t))
    }

    /// The tile that opens `app` (for the closing animation).
    pub fn lay_tile_of(&self, app: AppId) -> Rect {
        let i = TILES.iter().position(|t| t.app == Some(app)).unwrap_or(0);
        self.lay.tile(i)
    }

    fn footer_rect(&self, s: &Shared) -> Rect { Rect::new(0, self.lay.footer_y, s.w, s.h - self.lay.footer_y) }
}

/// The shared soft background: a gentle vertical gradient with a dot grid.
pub fn background(c: &mut Canvas, top: Color, bottom: Color) {
    let clip = c.clip;
    let full = Rect::new(0, 0, c.width, c.height);
    c.fill_gradient(full, top, bottom);
    let step = 48;
    let mut y = clip.y - clip.y.rem_euclid(step) + step / 2 - step;
    while y < clip.bottom() + step {
        let mut x = clip.x - clip.x.rem_euclid(step) + step / 2 - step;
        while x < clip.right() + step {
            c.fill_circle(x, y, 3, with_alpha(0xC8D0E4, 90));
            x += step;
        }
        y += step;
    }
}

impl App for Home {
    fn enter(&mut self, s: &mut Shared) { self.lay = layout(s.w, s.h); }

    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key, .. } => {
                let (col, row) = (self.sel % COLS, self.sel / COLS);
                match key {
                    Key::Left if col > 0 => self.select(self.sel - 1, d),
                    Key::Right if col + 1 < COLS => self.select(self.sel + 1, d),
                    Key::Up if row > 0 => self.select(self.sel - COLS, d),
                    Key::Down if row + 1 < ROWS => self.select(self.sel + COLS, d),
                    Key::Enter | Key::Char(' ') => return self.open(self.sel),
                    _ => {}
                }
            }
            UiEvent::PointerMove { x, y } => { if let Some(i) = self.tile_at(x, y) { self.select(i, d); } }
            UiEvent::Click { x, y } => { if let Some(i) = self.tile_at(x, y) { self.select(i, d); return self.open(i); } }
            _ => {}
        }
        Action::None
    }

    fn tick(&mut self, _s: &mut Shared, d: &mut Damage) -> bool {
        let mut moving = false;
        if self.intro.step() {
            moving = true;
            for i in 0..N { d.add(self.lay.tile_damage(i)); }
        }
        for i in 0..N {
            if self.grow[i].step() { moving = true; d.add(self.lay.tile_damage(i)); }
        }
        let mut ring_moved = false;
        for sp in self.ring.iter_mut() { ring_moved |= sp.step(); }
        if ring_moved {
            moving = true;
            d.add(self.ring_rect.inflate(8));
            self.ring_rect = Rect::new(fx_floor(self.ring[0].value), fx_floor(self.ring[1].value), fx_ceil(self.ring[2].value), fx_ceil(self.ring[3].value));
            d.add(self.ring_rect.inflate(8));
        }
        moving
    }

    fn second(&mut self, s: &mut Shared, d: &mut Damage) {
        let m = s.local_time.div_euclid(60);
        if m != self.last_min {
            self.last_min = m;
            d.add(self.footer_rect(s));
            d.add(self.lay.tile_damage(3)); // the clock tile's hands
        }
        // The avatar in the footer blinks now and then.
        if s.local_time % 4 == 0 { d.add(Rect::new(0, self.lay.footer_y - 120, s.w / 4, s.h - self.lay.footer_y + 120)); }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        background(c, hex(0xFBFCFF), hex(0xEDF1F9));
        let lay = &self.lay;
        let dt = super::clock::from_unix(s.local_time);

        // ---- Header: greeting and date ----
        let hy = lay.y0 / 2 - 20;
        c.text(&font::LARGE, lay.x0, hy, "Hello!", INK);
        c.text(&font::MEDIUM, lay.x0 + 16 * 7 + 8, hy + 6, "What shall we do today?", INK_SOFT);
        let mut date = FmtBuf::<32>::new();
        let _ = write!(date, "{} {} {}", super::clock::WEEKDAYS[dt.weekday as usize], dt.day, super::clock::MONTHS[(dt.month - 1) as usize]);
        let dw = font::LARGE.text_width(date.as_str()) as i32;
        c.text(&font::LARGE, lay.x0 + lay.tile_w * COLS as i32 + lay.gap * (COLS as i32 - 1) - dw, hy, date.as_str(), INK_SOFT);

        // ---- Tiles ----
        for i in 0..N {
            let base = lay.tile(i);
            if !lay.tile_damage(i).overlaps(&c.clip) { continue; }
            let sc = self.scale(i);
            if sc <= 8 { continue; }
            // Scale around the tile centre.
            let (cx, cy) = (base.x + base.w / 2, base.y + base.h / 2);
            let (w, h) = (fx_mul(fx(base.w), sc), fx_mul(fx(base.h), sc));
            let r = Rect::new(cx - fx_round(w) / 2, cy - fx_round(h) / 2, fx_round(w), fx_round(h));
            let rad = fx_round(fx_mul(fx(lay.radius), sc));
            let t = &TILES[i];
            if t.app.is_none() {
                c.fill_round_rect(r, rad, with_alpha(0xFFFFFF, 140));
                c.stroke_round_rect(r, rad, 3, hex(0xE1E6F0));
                icons::empty(c, cx, cy, r.h / 2);
                continue;
            }
            c.shadow(r, rad, lay.gap / 3, 60);
            c.fill_round_rect(r, rad, WHITE);
            let inner = r.inflate(-(r.h / 18).max(4));
            c.fill_round_rect(inner, rad * 3 / 4, mix(t.color, WHITE, 90));
            let icon_size = r.h * 9 / 20;
            let (icx, icy) = (cx, r.y + r.h * 2 / 5);
            match t.app {
                Some(AppId::Write) => icons::write(c, icx, icy, icon_size),
                Some(AppId::Files) => icons::files(c, icx, icy, icon_size),
                Some(AppId::Clock) => icons::clock(c, icx, icy, icon_size, dt.hour, dt.min),
                Some(AppId::Settings) => icons::settings(c, icx, icy, icon_size),
                Some(AppId::About) => icons::about(c, icx, icy, icon_size),
                Some(AppId::Pals) => match self.avatar_hook {
                    Some(f) => f(c, icx, icy + icon_size / 10, icon_size * 12 / 10, s.frame),
                    None => icons::about(c, icx, icy, icon_size),
                },
                _ => {}
            }
            c.text_centered(&font::LARGE, cx, r.bottom() - r.h / 4 - 4, t.name, INK);
        }

        // ---- Selection ring (glides between tiles) ----
        if !self.intro.running {
            let rr = Rect::new(fx_round(self.ring[0].value), fx_round(self.ring[1].value), fx_round(self.ring[2].value), fx_round(self.ring[3].value));
            if rr.inflate(8).overlaps(&c.clip) {
                c.stroke_round_rect(rr.inflate(4), lay.radius + 14, 4, with_alpha(ACCENT, 70));
                c.stroke_round_rect(rr, lay.radius + 10, 6, ACCENT);
            }
        }

        // ---- Footer: a soft white band with the big clock ----
        let fy = lay.footer_y;
        let foot = Rect::new(0, fy, s.w, s.h - fy);
        if foot.inflate(130).overlaps(&c.clip) {
            let ry = (s.h - fy) * 2;
            c.fill_rect(Rect::new(0, fy + 38, s.w, 2), hex(0xDDE3EE));
            c.fill_ellipse_fx(fx(s.w / 2), fx(fy + ry - 2), fx(s.w / 4 + 2), fx(ry + 2), hex(0xDDE3EE));
            c.fill_rect(Rect::new(0, fy + 40, s.w, s.h - fy - 40), WHITE);
            c.fill_ellipse_fx(fx(s.w / 2), fx(fy + ry), fx(s.w / 4), fx(ry), WHITE);
            let mut tm = FmtBuf::<8>::new();
            let _ = write!(tm, "{}:{:02}", dt.hour, dt.min);
            c.text_centered(&font::HUGE, s.w / 2, fy + 30, tm.as_str(), INK);
            let mut dd = FmtBuf::<40>::new();
            let _ = write!(dd, "{}, {} {} {}", super::clock::WEEKDAYS[dt.weekday as usize], dt.day, super::clock::MONTHS[(dt.month - 1) as usize], dt.year);
            c.text_centered(&font::MEDIUM, s.w / 2, fy + 104, dd.as_str(), INK_SOFT);
            // Your avatar stands in the bottom-left corner (milestone 7).
            if let Some(f) = self.avatar_hook { f(c, s.w / 8, fy + 20, (s.h - fy) * 13 / 10, s.frame); }
        }
    }

    fn color(&self) -> Color { BG }
}
