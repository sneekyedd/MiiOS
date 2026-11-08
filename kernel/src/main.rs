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
mod demo;
mod gfx;
mod mem;
mod serial;
mod time;

use arch::cpu;

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

    // Milestone 2: a back buffer in RAM, then a demo scene drawn with our primitives.
    let (w, h) = (fb.width as i32, fb.height as i32);
    let back = mem::early::alloc_phys((w * h * 4) as u64).expect("no RAM for back buffer");
    let mut screen = gfx::screen::Screen::new(
        fb.address as *mut u32, fb.pitch as usize / 4, mem::early::phys_to_virt(back), w, h);

    let t0 = time::uptime_us();
    screen.damage.add_all();
    screen.render(|c| demo::draw(c, 0));
    let full = time::uptime_us() - t0;
    log!("full-screen draw + present: {} us", full);

    log!("boot: {} us before kernel, {} us total", time::us_before_kernel(), time::uptime_us());
    log!("MiiOS ready");

    // Animate a small bouncing ball to show dirty rectangles: only ~2 small
    // rects are redrawn and copied per frame instead of the whole screen.
    let mut frame = 0;
    loop {
        let t = time::uptime_us();
        let old = demo::ball_rect(frame);
        frame += 1;
        screen.damage.add(old);
        screen.damage.add(demo::ball_rect(frame));
        let px = screen.render(|c| demo::draw(c, frame));
        if frame % 120 == 0 {
            log!("frame {}: redrew {} px in {} us", frame, px, time::uptime_us() - t);
        }
        time::spin_us(16_000);
    }
}

/// If anything goes badly wrong, Rust calls this. We print why and stop.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log!("KERNEL PANIC: {}", info);
    cpu::halt_forever();
}
