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
mod input;
mod mem;
mod serial;
mod time;
mod util;

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

    input::ps2::init(false);
    let mut demo = demo::Demo::new(w, h);

    let t0 = time::uptime_us();
    screen.damage.add_all();
    screen.render(|c| demo.draw(c));
    log!("full-screen draw + present: {} us", time::uptime_us() - t0);

    log!("boot: {} us before kernel, {} us total", time::us_before_kernel(), time::uptime_us());
    log!("MiiOS ready");

    // Main loop (polling for now; interrupts arrive in milestone 5).
    loop {
        input::ps2::poll();
        let mut any = false;
        while let Some(e) = input::next_event() {
            demo.handle(e, &mut screen.damage);
            any = true;
        }
        if any {
            let t = time::uptime_us();
            let px = screen.render(|c| demo.draw(c));
            log!("input frame: {} px in {} us", px, time::uptime_us() - t);
        }
        core::hint::spin_loop();
    }
}

/// If anything goes badly wrong, Rust calls this. We print why and stop.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log!("KERNEL PANIC: {}", info);
    cpu::halt_forever();
}
