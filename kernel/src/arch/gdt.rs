//! GDT (Global Descriptor Table) and TSS (Task State Segment).
//!
//! In 64-bit mode segments are mostly a leftover from the 1980s, but the CPU
//! still needs: a code segment, a data segment, and a TSS. The TSS matters to
//! us for one reason: it lists *emergency stacks* ("IST"). If the kernel
//! stack overflows, the CPU can't push the error info onto the broken stack,
//! so for "double fault" we tell it to switch to a separate clean stack.
//! Without this, a stack overflow would silently reboot the machine.

use core::arch::asm;
use core::mem::size_of;

#[repr(C, packed)]
struct Tss {
    _r0: u32,
    rsp: [u64; 3],
    _r1: u64,
    ist: [u64; 7],
    _r2: u64,
    _r3: u16,
    iomap: u16,
}

static mut TSS: Tss = Tss { _r0: 0, rsp: [0; 3], _r1: 0, ist: [0; 7], _r2: 0, _r3: 0, iomap: size_of::<Tss>() as u16 };

#[repr(C, align(16))]
struct Stack([u8; 16 * 1024]);
static mut DOUBLE_FAULT_STACK: Stack = Stack([0; 16 * 1024]);

// null, kernel code, kernel data, TSS (takes two slots)
static mut GDT: [u64; 5] = [
    0,
    0x00AF_9A00_0000_FFFF, // 64-bit code, ring 0
    0x00CF_9200_0000_FFFF, // data, ring 0
    0, 0,                  // TSS, filled in at runtime (needs its address)
];

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
const TSS_SEL: u16 = 0x18;
pub const DOUBLE_FAULT_IST: u8 = 1;

#[repr(C, packed)]
struct Pointer { limit: u16, base: u64 }

pub fn init() {
    unsafe {
        let stack_top = (&raw const DOUBLE_FAULT_STACK) as u64 + size_of::<Stack>() as u64;
        (*(&raw mut TSS)).ist[0] = stack_top;

        let base = (&raw const TSS) as u64;
        let limit = (size_of::<Tss>() - 1) as u64;
        // A TSS descriptor is 16 bytes with the address scattered across fields.
        let low = (limit & 0xFFFF)
            | ((base & 0xFF_FFFF) << 16)
            | (0x89 << 40)               // present, type = available 64-bit TSS
            | (((limit >> 16) & 0xF) << 48)
            | (((base >> 24) & 0xFF) << 56);
        let high = base >> 32;
        let gdt = &raw mut GDT;
        (*gdt)[3] = low;
        (*gdt)[4] = high;

        let ptr = Pointer { limit: (size_of::<[u64; 5]>() - 1) as u16, base: gdt as u64 };
        asm!("lgdt [{}]", in(reg) &ptr, options(readonly, nostack, preserves_flags));
        // Reload CS with a "far return": push the new CS and a return address, then retfq.
        asm!(
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            cs = in(reg) KERNEL_CS as u64,
            tmp = lateout(reg) _,
        );
        asm!(
            "mov ds, {0:x}", "mov es, {0:x}", "mov ss, {0:x}", "mov fs, {0:x}", "mov gs, {0:x}",
            in(reg) KERNEL_DS as u64, options(nostack, preserves_flags)
        );
        asm!("ltr {0:x}", in(reg) TSS_SEL, options(nostack, preserves_flags));
    }
}
