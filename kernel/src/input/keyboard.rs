//! Keyboard driver: turns "scancodes" into keys and characters (US layout).
//!
//! Each key sends a number when pressed ("make code") and the same number
//! + 0x80 when released ("break code"). Some newer keys (arrows, Delete...)
//! first send 0xE0 to say "the next code is from the extended set".

use super::{Event, Key, Mods, EVENTS};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static EXTENDED: AtomicBool = AtomicBool::new(false);
// Bit flags for modifier keys: 1 lshift, 2 rshift, 4 ctrl, 8 alt, 16 caps lock
static MODS: AtomicU8 = AtomicU8::new(0);

// Scancode -> (normal, shifted) characters for US layout. 0 = not a character.
const MAP: [(u8, u8); 0x3A] = {
    let mut m = [(0u8, 0u8); 0x3A];
    let normal = b"\x00\x1b1234567890-=\x08\tqwertyuiop[]\n\x00asdfghjkl;'`\x00\\zxcvbnm,./\x00*\x00 ";
    let shifted = b"\x00\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\x00ASDFGHJKL:\"~\x00|ZXCVBNM<>?\x00*\x00 ";
    let mut i = 0;
    while i < 0x3A { m[i] = (normal[i], shifted[i]); i += 1; }
    m
};

pub fn byte(b: u8) {
    if b == 0xE0 { EXTENDED.store(true, Ordering::Relaxed); return; }
    if b == 0xFA || b == 0xFE || b == 0x00 || b == 0xFF { return; } // ACK/resend/errors
    let ext = EXTENDED.swap(false, Ordering::Relaxed);
    let pressed = b & 0x80 == 0;
    let code = b & 0x7F;
    let mut mods = MODS.load(Ordering::Relaxed);

    let key = if ext {
        match code {
            0x48 => Key::Up, 0x50 => Key::Down, 0x4B => Key::Left, 0x4D => Key::Right,
            0x47 => Key::Home, 0x4F => Key::End, 0x49 => Key::PageUp, 0x51 => Key::PageDown,
            0x53 => Key::Delete, 0x1C => Key::Enter, 0x1D => Key::Ctrl, 0x38 => Key::Alt,
            0x5B | 0x5C => Key::Super, 0x35 => Key::Char('/'),
            0x2A | 0x37 => return, // parts of the Print Screen sequence: ignore
            c => Key::Unknown(c),
        }
    } else {
        match code {
            0x2A | 0x36 => Key::Shift,
            0x1D => Key::Ctrl,
            0x38 => Key::Alt,
            0x3A => Key::CapsLock,
            0x01 => Key::Escape,
            0x0E => Key::Backspace,
            0x0F => Key::Tab,
            0x1C => Key::Enter,
            0x3B..=0x44 => Key::F(code - 0x3B + 1),
            0x57 => Key::F(11), 0x58 => Key::F(12),
            c if (c as usize) < MAP.len() && MAP[c as usize].0 != 0 => {
                let (n, s) = MAP[c as usize];
                let shift = mods & 3 != 0;
                let caps = mods & 16 != 0;
                let letter = n.is_ascii_lowercase();
                let ch = if shift ^ (caps && letter) { s } else { n };
                Key::Char(ch as char)
            }
            c => Key::Unknown(c),
        }
    };

    // Track modifier keys.
    let bit = match (key, code) {
        (Key::Shift, 0x2A) => 1, (Key::Shift, _) => 2, (Key::Ctrl, _) => 4, (Key::Alt, _) => 8, _ => 0,
    };
    if bit != 0 { if pressed { mods |= bit } else { mods &= !bit } }
    if key == Key::CapsLock && pressed { mods ^= 16; }
    MODS.store(mods, Ordering::Relaxed);

    let m = Mods { shift: mods & 3 != 0, ctrl: mods & 4 != 0, alt: mods & 8 != 0 };
    EVENTS.push(Event::Key { key, pressed, mods: m });
}
