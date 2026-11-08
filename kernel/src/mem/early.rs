//! "Early" memory: a very simple allocator used before the real one exists.
//!
//! Limine gives us a *memory map*: a list of physical memory regions and what
//! each is for. "Usable" regions are free RAM. Early on, we just carve big
//! chunks (like the 8 MB screen back buffer) off the end of the largest usable
//! region, and remember what we took so the real allocator won't hand it out.

use crate::boot;
use spin::Mutex;

pub const PAGE: u64 = 4096;

pub struct Region { pub base: u64, pub len: u64 }

struct Early { taken: [(u64, u64); 16], n: usize }
static EARLY: Mutex<Early> = Mutex::new(Early { taken: [(0, 0); 16], n: 0 });

/// Take `bytes` of physical RAM (page aligned). Returns the physical address.
pub fn alloc_phys(bytes: u64) -> Option<u64> {
    let bytes = (bytes + PAGE - 1) & !(PAGE - 1);
    let mm = boot::MEMMAP.get()?;
    let mut e = EARLY.lock();
    // Pick the largest usable region, minus what we already took from it.
    let mut best: Option<(u64, u64)> = None;
    for m in mm.entries().filter(|m| m.kind == boot::MEM_USABLE) {
        let mut end = m.base + m.length;
        for &(b, l) in &e.taken[..e.n] {
            if b >= m.base && b + l <= m.base + m.length { end = end.min(b); }
        }
        let free = end - m.base;
        if free >= bytes && best.map_or(true, |(_, f)| free > f) { best = Some((end, free)); }
    }
    let (end, _) = best?;
    let base = end - bytes;
    let n = e.n;
    e.taken[n] = (base, bytes);
    e.n += 1;
    Some(base)
}

/// Everything taken so far (the real allocator skips these).
pub fn taken(mut f: impl FnMut(u64, u64)) {
    let e = EARLY.lock();
    for &(b, l) in &e.taken[..e.n] { f(b, l); }
}

/// Convert a physical address to a pointer we can use (via Limine's direct map).
pub fn phys_to_virt<T>(p: u64) -> *mut T { (p + boot::hhdm_offset()) as *mut T }
