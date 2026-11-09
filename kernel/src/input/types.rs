//! Input events and the event queue. Shared by the bare-metal kernel and
//! the Linux edition (which feeds it from the window system instead of PS/2).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter, Backspace, Tab, Escape, Delete,
    Up, Down, Left, Right, Home, End, PageUp, PageDown,
    F(u8),
    Shift, Ctrl, Alt, Super, CapsLock,
    Unknown(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods { pub shift: bool, pub ctrl: bool, pub alt: bool }

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Key { key: Key, pressed: bool, mods: Mods },
    /// Relative movement (screen direction: +y is down) and button state.
    Mouse { dx: i32, dy: i32, wheel: i32, left: bool, right: bool, middle: bool },
    /// Absolute pointer position (from a window system that already tracks it).
    Pointer { x: i32, y: i32, wheel: i32, left: bool, right: bool, middle: bool },
}

const QSIZE: usize = 256;

/// Single-producer single-consumer queue. Safe to push from an interrupt
/// handler while the main code pops, without any locks.
pub struct EventQueue {
    buf: UnsafeCell<[Option<Event>; QSIZE]>,
    head: AtomicUsize, // next slot to write
    tail: AtomicUsize, // next slot to read
}
unsafe impl Sync for EventQueue {}

impl EventQueue {
    pub const fn new() -> Self {
        Self { buf: UnsafeCell::new([None; QSIZE]), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }
    pub fn push(&self, e: Event) {
        let h = self.head.load(Ordering::Relaxed);
        let next = (h + 1) % QSIZE;
        if next == self.tail.load(Ordering::Acquire) { return; } // full: drop event
        unsafe { (*self.buf.get())[h] = Some(e) };
        self.head.store(next, Ordering::Release);
    }
    pub fn pop(&self) -> Option<Event> {
        let t = self.tail.load(Ordering::Relaxed);
        if t == self.head.load(Ordering::Acquire) { return None; }
        let e = unsafe { (*self.buf.get())[t] };
        self.tail.store((t + 1) % QSIZE, Ordering::Release);
        e
    }
}

pub static EVENTS: EventQueue = EventQueue::new();

pub fn next_event() -> Option<Event> { EVENTS.pop() }

/// Is there at least one event waiting? (Used by the scheduler to wake tasks.)
pub fn pending() -> bool {
    EVENTS.head.load(Ordering::Acquire) != EVENTS.tail.load(Ordering::Acquire)
}
