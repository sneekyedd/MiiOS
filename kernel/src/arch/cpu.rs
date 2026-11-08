//! Tiny wrappers around single CPU instructions.
//! `asm!` lets Rust emit an exact machine instruction when there is no
//! normal Rust way to say it (talking to hardware ports, halting, etc.).

use core::arch::asm;

/// Write a byte to an I/O port. Old PC devices (serial, keyboard, timer)
/// are controlled through these numbered "ports".
#[inline(always)]
pub fn outb(port: u16, val: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags)) }
}

#[inline(always)]
pub fn inb(port: u16) -> u8 {
    let v: u8;
    unsafe { asm!("in al, dx", out("al") v, in("dx") port, options(nomem, nostack, preserves_flags)) }
    v
}

#[inline(always)]
pub fn outl(port: u16, val: u32) {
    unsafe { asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags)) }
}

#[inline(always)]
pub fn inl(port: u16) -> u32 {
    let v: u32;
    unsafe { asm!("in eax, dx", out("eax") v, in("dx") port, options(nomem, nostack, preserves_flags)) }
    v
}

#[inline(always)]
pub fn outw(port: u16, val: u16) {
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags)) }
}

/// A ~1 microsecond pause (port 0x80 is a classic "do nothing" port).
#[inline(always)]
pub fn io_wait() { outb(0x80, 0) }

/// Sleep the CPU until the next interrupt. Saves power and heat.
#[inline(always)]
pub fn hlt() { unsafe { asm!("hlt", options(nomem, nostack, preserves_flags)) } }

/// Turn interrupts off / on.
#[inline(always)]
pub fn cli() { unsafe { asm!("cli", options(nomem, nostack)) } }
#[inline(always)]
pub fn sti() { unsafe { asm!("sti", options(nomem, nostack)) } }

/// Enable interrupts and halt in one go, so no interrupt can sneak in between.
#[inline(always)]
pub fn sti_hlt() { unsafe { asm!("sti; hlt", options(nomem, nostack)) } }

pub fn interrupts_enabled() -> bool {
    let f: u64;
    unsafe { asm!("pushfq; pop {}", out(reg) f, options(nomem, preserves_flags)) }
    f & (1 << 9) != 0
}

/// Run `f` with interrupts off, restoring the previous state afterwards.
#[inline]
pub fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
    let was = interrupts_enabled();
    if was { cli(); }
    let r = f();
    if was { sti(); }
    r
}

/// Read the Time Stamp Counter: a counter that ticks at a fixed rate
/// (about 2.5 GHz on the i5-7200U). Perfect for measuring short times.
#[inline(always)]
pub fn rdtsc() -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) }
    ((hi as u64) << 32) | lo as u64
}

/// Model Specific Registers: special CPU settings (PAT, APIC base, ...).
pub fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) }
    ((hi as u64) << 32) | lo as u64
}

pub fn wrmsr(msr: u32, v: u64) {
    unsafe { asm!("wrmsr", in("ecx") msr, in("eax") v as u32, in("edx") (v >> 32) as u32, options(nostack, preserves_flags)) }
}

pub fn read_cr3() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) }
    v
}

pub fn write_cr3(v: u64) {
    unsafe { asm!("mov cr3, {}", in(reg) v, options(nostack, preserves_flags)) }
}

pub fn read_cr2() -> u64 {
    let v: u64;
    unsafe { asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags)) }
    v
}

/// Forget the cached translation for one page (after changing its mapping).
#[inline(always)]
pub fn invlpg(addr: u64) {
    unsafe { asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags)) }
}

/// Make the CPU wait politely in spin loops.
#[inline(always)]
pub fn pause() { core::hint::spin_loop() }

/// Stop forever.
pub fn halt_forever() -> ! {
    loop { cli(); hlt(); }
}

/// Fast memory fill of 32-bit values using the CPU's "rep stosd" instruction.
#[inline(always)]
pub unsafe fn fill_u32(dst: *mut u32, val: u32, count: usize) {
    unsafe { asm!("rep stosd", inout("rdi") dst => _, inout("rcx") count => _, in("eax") val, options(nostack, preserves_flags)) }
}

/// Fast memory copy using "rep movsb" (very fast on Skylake: "ERMSB").
#[inline(always)]
pub unsafe fn copy_bytes(dst: *mut u8, src: *const u8, count: usize) {
    unsafe { asm!("rep movsb", inout("rdi") dst => _, inout("rsi") src => _, inout("rcx") count => _, options(nostack, preserves_flags)) }
}
