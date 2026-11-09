//! Input: keyboard and mouse events.
//!
//! Drivers turn raw bytes from the hardware into `Event`s and push them into
//! a queue. Apps pop events from the queue. The queue is a *ring buffer*: a
//! fixed array with a read index and a write index, so it never allocates.

pub mod keyboard;
pub mod mouse;
pub mod ps2;


mod types;
pub use types::*;
