//! Mouse driver (also drives the T470's TrackPoint and touchpad, which act
//! like a standard PS/2 mouse until a special driver unlocks more features).
//!
//! The mouse sends 3-byte packets (4 with a scroll wheel):
//!   byte 0: buttons + sign bits + "always 1" bit 3 (used to stay in sync)
//!   byte 1: X movement,  byte 2: Y movement (+ = up),  byte 3: wheel

use super::{Event, EVENTS};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

static WHEEL: AtomicBool = AtomicBool::new(false);
static IDX: AtomicUsize = AtomicUsize::new(0);
static PKT: [AtomicU8; 4] = [AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0), AtomicU8::new(0)];

pub fn set_wheel(on: bool) { WHEEL.store(on, Ordering::Relaxed); }

pub fn byte(b: u8) {
    let i = IDX.load(Ordering::Relaxed);
    if i == 0 && b & 0x08 == 0 { return; } // out of sync: wait for a valid first byte
    PKT[i].store(b, Ordering::Relaxed);
    let len = if WHEEL.load(Ordering::Relaxed) { 4 } else { 3 };
    if i + 1 < len { IDX.store(i + 1, Ordering::Relaxed); return; }
    IDX.store(0, Ordering::Relaxed);

    let f = PKT[0].load(Ordering::Relaxed);
    if f & 0xC0 != 0 { return; } // overflow: drop the packet
    let mut dx = PKT[1].load(Ordering::Relaxed) as i32;
    let mut dy = PKT[2].load(Ordering::Relaxed) as i32;
    if f & 0x10 != 0 { dx -= 256; } // 9-bit signed values
    if f & 0x20 != 0 { dy -= 256; }
    let wheel = if len == 4 { (PKT[3].load(Ordering::Relaxed) as i8 as i32 & 0x0F) << 28 >> 28 } else { 0 };
    EVENTS.push(Event::Mouse {
        dx, dy: -dy, wheel,
        left: f & 1 != 0, right: f & 2 != 0, middle: f & 4 != 0,
    });
}
