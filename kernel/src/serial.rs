//! Serial port (COM1) output.
//!
//! A serial port is the simplest possible "screen": we send it bytes and QEMU
//! prints them in your terminal. It works even if graphics are broken, so it
//! is our main debugging tool. (The T470 has no physical serial port, so on the
//! real laptop these messages simply go nowhere -- that's harmless.)

use crate::arch::cpu::{inb, outb};
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

const COM1: u16 = 0x3F8;
static PRESENT: AtomicBool = AtomicBool::new(false);

pub fn init() {
    outb(COM1 + 1, 0x00); // no serial interrupts
    outb(COM1 + 3, 0x80); // enable baud rate divisor
    outb(COM1, 0x01);     // divisor 1 = 115200 baud (fastest)
    outb(COM1 + 1, 0x00);
    outb(COM1 + 3, 0x03); // 8 bits, no parity, one stop bit
    outb(COM1 + 2, 0xC7); // enable FIFO
    outb(COM1 + 4, 0x0B);
    // A missing port reads back 0xFF. Don't wait on hardware that isn't there.
    PRESENT.store(inb(COM1 + 5) != 0xFF, Ordering::Relaxed);
}

fn write_byte(b: u8) {
    if !PRESENT.load(Ordering::Relaxed) { return; }
    let mut spins = 0;
    while inb(COM1 + 5) & 0x20 == 0 && spins < 100_000 { spins += 1; }
    outb(COM1, b);
}

pub struct Serial;
impl fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' { write_byte(b'\r'); }
            write_byte(b);
        }
        Ok(())
    }
}

pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    crate::arch::cpu::without_interrupts(|| { let _ = Serial.write_fmt(args); });
}

/// `log!("x = {}", x)` prints a line to the serial port.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::serial::_print(format_args!("{}\n", format_args!($($arg)*))) };
}
