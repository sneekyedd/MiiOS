//! Physical frame allocator.
//!
//! RAM is handed out in 4 KiB pieces called *frames*. We keep:
//!  * a list of free regions from the memory map, handed out front to back
//!    ("bump" allocation: just move a pointer forward), and
//!  * a *free list* of frames that were given back. Each free frame stores
//!    the address of the next free frame inside itself, so the list costs
//!    zero extra memory.
//! Both allocate and free are O(1): a few instructions.

use super::early::{phys_to_virt, PAGE};
use crate::boot;
use spin::Mutex;

const MAX_REGIONS: usize = 64;

struct Frames {
    regions: [(u64, u64); MAX_REGIONS], // (next free address, end)
    n: usize,
    cur: usize,
    free_list: u64, // physical address of first freed frame, 0 = none
    pub total: u64,
    pub used: u64,
}

static FRAMES: Mutex<Frames> = Mutex::new(Frames { regions: [(0, 0); MAX_REGIONS], n: 0, cur: 0, free_list: 0, total: 0, used: 0 });

/// Build the region list from Limine's memory map, skipping what the early
/// allocator already took. Call once, after all early allocations.
pub fn init() {
    let mm = boot::MEMMAP.get().expect("no memory map");
    let mut f = FRAMES.lock();
    for m in mm.entries().filter(|m| m.kind == boot::MEM_USABLE) {
        let mut start = (m.base + PAGE - 1) & !(PAGE - 1);
        let mut end = (m.base + m.length) & !(PAGE - 1);
        if start < 0x10_0000 { start = 0x10_0000; } // leave the first 1 MiB alone (old BIOS stuff)
        // Early allocations were carved from the *end* of a region.
        super::early::taken(|b, _l| { if b >= start && b < end { end = b; } });
        if end > start && f.n < MAX_REGIONS {
            let n = f.n;
            f.regions[n] = (start, end);
            f.n += 1;
            f.total += end - start;
        }
    }
}

/// Get one zeroed 4 KiB frame. Returns its physical address.
pub fn alloc() -> Option<u64> {
    let mut f = FRAMES.lock();
    let p = if f.free_list != 0 {
        let p = f.free_list;
        f.free_list = unsafe { *phys_to_virt::<u64>(p) };
        p
    } else {
        loop {
            if f.cur >= f.n { return None; }
            let c = f.cur;
            let (next, end) = f.regions[c];
            if next < end { f.regions[c].0 += PAGE; break next; }
            f.cur += 1;
        }
    };
    f.used += PAGE;
    drop(f);
    unsafe { core::ptr::write_bytes(phys_to_virt::<u8>(p), 0, PAGE as usize) };
    Some(p)
}

/// Give a frame back.
pub fn free(p: u64) {
    let mut f = FRAMES.lock();
    unsafe { *phys_to_virt::<u64>(p) = f.free_list };
    f.free_list = p;
    f.used -= PAGE;
}

/// (total usable bytes, bytes in use)
pub fn stats() -> (u64, u64) { let f = FRAMES.lock(); (f.total, f.used) }
