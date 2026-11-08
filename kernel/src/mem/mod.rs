//! Memory management: physical frames, page tables, and the heap.
pub mod early;
pub mod frame;
pub mod heap;
pub mod paging;

/// Set up everything, in order. Call after all `early` allocations.
pub fn init() {
    paging::setup_pat();
    frame::init();
    heap::init();
}
