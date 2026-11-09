//! The window: minifb gives us a pixel buffer to show, plus keyboard and
//! mouse events, which we translate into MiiOS input events.

use crate::input::{Event, Key, Mods, EVENTS};
use minifb::{InputCallback, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

thread_local! {
    static WIN: RefCell<Option<Window>> = const { RefCell::new(None) };
}
static mut FRONT: Vec<u32> = Vec::new();
static SIZE: Mutex<(usize, usize)> = Mutex::new((0, 0));
/// Typed characters arrive through this callback (it knows Shift and the keyboard layout).
static CHARS: Mutex<Vec<char>> = Mutex::new(Vec::new());
static LAST_PTR: Mutex<(i32, i32, bool, bool, bool)> = Mutex::new((-1, -1, false, false, false));

struct Chars;
impl InputCallback for Chars {
    fn add_char(&mut self, c: u32) {
        if let Some(ch) = char::from_u32(c) {
            if (' '..='~').contains(&ch) { CHARS.lock().unwrap().push(ch); }
        }
    }
}

pub fn open(w: i32, h: i32) {
    let mut win = Window::new("MiiOS", w as usize, h as usize, WindowOptions { borderless: true, ..WindowOptions::default() })
        .expect("could not open a window (is Sway running?)");
    win.set_input_callback(Box::new(Chars));
    win.set_cursor_visibility(false); // MiiOS draws its own friendly pointer
    #[allow(static_mut_refs)]
    unsafe { FRONT = vec![0; (w * h) as usize]; }
    *SIZE.lock().unwrap() = (w as usize, h as usize);
    WIN.with(|c| *c.borrow_mut() = Some(win));
}

/// The buffer the UI copies finished frames into.
pub fn front() -> *mut u32 {
    #[allow(static_mut_refs)]
    unsafe { FRONT.as_mut_ptr() }
}

fn special(k: minifb::Key) -> Option<Key> {
    use minifb::Key as K;
    Some(match k {
        K::Enter | K::NumPadEnter => Key::Enter, K::Backspace => Key::Backspace, K::Tab => Key::Tab,
        K::Escape => Key::Escape, K::Delete => Key::Delete,
        K::Up => Key::Up, K::Down => Key::Down, K::Left => Key::Left, K::Right => Key::Right,
        K::Home => Key::Home, K::End => Key::End, K::PageUp => Key::PageUp, K::PageDown => Key::PageDown,
        K::F1 => Key::F(1), K::F2 => Key::F(2), K::F3 => Key::F(3), K::F4 => Key::F(4), K::F5 => Key::F(5), K::F6 => Key::F(6),
        K::F7 => Key::F(7), K::F8 => Key::F(8), K::F9 => Key::F(9), K::F10 => Key::F(10), K::F11 => Key::F(11), K::F12 => Key::F(12),
        _ => return None,
    })
}

/// Letter keys, for Ctrl+letter shortcuts (the character callback doesn't report those).
fn letter(k: minifb::Key) -> Option<char> {
    let i = k as u32;
    let a = minifb::Key::A as u32;
    if i >= a && i <= minifb::Key::Z as u32 { Some((b'a' + (i - a) as u8) as char) } else { None }
}

fn pump(win: &mut Window) {
    let mods = Mods {
        shift: win.is_key_down(minifb::Key::LeftShift) || win.is_key_down(minifb::Key::RightShift),
        ctrl: win.is_key_down(minifb::Key::LeftCtrl) || win.is_key_down(minifb::Key::RightCtrl),
        alt: win.is_key_down(minifb::Key::LeftAlt) || win.is_key_down(minifb::Key::RightAlt),
    };
    for k in win.get_keys_pressed(KeyRepeat::Yes) {
        if let Some(key) = special(k) { EVENTS.push(Event::Key { key, pressed: true, mods }); }
        else if mods.ctrl { if let Some(c) = letter(k) { EVENTS.push(Event::Key { key: Key::Char(c), pressed: true, mods }); } }
    }
    let typed: Vec<char> = std::mem::take(&mut *CHARS.lock().unwrap());
    if !mods.ctrl { for c in typed { EVENTS.push(Event::Key { key: Key::Char(c), pressed: true, mods }); } }

    let (x, y) = win.get_mouse_pos(MouseMode::Clamp).map(|(x, y)| (x as i32, y as i32)).unwrap_or((-1, -1));
    let (l, r, m) = (win.get_mouse_down(MouseButton::Left), win.get_mouse_down(MouseButton::Right), win.get_mouse_down(MouseButton::Middle));
    let wheel = win.get_scroll_wheel().map(|(_, dy)| if dy > 0.0 { -3 } else if dy < 0.0 { 3 } else { 0 }).unwrap_or(0);
    let mut last = LAST_PTR.lock().unwrap();
    if x >= 0 && ((x, y, l, r, m) != *last || wheel != 0) {
        *last = (x, y, l, r, m);
        EVENTS.push(Event::Pointer { x, y, wheel, left: l, right: r, middle: m });
    }
}

/// Show the latest frame, then wait up to `ms` for keyboard/mouse input.
pub fn present_and_wait(ms: u64) {
    let (w, h) = *SIZE.lock().unwrap();
    let end = Instant::now() + Duration::from_millis(ms);
    WIN.with(|c| {
        let mut b = c.borrow_mut();
        let Some(win) = b.as_mut() else { return };
        if !win.is_open() { std::process::exit(0); }
        #[allow(static_mut_refs)]
        let _ = win.update_with_buffer(unsafe { &FRONT }, w, h);
        pump(win);
        while !crate::input::pending() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(4));
            win.update();
            pump(win);
        }
    });
}
