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

    // Milestone 4: real memory management.
    mem::init();
    let fb_bytes = fb.pitch * fb.height;
    let fb_phys = fb.address as u64 - boot::hhdm_offset();
    match mem::paging::map_framebuffer(fb_phys, fb_bytes) {
        Ok(p) => { screen.set_front(p); log!("framebuffer remapped write-combining at {:p}", p); }
        Err(e) => log!("could not remap framebuffer: {}", e),
    }
    mem_selftest();

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

/// Prove the heap and page tables work: allocate, check, free.
fn mem_selftest() {
    extern crate alloc;
    use alloc::{boxed::Box, string::String, vec::Vec};
    let t = time::uptime_us();
    let b = Box::new(42u64);
    let mut v: Vec<u32> = Vec::new();
    for i in 0..750_000 { v.push(i); } // 3 MB: forces the heap to grow past its first 2 MiB
    let sum: u64 = v.iter().map(|&x| x as u64).sum();
    assert_eq!(sum, 749_999 * 750_000 / 2);
    let mut s = String::from("Mii");
    s.push_str("OS");
    let virt = &*b as *const u64 as u64;
    let phys = mem::paging::translate(virt).unwrap();
    log!("heap test ok: box at {:#x} -> phys {:#x}, vec sum {}, string '{}' ({} us)", virt, phys, sum, s, time::uptime_us() - t);
    drop(v);
    let (total, used) = mem::frame::stats();
    let (hsize, hused) = mem::heap::stats();
    log!("RAM: {} MiB usable, {} KiB used by frames; heap {} KiB mapped, {} bytes in use", total >> 20, used >> 10, hsize >> 10, hused);
}

/// If anything goes badly wrong, Rust calls this. We print why and stop.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log!("KERNEL PANIC: {}", info);
    cpu::halt_forever();
}
