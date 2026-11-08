//! Paging: translating *virtual* addresses (what code uses) into *physical*
//! addresses (where the bytes really are in RAM).
//!
//! x86-64 uses a tree of 4 levels of tables, each a 4 KiB page holding 512
//! entries. A virtual address is split into 4 indexes (9 bits each) plus a
//! 12-bit offset inside the final 4 KiB page:
//!
//!   | PML4 idx | PDPT idx | PD idx | PT idx | offset |
//!     47..39     38..30     29..21   20..12   11..0
//!
//! Each entry holds a physical address plus flag bits (present, writable,
//! caching mode...). Limine already set up a tree for us; we add our own
//! mappings to it: the heap, the framebuffer (with faster caching), and
//! device registers ("MMIO").

use super::early::phys_to_virt;
use super::frame;
use crate::arch::cpu;
use core::sync::atomic::{AtomicU64, Ordering};

pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
pub const WRITE_THROUGH: u64 = 1 << 3; // "PWT"
pub const NO_CACHE: u64 = 1 << 4;      // "PCD"
pub const HUGE: u64 = 1 << 7;          // in upper levels: this entry maps a big page
pub const PAT_4K: u64 = 1 << 7;        // in a 4K page entry: 3rd bit of the cache-type index
pub const GLOBAL: u64 = 1 << 8;
pub const NO_EXECUTE: u64 = 1 << 63;
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Cache type "write-combining": writes are collected and sent in bursts.
/// Ideal for the framebuffer (we only ever write to it, in long runs).
/// PAT index 5 = PAT bit + PWT bit (see `setup_pat`).
pub const CACHE_WC: u64 = PAT_4K | WRITE_THROUGH;
/// Uncached: every access goes straight to the device. Required for device registers.
pub const CACHE_UC: u64 = NO_CACHE | WRITE_THROUGH;

/// The PAT ("page attribute table") MSR defines what each cache-type index
/// means. We keep the standard first four and make index 5 write-combining
/// (the same layout Limine uses).
pub fn setup_pat() {
    // index:   7=UC    6=UC-   5=WC    4=WP    3=UC    2=UC-   1=WT    0=WB
    let pat: u64 = 0x00_07_01_05_00_07_04_06;
    cpu::wrmsr(0x277, pat);
    cpu::write_cr3(cpu::read_cr3()); // flush cached translations
}

fn table(phys: u64) -> &'static mut [u64; 512] { unsafe { &mut *phys_to_virt::<[u64; 512]>(phys) } }

/// Map one 4 KiB page: virtual `virt` -> physical `phys`, with `flags`.
pub fn map_page(virt: u64, phys: u64, flags: u64) -> Result<(), &'static str> {
    let mut t = cpu::read_cr3() & ADDR_MASK;
    for level in [39u64, 30, 21] {
        let idx = ((virt >> level) & 511) as usize;
        let e = &mut table(t)[idx];
        if *e & PRESENT == 0 {
            let new = frame::alloc().ok_or("out of memory for page tables")?;
            *e = new | PRESENT | WRITABLE;
        } else if *e & HUGE != 0 {
            return Err("address is inside a huge page");
        }
        t = *e & ADDR_MASK;
    }
    let idx = ((virt >> 12) & 511) as usize;
    table(t)[idx] = (phys & ADDR_MASK) | flags | PRESENT;
    cpu::invlpg(virt);
    Ok(())
}

/// Walk the tables to find the physical address behind `virt`.
pub fn translate(virt: u64) -> Option<u64> {
    let mut t = cpu::read_cr3() & ADDR_MASK;
    for (level, size) in [(39u64, 0u64), (30, 1 << 30), (21, 1 << 21)] {
        let e = table(t)[((virt >> level) & 511) as usize];
        if e & PRESENT == 0 { return None; }
        if e & HUGE != 0 && size != 0 { return Some((e & ADDR_MASK & !(size - 1)) + (virt & (size - 1))); }
        t = e & ADDR_MASK;
    }
    let e = table(t)[((virt >> 12) & 511) as usize];
    if e & PRESENT == 0 { None } else { Some((e & ADDR_MASK) + (virt & 0xFFF)) }
}

// Our own regions of virtual address space (well away from Limine's).
pub const HEAP_BASE: u64 = 0xFFFF_9000_0000_0000;
pub const HEAP_MAX: u64 = 1 << 30; // up to 1 GiB of heap
const FB_BASE: u64 = 0xFFFF_A000_0000_0000;
const MMIO_BASE: u64 = 0xFFFF_B000_0000_0000;
static MMIO_NEXT: AtomicU64 = AtomicU64::new(MMIO_BASE);

/// Map the framebuffer again, this time write-combining. Returns the new address.
pub fn map_framebuffer(phys: u64, bytes: u64) -> Result<*mut u32, &'static str> {
    let pages = (bytes + 4095) / 4096;
    for i in 0..pages {
        map_page(FB_BASE + i * 4096, phys + i * 4096, WRITABLE | NO_EXECUTE | GLOBAL | CACHE_WC)?;
    }
    Ok(FB_BASE as *mut u32)
}

/// Map device registers (uncached). Returns a pointer to them.
pub fn map_mmio(phys: u64, bytes: u64) -> Result<*mut u8, &'static str> {
    let off = phys & 0xFFF;
    let start = phys & !0xFFF;
    let pages = (off + bytes + 4095) / 4096;
    let virt = MMIO_NEXT.fetch_add(pages * 4096 + 4096, Ordering::Relaxed); // +1 guard page
    for i in 0..pages {
        map_page(virt + i * 4096, start + i * 4096, WRITABLE | NO_EXECUTE | CACHE_UC)?;
    }
    Ok((virt + off) as *mut u8)
}
