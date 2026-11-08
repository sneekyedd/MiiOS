//! Time keeping.
//!
//! The CPU's Time Stamp Counter (TSC) counts up at a constant rate, but we
//! don't know the rate in advance. So at boot we measure it once against the
//! old PC timer chip (the "PIT", which always runs at 1,193,182 Hz): we let the
//! PIT count down 10 ms and see how far the TSC moved meanwhile.

use crate::arch::cpu::{inb, outb, rdtsc};
use core::sync::atomic::{AtomicU64, Ordering};

static TSC_HZ: AtomicU64 = AtomicU64::new(2_500_000_000);
static TSC_AT_KMAIN: AtomicU64 = AtomicU64::new(0);

pub const PIT_HZ: u64 = 1_193_182;

pub fn mark_kernel_entry(tsc: u64) { TSC_AT_KMAIN.store(tsc, Ordering::Relaxed); }

/// Measure the TSC frequency. Takes ~10 ms.
pub fn calibrate_tsc() -> u64 {
    const MS: u64 = 10;
    let count = (PIT_HZ * MS / 1000) as u16;
    // Port 0x61 bit 0 = "gate" that lets PIT channel 2 count; bit 1 = speaker (keep off).
    let p = inb(0x61);
    outb(0x61, (p & !0x02) & !0x01);
    outb(0x43, 0b1011_0000); // channel 2, lo+hi byte, mode 0 ("count down once"), binary
    outb(0x42, count as u8);
    outb(0x42, (count >> 8) as u8);
    let t0 = rdtsc();
    outb(0x61, (p & !0x02) | 0x01); // open the gate: counting starts
    // Bit 5 of port 0x61 turns on when the countdown reaches zero.
    let mut ok = false;
    for _ in 0..50_000_000u64 {
        if inb(0x61) & 0x20 != 0 { ok = true; break; }
    }
    let t1 = rdtsc();
    outb(0x61, p);
    let hz = if ok { (t1 - t0) * 1000 / MS } else { cpuid_tsc_hz().unwrap_or(2_500_000_000) };
    TSC_HZ.store(hz, Ordering::Relaxed);
    hz
}

/// Fallback: newer Intel CPUs report their base clock in CPUID leaf 0x16.
fn cpuid_tsc_hz() -> Option<u64> {
    let max = core::arch::x86_64::__cpuid(0).eax;
    if max < 0x16 { return None; }
    let mhz = core::arch::x86_64::__cpuid(0x16).eax as u64;
    if mhz == 0 { None } else { Some(mhz * 1_000_000) }
}

pub fn tsc_hz() -> u64 { TSC_HZ.load(Ordering::Relaxed) }

/// Microseconds since the CPU was switched on (the TSC starts at 0 at power-on).
pub fn uptime_us() -> u64 {
    let t = rdtsc() as u128;
    (t * 1_000_000 / tsc_hz() as u128) as u64
}

pub fn uptime_ms() -> u64 { uptime_us() / 1000 }

/// Microseconds from power-on until the kernel's first instruction.
pub fn us_before_kernel() -> u64 {
    (TSC_AT_KMAIN.load(Ordering::Relaxed) as u128 * 1_000_000 / tsc_hz() as u128) as u64
}

/// Busy-wait for a number of microseconds.
pub fn spin_us(us: u64) {
    let end = rdtsc() + us * tsc_hz() / 1_000_000;
    while rdtsc() < end { core::hint::spin_loop(); }
}
