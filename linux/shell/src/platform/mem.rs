//! Memory numbers, read from /proc/meminfo.
pub mod frame {
    /// (total bytes, bytes in use)
    pub fn stats() -> (u64, u64) {
        let s = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let get = |k: &str| s.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()).unwrap_or(0) * 1024;
        let (total, avail) = (get("MemTotal:"), get("MemAvailable:"));
        (total, total.saturating_sub(avail))
    }
}
pub mod heap {
    pub fn stats() -> (usize, usize) { (0, 0) }
}
