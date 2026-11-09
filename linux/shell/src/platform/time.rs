//! Time, from Linux's clocks.
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static START: OnceLock<Instant> = OnceLock::new();
fn start() -> Instant { *START.get_or_init(Instant::now) }

pub fn uptime_us() -> u64 { start().elapsed().as_micros() as u64 }
pub fn ticks_ms() -> u64 { uptime_us() / 1000 }
pub fn unix_now() -> i64 {
    // MIIOS_TIME_SHIFT=seconds moves the clock (for testing day/night behaviour).
    static SHIFT: OnceLock<i64> = OnceLock::new();
    let shift = *SHIFT.get_or_init(|| std::env::var("MIIOS_TIME_SHIFT").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + shift
}
/// Not meaningful here (the Linux kernel booted us); About shows Linux facts instead.
pub fn us_before_kernel() -> u64 { 0 }
