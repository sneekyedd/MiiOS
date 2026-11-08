//! Local APIC: each CPU core's modern interrupt controller.
//!
//! We still use the old PIC to route keyboard/mouse/timer interrupts (simple,
//! and the T470 supports it). On UEFI machines the PIC's output reaches the
//! CPU through the local APIC's "LINT0" input, which must be set to
//! "ExtINT" (pass PIC interrupts through). Firmware normally does this, but
//! we set it ourselves so we don't depend on it.

use super::cpu;
use crate::log;

pub fn init_virtual_wire() {
    let base_msr = cpu::rdmsr(0x1B);
    let phys = base_msr & 0xF_FFFF_F000;
    let Ok(regs) = crate::mem::paging::map_mmio(phys, 4096) else { log!("apic: map failed"); return };
    let reg = |off: usize| unsafe { regs.add(off) as *mut u32 };
    unsafe {
        cpu::wrmsr(0x1B, base_msr | (1 << 11));   // APIC globally enabled
        reg(0x80).write_volatile(0);              // task priority: accept everything
        reg(0xF0).write_volatile(0x1FF);          // software enable + spurious vector 255
        reg(0x350).write_volatile(0x700);         // LINT0 = ExtINT (PIC interrupts)
        reg(0x360).write_volatile(0x400);         // LINT1 = NMI
    }
    log!("apic: local APIC at {:#x}, PIC routed through LINT0", phys);
}
