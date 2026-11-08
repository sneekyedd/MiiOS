//! Local APIC: each CPU core's modern interrupt controller.
//!
//! We still use the old PIC to route keyboard/mouse/timer interrupts (simple,
//! and the T470 supports it). On UEFI machines the PIC's output reaches the
//! CPU through the local APIC's "LINT0" input, which must be set to
//! "ExtINT" (pass PIC interrupts through). Firmware normally does this, but
//! we set it ourselves so we don't depend on it.

use super::cpu;
use crate::log;
use core::sync::atomic::{AtomicU64, Ordering};

static BASE: AtomicU64 = AtomicU64::new(0);
pub const TIMER_VECTOR: u8 = 48;

fn reg(off: usize) -> *mut u32 { (BASE.load(Ordering::Relaxed) as usize + off) as *mut u32 }

/// Tell the local APIC we finished handling its interrupt.
pub fn eoi() { if BASE.load(Ordering::Relaxed) != 0 { unsafe { reg(0xB0).write_volatile(0) } } }

/// Start the local APIC timer firing `hz` times per second.
///
/// Why not the old PIT chip? On Skylake laptops like the T470 the firmware
/// may switch the PIT's clock off to save power ("8254 clock gating"), and
/// then a PIT-based tick would simply never arrive. The local APIC timer is
/// part of the CPU itself, so it's always there. Its speed isn't documented,
/// so we measure it against the TSC: let it count down for 10 ms and see
/// how far it got.
pub fn start_timer(hz: u32) -> bool {
    if BASE.load(Ordering::Relaxed) == 0 { return false; }
    unsafe {
        reg(0x3E0).write_volatile(0b0011);           // divide the bus clock by 16
        reg(0x320).write_volatile(1 << 16);          // timer masked while we measure
        reg(0x380).write_volatile(u32::MAX);         // start counting down from the top
        crate::time::spin_us(10_000);
        let elapsed = u32::MAX - reg(0x390).read_volatile();
        let per_tick = (elapsed as u64 * 100 / hz as u64) as u32; // counts per 10 ms -> per tick
        if per_tick == 0 { return false; }
        reg(0x320).write_volatile(TIMER_VECTOR as u32 | (1 << 17)); // periodic mode
        reg(0x380).write_volatile(per_tick);
        log!("apic: timer {} counts per tick ({} Hz)", per_tick, hz);
    }
    true
}

pub fn init_virtual_wire() {
    let base_msr = cpu::rdmsr(0x1B);
    let phys = base_msr & 0xF_FFFF_F000;
    let Ok(regs) = crate::mem::paging::map_mmio(phys, 4096) else { log!("apic: map failed"); return };
    BASE.store(regs as u64, Ordering::Relaxed);
    unsafe {
        cpu::wrmsr(0x1B, base_msr | (1 << 11));   // APIC globally enabled
        reg(0x80).write_volatile(0);              // task priority: accept everything
        reg(0xF0).write_volatile(0x1FF);          // software enable + spurious vector 255
        reg(0x350).write_volatile(0x700);         // LINT0 = ExtINT (PIC interrupts)
        reg(0x360).write_volatile(0x400);         // LINT1 = NMI
    }
    log!("apic: local APIC at {:#x}, PIC routed through LINT0", phys);
}
