//! The kernel heap: lets us use `Vec`, `Box` and `String`.
//!
//! We reserve 1 GiB of *virtual* addresses at HEAP_BASE but back only the
//! first few MiB with real RAM. When the allocator runs out, we map more
//! frames at the end and tell it the heap grew. So the heap costs only the
//! RAM it really uses.
//!
//! Performance rule: drawing code never allocates. The heap is for things
//! created once (buffers, documents), not for every frame.

use super::{frame, paging};
use core::alloc::{GlobalAlloc, Layout};
use linked_list_allocator::Heap;
use spin::Mutex;

const INITIAL: u64 = 2 << 20; // 2 MiB to start
const GROW_MIN: u64 = 1 << 20;

struct KernelHeap { heap: Mutex<Heap>, size: core::sync::atomic::AtomicU64 }

#[global_allocator]
static HEAP: KernelHeap = KernelHeap { heap: Mutex::new(Heap::empty()), size: core::sync::atomic::AtomicU64::new(0) };

fn map_range(start: u64, bytes: u64) -> bool {
    let mut off = 0;
    while off < bytes {
        let Some(f) = frame::alloc() else { return false };
        if paging::map_page(start + off, f, paging::WRITABLE | paging::NO_EXECUTE).is_err() { return false; }
        off += 4096;
    }
    true
}

pub fn init() {
    assert!(map_range(paging::HEAP_BASE, INITIAL), "could not map heap");
    unsafe { HEAP.heap.lock().init(paging::HEAP_BASE as *mut u8, INITIAL as usize) };
    HEAP.size.store(INITIAL, core::sync::atomic::Ordering::Relaxed);
}

/// Bytes of heap mapped / bytes in use.
pub fn stats() -> (usize, usize) {
    let h = HEAP.heap.lock();
    (h.size(), h.used())
}

unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        crate::arch::cpu::without_interrupts(|| {
            let mut h = self.heap.lock();
            if let Ok(p) = h.allocate_first_fit(layout) { return p.as_ptr(); }
            // Out of space: grow the heap and try again.
            let size = h.size() as u64;
            let grow = (layout.size() as u64 + layout.align() as u64 + 4096).max(GROW_MIN).next_multiple_of(4096);
            if size + grow > paging::HEAP_MAX || !map_range(paging::HEAP_BASE + size, grow) {
                return core::ptr::null_mut();
            }
            unsafe { h.extend(grow as usize) };
            h.allocate_first_fit(layout).map(|p| p.as_ptr()).unwrap_or(core::ptr::null_mut())
        })
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        crate::arch::cpu::without_interrupts(|| unsafe {
            self.heap.lock().deallocate(core::ptr::NonNull::new_unchecked(ptr), layout)
        })
    }
}
