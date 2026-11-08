//! IDT (Interrupt Descriptor Table): for each of the 256 interrupt numbers,
//! where the CPU should jump.
//!
//! Numbers 0-31 are CPU *exceptions* (divide by zero, page fault...).
//! 32-47 are hardware IRQs from the PIC (see pic.rs).
//!
//! When an interrupt fires, the CPU pushes the return address, flags and
//! stack pointer and jumps to our *stub* (written in assembly below). The
//! stub saves all registers, calls `interrupt_dispatch` in Rust with a
//! pointer to them, restores them, and returns with `iretq`.
//! (The nicer `extern "x86-interrupt"` needs nightly Rust; this works on stable.)

use super::{cpu, gdt, pic};
use core::arch::{asm, global_asm};

/// Registers as saved by the stub, in memory order.
#[repr(C)]
#[derive(Debug)]
pub struct Frame {
    pub r15: u64, pub r14: u64, pub r13: u64, pub r12: u64, pub r11: u64, pub r10: u64, pub r9: u64, pub r8: u64,
    pub rbp: u64, pub rdi: u64, pub rsi: u64, pub rdx: u64, pub rcx: u64, pub rbx: u64, pub rax: u64,
    pub vector: u64, pub error: u64,
    pub rip: u64, pub cs: u64, pub rflags: u64, pub rsp: u64, pub ss: u64,
}

// Stubs for vectors 0..=47. Exceptions 8,10-14,17,21,29,30 push an error code
// themselves; for the rest we push a 0 so every frame looks the same.
global_asm!(r#"
.section .text
.irp n, 0,1,2,3,4,5,6,7,9,15,16,18,19,20,22,23,24,25,26,27,28,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
isr_stub_\n:
    push 0
    push \n
    jmp isr_common
.endr
.irp n, 8,10,11,12,13,14,17,21,29,30
isr_stub_\n:
    push \n
    jmp isr_common
.endr
isr_stub_255:
    iretq

isr_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp
    cld
    call interrupt_dispatch
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

.pushsection .rodata
.balign 8
isr_table:
.irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,41,42,43,44,45,46,47
    .quad isr_stub_\n
.endr
isr_spurious:
    .quad isr_stub_255
.popsection
"#);

unsafe extern "C" {
    static isr_table: [u64; 48];
    static isr_spurious: u64;
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Entry { off_lo: u16, sel: u16, ist: u8, attr: u8, off_mid: u16, off_hi: u32, _z: u32 }
impl Entry {
    const EMPTY: Entry = Entry { off_lo: 0, sel: 0, ist: 0, attr: 0, off_mid: 0, off_hi: 0, _z: 0 };
    fn new(addr: u64, ist: u8) -> Entry {
        Entry { off_lo: addr as u16, sel: gdt::KERNEL_CS, ist, attr: 0x8E, // present, interrupt gate
                off_mid: (addr >> 16) as u16, off_hi: (addr >> 32) as u32, _z: 0 }
    }
}

static mut IDT: [Entry; 256] = [Entry::EMPTY; 256];

pub fn init() {
    unsafe {
        let idt = &raw mut IDT;
        for v in 0..48 {
            let ist = if v == 8 { gdt::DOUBLE_FAULT_IST } else { 0 };
            (*idt)[v] = Entry::new(isr_table[v], ist);
        }
        (*idt)[255] = Entry::new(isr_spurious, 0);
        #[repr(C, packed)]
        struct Ptr { limit: u16, base: u64 }
        let p = Ptr { limit: (core::mem::size_of::<[Entry; 256]>() - 1) as u16, base: idt as u64 };
        asm!("lidt [{}]", in(reg) &p, options(readonly, nostack, preserves_flags));
    }
}

const NAMES: [&str; 32] = [
    "divide by zero", "debug", "NMI", "breakpoint", "overflow", "bound range", "invalid opcode",
    "device not available", "double fault", "coprocessor", "invalid TSS", "segment not present",
    "stack fault", "general protection fault", "page fault", "reserved", "x87 FPU error",
    "alignment check", "machine check", "SIMD error", "virtualization", "control protection",
    "reserved", "reserved", "reserved", "reserved", "reserved", "reserved", "hypervisor injection",
    "VMM communication", "security", "reserved",
];

#[unsafe(no_mangle)]
extern "C" fn interrupt_dispatch(f: &mut Frame) {
    let v = f.vector as u8;
    if v < 32 {
        let extra = if v == 14 { cpu::read_cr2() } else { 0 };
        panic!("CPU exception {} ({}) at rip={:#x} error={:#x} cr2={:#x}", v, NAMES[v as usize], f.rip, f.error, extra);
    }
    let irq = v - pic::IRQ_BASE;
    if pic::is_spurious(irq) { return; }
    match irq {
        0 => crate::time::on_tick(),
        1 | 12 => crate::input::ps2::poll(),
        _ => {}
    }
    pic::eoi(irq);
}
