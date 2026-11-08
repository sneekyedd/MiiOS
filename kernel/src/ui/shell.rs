//! The shell: owns the screen, turns raw input into UI events, runs the
//! current app, plays the open/close animations and draws the pointer.
//!
//! The main loop, in plain words:
//!   1. Handle all waiting input events.
//!   2. Step animations at a fixed 60 Hz (catching up if we were late).
//!   3. Once per second, let the app update clocks.
//!   4. Redraw only the damaged rectangles and copy them to the screen.
//!   5. Sleep until input arrives or the next frame/second is due.
//!      While nothing moves, the CPU sits halted almost all the time.

extern crate alloc;
use super::anim::Tween;
use super::home::Home;
use super::{Action, App, AppId, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::screen::Screen;
use crate::gfx::{cursor, Canvas, Rect};
use crate::input::{self, Event, Key};
use crate::{log, task, time};
use alloc::boxed::Box;
use alloc::vec::Vec;

enum Transition {
    None,
    Opening { to: AppId, from: Rect, t: Tween },
    Closing { from: AppId, to: Rect, t: Tween },
}

pub struct Shell {
    screen: Screen,
    shared: Shared,
    home: Home,
    apps: Vec<(AppId, Box<dyn App>)>,
    current: AppId,
    trans: Transition,
    overlay: Rect,
    px: i32,
    py: i32,
    pointer_visible: bool,
    buttons: (bool, bool, bool),
    anim_origin: u64,
    steps_done: u64,
    animating: bool,
    last_second: i64,
    // performance counters
    frame_us_total: u64,
    frames: u64,
}

impl Shell {
    pub fn new(screen: Screen, home: Home, apps: Vec<(AppId, Box<dyn App>)>) -> Shell {
        let (w, h) = (screen.width(), screen.height());
        let shared = Shared { w, h, local_time: time::unix_now(), tz_minutes: 0, frame: 0, pals: super::avatar::PalList::new(crate::arch::cpu::rdtsc()), open_doc: None };
        Shell {
            screen, shared, home, apps, current: AppId::Home, trans: Transition::None, overlay: Rect::default(),
            px: w / 2, py: h / 2, pointer_visible: false, buttons: (false, false, false),
            anim_origin: time::ticks_ms(), steps_done: 0, animating: true, last_second: -1, frame_us_total: 0, frames: 0,
        }
    }

    pub fn shared(&mut self) -> &mut Shared { &mut self.shared }

    fn app(&mut self, id: AppId) -> &mut dyn App {
        if id == AppId::Home { return &mut self.home; }
        let i = self.apps.iter().position(|(a, _)| *a == id).unwrap_or(0);
        &mut *self.apps[i].1
    }

    fn app_ref(&self, id: AppId) -> &dyn App { find(&self.home, &self.apps, id) }

    /// Raw mouse/keyboard event -> UI events for the current app.
    fn handle_input(&mut self, e: Event) {
        match e {
            Event::Key { key, pressed, mods } => {
                if !pressed { return; }
                if matches!(key, Key::Shift | Key::Ctrl | Key::Alt | Key::CapsLock) { return; }
                if self.pointer_visible && !matches!(self.current, AppId::Home) {
                    // Typing hides the pointer so it doesn't sit on your text.
                    self.pointer_visible = false;
                    self.screen.damage.add(cursor::rect(self.px, self.py));
                }
                self.dispatch(UiEvent::Key { key, mods });
            }
            Event::Mouse { dx, dy, wheel, left, right, middle } => {
                // TrackPoint-style scrolling: hold the middle button and push.
                if middle && (dx != 0 || dy != 0) {
                    if dy != 0 { self.dispatch(UiEvent::Scroll { dy: dy.signum() }); }
                    self.buttons = (left, right, middle);
                    return;
                }
                if wheel != 0 { self.dispatch(UiEvent::Scroll { dy: wheel.signum() * 3 }); }
                if dx != 0 || dy != 0 || !self.pointer_visible {
                    let accel = |d: i32| { let m = d.abs(); if m < 3 { d } else if m < 8 { d * 2 } else { d * 3 } };
                    self.screen.damage.add(cursor::rect(self.px, self.py));
                    self.px = (self.px + accel(dx)).clamp(0, self.shared.w - 1);
                    self.py = (self.py + accel(dy)).clamp(0, self.shared.h - 1);
                    self.pointer_visible = true;
                    self.screen.damage.add(cursor::rect(self.px, self.py));
                    self.dispatch(UiEvent::PointerMove { x: self.px, y: self.py });
                }
                if left && !self.buttons.0 { self.dispatch(UiEvent::Click { x: self.px, y: self.py }); }
                if !left && self.buttons.0 { self.dispatch(UiEvent::Release { x: self.px, y: self.py }); }
                self.buttons = (left, right, middle);
            }
        }
    }

    fn dispatch(&mut self, e: UiEvent) {
        if !matches!(self.trans, Transition::None) { return; } // ignore input mid-animation
        let cur = self.current;
        let shared = &mut self.shared as *mut Shared;
        let damage = &mut self.screen.damage as *mut crate::gfx::screen::Damage;
        // Safety: the app, shared state and damage list are separate objects.
        let action = unsafe { self.app(cur).event(&e, &mut *shared, &mut *damage) };
        match action {
            Action::None => {}
            Action::Open(id, from) => {
                let mut t = Tween::default();
                t.start(14); // ~0.23 s
                self.trans = Transition::Opening { to: id, from, t };
                self.overlay = from;
                self.animating = true;
            }
            Action::Back => {
                let to = self.home.lay_tile_of(cur);
                let mut t = Tween::default();
                t.start(12);
                self.trans = Transition::Closing { from: cur, to, t };
                self.current = AppId::Home;
                let s = &mut self.shared as *mut Shared;
                unsafe { self.home.enter(&mut *s) };
                self.overlay = Rect::new(0, 0, self.shared.w, self.shared.h);
                self.screen.damage.add_all();
                self.animating = true;
            }
        }
    }

    /// One 60 Hz animation step. Returns true if anything is moving.
    fn step(&mut self) -> bool {
        self.shared.frame += 1;
        let full = Rect::new(0, 0, self.shared.w, self.shared.h);
        let mut moving = match &mut self.trans {
            Transition::None => false,
            Transition::Opening { to, from, t } => {
                t.step();
                let p = ease_out(t.p());
                let r = lerp_rect(*from, full, p);
                self.screen.damage.add(r);
                self.overlay = r;
                if !t.running {
                    let id = *to;
                    self.trans = Transition::None;
                    self.current = id;
                    let s = &mut self.shared as *mut Shared;
                    unsafe { self.app(id).enter(&mut *s) };
                    self.screen.damage.add_all();
                }
                true
            }
            Transition::Closing { to, t, .. } => {
                t.step();
                let p = ease_out(t.p());
                let r = lerp_rect(full, *to, p);
                self.screen.damage.add(self.overlay);
                self.overlay = r;
                if !t.running { self.trans = Transition::None; }
                true
            }
        };
        let cur = self.current;
        let shared = &mut self.shared as *mut Shared;
        let damage = &mut self.screen.damage as *mut crate::gfx::screen::Damage;
        moving |= unsafe { self.app(cur).tick(&mut *shared, &mut *damage) };
        moving
    }

    fn render(&mut self) {
        let t0 = time::uptime_us();
        let cur = self.current;
        let (px, py, pv) = (self.px, self.py, self.pointer_visible);
        let overlay = self.overlay;
        let trans_color = match &self.trans {
            Transition::Opening { to, .. } => Some(self.app_ref(*to).color()),
            Transition::Closing { from, .. } => Some(self.app_ref(*from).color()),
            Transition::None => None,
        };
        let full = Rect::new(0, 0, self.shared.w, self.shared.h);
        let app = find(&self.home, &self.apps, cur);
        let shared = &self.shared;
        let px_drawn = self.screen.render(|c: &mut Canvas| {
            match trans_color {
                Some(col) => {
                    let p = progress(overlay, full);
                    let rad = 40 - 40 * p / 256;
                    // Skip drawing what's underneath if the overlay hides it completely.
                    let inner = overlay.inflate(-rad);
                    if !(inner.contains(c.clip.x, c.clip.y) && inner.contains(c.clip.right() - 1, c.clip.bottom() - 1)) {
                        app.draw(c, shared);
                    }
                    c.fill_round_rect(overlay, rad, mix(col, WHITE, 150));
                }
                None => app.draw(c, shared),
            }
            if pv { cursor::draw(c, px, py); }
        });
        if px_drawn > 0 {
            self.frames += 1;
            self.frame_us_total += time::uptime_us() - t0;
            if self.frames % 300 == 0 {
                log!("ui: {} frames, average {} us per frame", self.frames, self.frame_us_total / self.frames);
            }
        }
    }

    /// Run forever.
    pub fn run(&mut self) -> ! {
        self.screen.damage.add_all();
        let s = &mut self.shared as *mut Shared;
        unsafe { self.home.enter(&mut *s) };
        loop {
            while let Some(e) = input::next_event() { self.handle_input(e); }

            // Storage just came online: load saved pals and settings, redraw everything.
            if crate::fs::take_just_mounted() {
                super::load_profile(&mut self.shared);
                self.screen.damage.add_all();
            }

            // Fixed 60 Hz animation steps, counted from `anim_origin` so the
            // speed is exact even though we wake up at slightly uneven times.
            let now = time::ticks_ms();
            if !self.animating {
                // Idle until now: restart the clock and allow one step right away.
                self.anim_origin = now.saturating_sub(17);
                self.steps_done = 0;
            }
            let due = ((now - self.anim_origin) * 60 / 1000).saturating_sub(self.steps_done);
            let mut moving = false;
            for _ in 0..due.min(4) { moving |= self.step(); }
            self.steps_done += due; // if we fell far behind, skip ahead rather than rush
            self.animating = moving;

            // Once a second: clocks.
            self.shared.local_time = time::unix_now() + self.shared.tz_minutes as i64 * 60;
            if self.shared.local_time != self.last_second {
                self.last_second = self.shared.local_time;
                let cur = self.current;
                let shared = &mut self.shared as *mut Shared;
                let damage = &mut self.screen.damage as *mut crate::gfx::screen::Damage;
                unsafe { self.app(cur).second(&mut *shared, &mut *damage) };
            }

            self.render();

            if self.animating {
                task::wait_input(16);
            } else {
                let ms_to_next_second = 1000 - (time::uptime_us() / 1000 % 1000);
                task::wait_input(ms_to_next_second.max(1));
            }
        }
    }
}

/// Look up an app by id (a free function so it only borrows these two fields).
fn find<'a>(home: &'a Home, apps: &'a [(AppId, Box<dyn App>)], id: AppId) -> &'a dyn App {
    if id == AppId::Home { return home; }
    let i = apps.iter().position(|(a, _)| *a == id).unwrap_or(0);
    &*apps[i].1
}

fn lerp_rect(a: Rect, b: Rect, t: i32) -> Rect {
    Rect::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t), lerp(a.w, b.w, t), lerp(a.h, b.h, t))
}

/// How far `r` has grown toward `full`, 0..=256 (for corner radius).
fn progress(r: Rect, full: Rect) -> i32 { (r.w * 256 / full.w.max(1)).clamp(0, 256) }
