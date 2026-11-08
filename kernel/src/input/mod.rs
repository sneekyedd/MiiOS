//! Input: keyboard and mouse events.
//!
//! Drivers turn raw bytes from the hardware into `Event`s and push them into
//! a queue. Apps pop events from the queue. The queue is a *ring buffer*: a
//! fixed array with a read index and a write index, so it never allocates.

pub mod keyboard;
pub mod mouse;
pub mod ps2;

use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::UnsafeCell;

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
