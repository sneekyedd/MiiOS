//! A simple page for apps that aren't built yet.

use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color};
use crate::input::Key;

pub struct Placeholder { pub title: &'static str, pub color: Color, pub text: &'static str, hover: bool }

impl Placeholder {
    pub const fn new(title: &'static str, color: Color, text: &'static str) -> Self { Self { title, color, text, hover: false } }
}

impl App for Placeholder {
    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key: Key::Escape | Key::Backspace, .. } => Action::Back,
            UiEvent::Click { x, y } if widgets::back_button().contains(x, y) => Action::Back,
            UiEvent::PointerMove { x, y } => {
                let h = widgets::back_button().contains(x, y);
                if h != self.hover { self.hover = h; d.add(widgets::back_button().inflate(12)); }
                Action::None
            }
            _ => Action::None,
        }
    }
    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color);
        widgets::header(c, self.title, self.color, self.hover);
        c.text_centered(&font::LARGE, s.w / 2, s.h / 2 - 40, self.text, INK_SOFT);
        c.text_centered(&font::MEDIUM, s.w / 2, s.h / 2 + 10, "Press Esc to go back.", INK_SOFT);
    }
    fn color(&self) -> Color { self.color }
}
