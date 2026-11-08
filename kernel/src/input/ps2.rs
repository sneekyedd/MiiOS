//! The PS/2 controller (an "8042" chip, today built into the laptop's
//! embedded controller). The ThinkPad T470's built-in keyboard, TrackPoint
//! and touchpad all talk to us through it.
//!
//! Two I/O ports: 0x60 = data, 0x64 = status (read) / command (write).
//! Status bit 0: a byte is waiting for us. Bit 1: controller busy, don't
//! write yet. Bit 5: the waiting byte came from the mouse (2nd port).

use crate::arch::cpu::{inb, outb};
use crate::log;
use core::sync::atomic::{AtomicBool, Ordering};

/// False if this PC has no PS/2 controller at all (its ports then read 0xFF).
static PRESENT: AtomicBool = AtomicBool::new(false);

/// Throw away any waiting bytes (bounded, in case the hardware misbehaves).
fn flush() { for _ in 0..64 { if inb(STATUS) & 1 == 0 { break; } inb(DATA); } }

const DATA: u16 = 0x60;
const STATUS: u16 = 0x64;
const CMD: u16 = 0x64;

fn wait_write() -> bool {
    for _ in 0..100_000 { if inb(STATUS) & 2 == 0 { return true; } }
    false
}
fn wait_read() -> bool {
    for _ in 0..100_000 { if inb(STATUS) & 1 != 0 { return true; } }
    false
}
fn command(c: u8) { wait_write(); outb(CMD, c); }
fn write_data(b: u8) { wait_write(); outb(DATA, b); }
fn read_data() -> Option<u8> { if wait_read() { Some(inb(DATA)) } else { None } }

/// Send a byte to the mouse and wait for its "ACK" (0xFA).
pub fn mouse_write(b: u8) -> bool {
    command(0xD4); // "next data byte goes to the mouse"
    write_data(b);
    for _ in 0..4 {
        match read_data() { Some(0xFA) => return true, Some(_) => continue, None => return false }
    }
    false
}

pub fn mouse_read() -> Option<u8> { read_data() }

/// Send a byte to the keyboard and wait for ACK.
pub fn kbd_write(b: u8) -> bool {
    write_data(b);
    matches!(read_data(), Some(0xFA))
}

/// Set up the controller. Returns true if a mouse answered.
pub fn init(enable_irqs: bool) -> bool {
    if inb(STATUS) == 0xFF {
        log!("ps2: no PS/2 controller (USB keyboards still work)");
        return false;
    }
    PRESENT.store(true, Ordering::Relaxed);
    command(0xAD); // disable keyboard port while we configure
    command(0xA7); // disable mouse port
    flush(); // throw away stale bytes

    command(0x20); // read configuration byte
    let mut cfg = read_data().unwrap_or(0);
    cfg &= !0b0000_0011;   // interrupts off during setup
    cfg |= 0b0100_0000;    // "translation": keyboard speaks the simple scancode set 1
    command(0x60); write_data(cfg);

    command(0xAE); // enable keyboard port
    command(0xA8); // enable mouse port

    // Mouse: defaults, then try to unlock the scroll wheel ("IntelliMouse"
    // trick: set sample rate 200, 100, 80, then ask for the device ID).
    let mut mouse_ok = mouse_write(0xF6);
    let mut wheel = false;
    if mouse_ok {
        for r in [200u8, 100, 80] { mouse_write(0xF3); mouse_write(r); }
        if mouse_write(0xF2) { wheel = matches!(read_data(), Some(3)); }
        mouse_write(0xF3); mouse_write(100); // 100 reports per second: smooth but cheap
        mouse_ok = mouse_write(0xF4);        // start sending movement
    }
    super::mouse::set_wheel(wheel);
    log!("ps2: mouse {} (wheel: {})", if mouse_ok { "found" } else { "missing" }, wheel);

    if enable_irqs {
        command(0x20);
        let cfg = read_data().unwrap_or(cfg) | 0b11; // IRQ1 (keyboard) and IRQ12 (mouse)
        command(0x60); write_data(cfg);
    }
    flush();
    mouse_ok
}

/// Polling: read every waiting byte and hand it to the right driver.
pub fn poll() {
    if !PRESENT.load(Ordering::Relaxed) { return; }
    for _ in 0..64 {
        let s = inb(STATUS);
        if s & 1 == 0 { break; }
        let b = inb(DATA);
        if s & 0x20 != 0 { super::mouse::byte(b) } else { super::keyboard::byte(b) }
    }
}
