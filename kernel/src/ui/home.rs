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
use super::avatar::{self, Pose};
use super::{icons, Action, App, AppId, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use core::fmt::Write;
extern crate alloc;
use alloc::vec::Vec;

pub const COLS: usize = 4;
pub const ROWS: usize = 3;
const N: usize = COLS * ROWS;

#[derive(Clone, Copy)]
pub struct TileInfo {
    pub app: Option<AppId>,          // a built-in MiiOS app...
    pub launch: Option<&'static str>, // ...or an outside program (Linux edition only)
    pub name: &'static str,
    pub color: Color,
}

const fn app(a: AppId, name: &'static str, color: Color) -> TileInfo { TileInfo { app: Some(a), launch: None, name, color } }
const fn ext(l: &'static str, name: &'static str, color: Color) -> TileInfo { TileInfo { app: None, launch: Some(l), name, color } }
const EMPTY: TileInfo = TileInfo { app: None, launch: None, name: "", color: 0 };

/// Tiles on the bare-metal edition.
const BARE_TILES: [TileInfo; N] = [
    app(AppId::Pals, "Pals", PASTELS[0]), app(AppId::Write, "Write", PASTELS[2]),
    app(AppId::Files, "Files", PASTELS[4]), app(AppId::Clock, "Clock", PASTELS[1]),
    app(AppId::Settings, "Settings", PASTELS[5]), app(AppId::About, "About", PASTELS[3]),
    EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
];

/// Tiles on the Linux edition: your 3D Miis, Mii Hub, the web, Wi-Fi and a terminal.
const LINUX_TILES: [TileInfo; N] = [
    app(AppId::Miis, "Miis", PASTELS[0]), ext("mii-hub", "Mii Hub", PASTELS[3]),
    ext("web", "Web", PASTELS[4]), app(AppId::Wifi, "Wi-Fi", PASTELS[7]),
    app(AppId::Write, "Write", PASTELS[2]), app(AppId::Files, "Files", PASTELS[4]),
    app(AppId::Clock, "Clock", PASTELS[1]), app(AppId::Settings, "Settings", PASTELS[5]),
    ext("terminal", "Terminal", hex(0xDDE1EA)), app(AppId::About, "About", PASTELS[3]),
    EMPTY, EMPTY,
];

pub fn tiles() -> &'static [TileInfo; N] { if crate::host::HOSTED { &LINUX_TILES } else { &BARE_TILES } }

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
    wave: Tween,          // your pal waves hello
    blink: Tween,         // someone blinks
    blink_who: usize,
    plaza: super::plaza::Plaza, // your 3D Miis (Linux edition)
    w: i32,
    h: i32,
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
            wave: Tween::default(),
            blink: Tween::default(),
            blink_who: 0,
            plaza: super::plaza::Plaza::new(crate::arch::cpu::rdtsc()),
            w, h,
        }
    }

    fn select(&mut self, i: usize, d: &mut Damage) {
        if i == self.sel { return; }
        if self.plaza_on() { self.plaza.tile_selected(self.lay.tile(i)); }
        crate::host::sound(crate::host::Sfx::Hover, 256 + (i as i32 % 4) * 24); // a slightly different note per column
        d.add(self.lay.tile_damage(self.sel));
        d.add(self.lay.tile_damage(i));
        self.grow[self.sel].set(ONE);
        self.sel = i;
        self.grow[i].set(ONE + 16); // +6%
        let t = self.lay.tile(i).inflate(10);
        self.ring[0].set(fx(t.x)); self.ring[1].set(fx(t.y));
        self.ring[2].set(fx(t.w)); self.ring[3].set(fx(t.h));
    }

    fn open(&mut self, i: usize) -> Action {
        let t = tiles()[i];
        if self.plaza_on() && (t.app.is_some() || t.launch.is_some()) { self.plaza.app_opened(self.lay.tile(i)); }
        match (t.app, t.launch) {
            (Some(a), _) => Action::Open(a, self.lay.tile(i)),
            (None, Some(l)) => { Action::Launch(l) }
            _ => Action::None,
        }
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
        let i = tiles().iter().position(|t| t.app == Some(app)).unwrap_or(0);
        self.lay.tile(i)
    }

    fn footer_rect(&self, s: &Shared) -> Rect { Rect::new(0, self.lay.footer_y, s.w, s.h - self.lay.footer_y) }

    /// Where pal `i` stands in the footer: (centre x, feet y, height).
    fn pal_spot(&self, i: usize) -> (i32, i32, i32) {
        let fh = self.h - self.lay.footer_y;
        if i == 0 { (self.w / 14, self.h - 14, fh * 135 / 100) }
        else { (self.w - self.w / 16 - (i as i32 - 1) * (fh * 7 / 10), self.h - 14, fh) }
    }

    /// The plaza replaces the drawn pals when you have 3D Miis.
    fn plaza_on(&self) -> bool { crate::host::HOSTED && self.plaza.active() }
    fn sync_plaza(&mut self) {
        if !crate::host::HOSTED { return; }
        let tiles: Vec<Rect> = (0..N).filter(|&i| tiles()[i].app.is_some() || tiles()[i].launch.is_some()).map(|i| self.lay.tile(i)).collect();
        self.plaza.sync(self.w, self.h, self.lay.y0 + 10, &tiles);
    }

    fn pal_rect(&self, i: usize) -> Rect { let (x, y, h) = self.pal_spot(i); avatar::body_rect(x, y, h) }
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
    fn enter(&mut self, s: &mut Shared) {
        self.lay = layout(s.w, s.h);
        self.w = s.w; self.h = s.h;
        self.wave.start(90);
        self.sync_plaza();
    }

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
                    Key::Char('h') if self.plaza_on() => self.plaza.greet_all(d),
                    _ => {}
                }
            }
            UiEvent::PointerMove { x, y } => {
                if self.plaza_on() { self.plaza.pointer(x, y, d); }
                if let Some(i) = self.tile_at(x, y) { self.select(i, d); }
            }
            UiEvent::Click { x, y } => {
                if self.plaza_on() && self.plaza.click(x, y, d) { return Action::None; } // picked up / said hi to a Mii
                if let Some(i) = self.tile_at(x, y) { self.select(i, d); return self.open(i); }
            }
            UiEvent::Release { x, y } => { if self.plaza_on() { self.plaza.release(x, y); } }
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
        if self.plaza_on() {
            if self.wave.running && self.wave.t == 0 { self.plaza.greet_all(d); }
            self.wave.step();
            moving |= self.plaza.tick(d);
        } else {
            if self.wave.step() { moving = true; d.add(self.pal_rect(0)); }
            if self.blink.step() { moving = true; d.add(self.pal_rect(self.blink_who)); }
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
        if crate::host::HOSTED {
            // New Miis imported? Refresh the plaza. Then let the Miis decide what to do.
            if crate::host::miis_rescan() || !self.plaza.active() { self.sync_plaza(); d.add(self.footer_rect(s).inflate(400)); }
            self.plaza.second(d, s.local_time);
            if self.plaza_on() { return; }
        }
        // Pals blink now and then (a different one each time).
        if s.local_time % 3 == 0 && !self.blink.running {
            self.blink_who = (s.local_time / 3) as usize % s.pals.count.min(5);
            self.blink.start(8);
        }
    }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        background(c, hex(0xFBFCFF), hex(0xEDF1F9));
        let lay = &self.lay;
        let dt = super::clock::from_unix(s.local_time);

        // ---- Header: greeting and date ----
        let hy = lay.y0 / 2 - 20;
        let mut hello = FmtBuf::<32>::new();
        match self.plaza.me_name().filter(|_| self.plaza_on()) {
            Some(n) => { let _ = write!(hello, "Hello, {}!", n); }
            None if self.plaza_on() => { let _ = write!(hello, "Hello!"); }
            None => { let _ = write!(hello, "Hello, {}!", s.pals.me().name()); }
        }
        let hx = c.text(&font::LARGE, lay.x0, hy, hello.as_str(), INK);
        c.text(&font::MEDIUM, hx + 16, hy + 6, "What shall we do today?", INK_SOFT);
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
            let t = &tiles()[i];
            if t.app.is_none() && t.launch.is_none() {
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
                Some(AppId::Pals) => avatar::draw_head(c, icx, icy + icon_size / 10, icon_size * 11 / 10, s.pals.me(), Pose::default()),
                Some(AppId::Wifi) => icons::wifi(c, icx, icy, icon_size),
                Some(AppId::Miis) => {
                    // Your own Mii (or the first one) as the icon.
                    let who = crate::host::mii_me().unwrap_or(0);
                    if crate::host::miis().is_empty() { icons::about(c, icx, icy, icon_size); }
                    else {
                        let ih = icon_size * 15 / 10;
                        crate::host::mii_draw(c, who, crate::host::MiiPose::default(), Rect::new(icx - ih * 3 / 8, icy + icon_size / 2 - ih, ih * 3 / 4, ih));
                    }
                }
                _ => match t.launch {
                    Some("web") => icons::web(c, icx, icy, icon_size),
                    Some("terminal") => icons::terminal(c, icx, icy, icon_size),
                    Some(name) => match crate::host::app_icon(name) {
                        Some(img) => { let sz = icon_size * 12 / 10; c.blit_image(img, Rect::new(icx - sz / 2, icy - sz / 2, sz, sz)); }
                        None => icons::about(c, icx, icy, icon_size),
                    },
                    None => {}
                },
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
        }
        // ---- Your 3D Miis walking around (Linux edition) ----
        if self.plaza_on() { self.plaza.draw(c); return; }
        // ---- Pals standing in the footer: you on the left, friends on the right ----
        for i in 0..s.pals.count.min(5) {
            let (x, y, h) = self.pal_spot(i);
            let pose = Pose {
                blink: self.blink.running && self.blink_who == i,
                wave: if i == 0 && self.wave.running { self.wave.t.max(1) } else { 0 },
                bounce: 0,
            };
            avatar::draw_body(c, x, y, h, &s.pals.list[i], pose);
        }
    }

    fn color(&self) -> Color { BG }
}
