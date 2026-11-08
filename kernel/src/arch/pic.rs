//! The 8259 PIC ("Programmable Interrupt Controller") and the 8254 PIT
//! ("Programmable Interval Timer").
//!
//! The PIC collects hardware interrupt lines (IRQ 0 = timer, 1 = keyboard,
//! 12 = mouse...) and forwards them to the CPU. By default it uses interrupt
//! numbers 8-15, which clash with CPU exceptions, so we "remap" IRQs to 32-47.
//! After handling an IRQ we must send "End Of Interrupt" (EOI) or the PIC
//! won't send more.

use super::cpu::{inb, io_wait, outb};

const PIC1: u16 = 0x20;
const PIC2: u16 = 0xA0;
pub const IRQ_BASE: u8 = 32;

pub fn init() {
    // ICW1: start initialisation, expect ICW4
    outb(PIC1, 0x11); io_wait();
    outb(PIC2, 0x11); io_wait();
    // ICW2: new interrupt numbers
    outb(PIC1 + 1, IRQ_BASE); io_wait();
    outb(PIC2 + 1, IRQ_BASE + 8); io_wait();
    // ICW3: the second PIC is chained on line 2 of the first
    outb(PIC1 + 1, 4); io_wait();
    outb(PIC2 + 1, 2); io_wait();
    // ICW4: 8086 mode
    outb(PIC1 + 1, 1); io_wait();
    outb(PIC2 + 1, 1); io_wait();
    // Masks: a 1 bit blocks that IRQ. Allow keyboard (1), chain (2), mouse (12).
    // (The timer normally comes from the local APIC instead; see apic.rs.)
    outb(PIC1 + 1, !(1u8 << 1 | 1 << 2));
    outb(PIC2 + 1, !(1u8 << (12 - 8)));
}

pub fn eoi(irq: u8) {
    if irq >= 8 { outb(PIC2, 0x20); }
    outb(PIC1, 0x20);
}

/// IRQ 7 and 15 can be "spurious" (electrical noise). Check the PIC's
/// in-service register before treating them as real.
pub fn is_spurious(irq: u8) -> bool {
    match irq {
        7 => { outb(PIC1, 0x0B); inb(PIC1) & 0x80 == 0 }
        15 => { outb(PIC2, 0x0B); if inb(PIC2) & 0x80 == 0 { outb(PIC1, 0x20); true } else { false } }
        _ => false,
    }
}

/// Make the PIT fire IRQ 0 `hz` times per second.
pub fn pit_init(hz: u32) {
    let divisor = (crate::time::PIT_HZ as u32 / hz) as u16;
    outb(0x43, 0b0011_0100); // channel 0, lo+hi byte, mode 2 (rate generator)
    outb(0x40, divisor as u8);
    outb(0x40, (divisor >> 8) as u8);
}

/// Allow one IRQ line on the first PIC.
pub fn unmask(irq: u8) {
    let m = inb(PIC1 + 1);
    outb(PIC1 + 1, m & !(1 << irq));
}
