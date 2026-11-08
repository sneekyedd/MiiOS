//! The user interface: the shell (app switching, pointer) and the apps.

pub mod anim;
pub mod clock;
pub mod home;
pub mod icons;
pub mod placeholder;
pub mod shell;
pub mod widgets;

use crate::gfx::screen::Damage;
use crate::gfx::{Canvas, Color, Rect};
use crate::input::{Key, Mods};

/// Input as apps see it: the shell has already turned raw mouse movement
/// into an absolute pointer position and button presses into clicks.
#[derive(Clone, Copy, Debug)]
pub enum UiEvent {
    Key { key: Key, mods: Mods },
    PointerMove { x: i32, y: i32 },
    Click { x: i32, y: i32 },
    Release { x: i32, y: i32 },
    Scroll { dy: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppId { Home, Pals, Write, Files, Clock, Settings, About }

/// What an app wants the shell to do after an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action { None, Open(AppId, Rect), Back }

/// Data every app can see and change.
pub struct Shared {
    pub w: i32,
    pub h: i32,
    /// Local time in seconds (Unix time + time zone offset).
    pub local_time: i64,
    pub tz_minutes: i32,
    pub frame: u64,
}

pub trait App {
    /// Called when the app is opened.
    fn enter(&mut self, _s: &mut Shared) {}
    fn event(&mut self, e: &UiEvent, s: &mut Shared, d: &mut Damage) -> Action;
    /// One 60 Hz animation step. Return true while something is still moving.
    fn tick(&mut self, _s: &mut Shared, _d: &mut Damage) -> bool { false }
    /// Called once per second (for clocks etc.).
    fn second(&mut self, _s: &mut Shared, _d: &mut Damage) {}
    /// Draw everything; the canvas clip limits what's actually touched.
    fn draw(&self, c: &mut Canvas, s: &Shared);
    /// Main colour, used for the opening animation.
    fn color(&self) -> Color;
}
