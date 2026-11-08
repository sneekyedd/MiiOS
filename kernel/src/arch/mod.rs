//! x86-64 specific code.
pub mod apic;
pub mod cpu;
pub mod gdt;
pub mod idt;
pub mod pic;
pub mod power;

/// Timer interrupts per second. 1000 = 1 ms resolution.
pub const TICK_HZ: u32 = 1000;

/// Set up segments, the interrupt table, the PIC and the timer, then enable interrupts.
pub fn init_interrupts() {
    gdt::init();
    idt::init();
    pic::init();
    apic::init_virtual_wire();
    if !apic::start_timer(TICK_HZ) {
        // Fallback: the old PIT chip on IRQ 0.
        pic::pit_init(TICK_HZ);
        pic::unmask(0);
    }
    cpu::sti();
}
