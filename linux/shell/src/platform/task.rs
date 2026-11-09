//! On Linux there's one task (this program); "waiting for input" means:
//! show the finished frame in the window, then collect window events.
pub fn wait_input(ms: u64) { super::window::present_and_wait(ms); }
pub fn kick() {}
pub fn yield_now() {}
pub fn stats() -> (usize, u64) { (1, 0) }
pub fn idle_us() -> u64 { 0 }
