//! Stand-ins for the kernel's CPU helpers, and power via systemd.
pub mod cpu {
    #[inline(always)]
    pub fn rdtsc() -> u64 { unsafe { core::arch::x86_64::_rdtsc() } }
    #[inline(always)]
    pub unsafe fn fill_u32(dst: *mut u32, val: u32, count: usize) {
        unsafe { core::slice::from_raw_parts_mut(dst, count).fill(val) }
    }
    #[inline(always)]
    pub unsafe fn copy_bytes(dst: *mut u8, src: *const u8, count: usize) {
        unsafe { core::ptr::copy_nonoverlapping(src, dst, count) }
    }
}
pub mod power {
    pub fn power_off() -> ! {
        let _ = std::process::Command::new("systemctl").arg("poweroff").status();
        std::process::exit(0)
    }
    pub fn restart() -> ! {
        let _ = std::process::Command::new("systemctl").arg("reboot").status();
        std::process::exit(0)
    }
}
