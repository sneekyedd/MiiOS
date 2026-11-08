//! MiiOS kernel entry point.
//!
//! `#![no_std]`  = we can't use Rust's standard library (it needs an OS).
//! `#![no_main]` = there is no normal `main()`; Limine jumps to `kmain`.
#![no_std]
#![no_main]
// Many helpers are written ahead of the milestone that uses them.
#![allow(dead_code)]

mod arch;
mod boot;
mod gfx;
mod serial;
mod time;

use arch::cpu;
use gfx::font;

/// The very first Rust code that runs. Limine jumps here in 64-bit mode with
/// paging on, interrupts off, and a stack ready to use.
#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    time::mark_kernel_entry(cpu::rdtsc());
    serial::init();
    log!("MiiOS: hello from the kernel!");

    if !boot::base_revision_supported() {
        log!("Limine is too old for this kernel");
        cpu::halt_forever();
    }

    let hz = time::calibrate_tsc();
    log!("TSC runs at {} MHz", hz / 1_000_000);

    let Some(fb) = boot::FRAMEBUFFER.get().and_then(|r| r.first()) else {
        log!("no framebuffer!");
        cpu::halt_forever();
    };
    log!("screen: {}x{} pitch={} bpp={}", fb.width, fb.height, fb.pitch, fb.bpp);

    // Milestone 1: paint the screen a soft white and write a greeting.
    let (w, h, pitch) = (fb.width as usize, fb.height as usize, fb.pitch as usize / 4);
    let px = fb.address as *mut u32;
    for y in 0..h {
        unsafe { cpu::fill_u32(px.add(y * pitch), 0x00F4F6FB, w) };
    }
    let draw_text = |x0: usize, y0: usize, f: &font::Font, s: &str, color: u32| {
        for (i, c) in s.chars().enumerate() {
            let g = f.glyph(c);
            for gy in 0..f.height {
                for gx in 0..f.width {
                    if f.bit(g, gx, gy) {
                        let x = x0 + i * f.width as usize + gx as usize;
                        let y = y0 + gy as usize;
                        if x < w && y < h { unsafe { *px.add(y * pitch + x) = color } }
                    }
                }
            }
        }
    };
    draw_text(64, 64, &font::LARGE, "Hello from MiiOS!", 0x003A3F58);
    draw_text(64, 112, &font::SMALL, "Milestone 1: the kernel booted and can draw text.", 0x006C7393);

    log!("boot: {} us before kernel, {} us total", time::us_before_kernel(), time::uptime_us());
    log!("MiiOS ready");
    cpu::halt_forever();
}

/// If anything goes badly wrong, Rust calls this. We print why and stop.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log!("KERNEL PANIC: {}", info);
    cpu::halt_forever();
}
