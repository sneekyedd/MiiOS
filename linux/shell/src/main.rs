//! miios-shell: the MiiOS home screen, running as a normal Linux program.
//!
//! The clever bit: the user interface is *not* rewritten. The `#[path]`
//! lines below pull in the very same source files the bare-metal kernel uses
//! (`kernel/src/ui`, `kernel/src/gfx`, ...). Those files call a handful of
//! "platform" functions (`crate::time::uptime_us`, `crate::fs::write`,
//! `crate::task::wait_input`, ...). The kernel implements them with hardware
//! drivers; this program implements them with Linux (files in ~/MiiOS, a
//! window from the `minifb` library, NetworkManager for Wi-Fi). Same code,
//! two platforms.

#![allow(dead_code)]

#[path = "../../../kernel/src/gfx/mod.rs"]
mod gfx;
#[path = "../../../kernel/src/ui/mod.rs"]
mod ui;
#[path = "../../../kernel/src/util.rs"]
mod util;
#[path = "../../../kernel/src/input/types.rs"]
mod input;

mod platform;
pub use platform::{arch, fs, host, mem, task, time};

/// `log!(...)`: on Linux, messages go to standard error (and the session log).
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { eprintln!("miios: {}", format_args!($($arg)*)) };
}

/// `miios-shell --render MII.glb OUT.png [yaw] [walk] [wave]`: draw one Mii
/// into a PNG file (handy for testing the 3D renderer without a screen).
fn render_cli(a: &[String]) {
    let bytes = std::fs::read(&a[2]).expect("read .glb");
    let m = platform::mii3d::Model::load(&bytes).expect("load .glb");
    let num = |i: usize| a.get(i).and_then(|v| v.parse::<i32>().ok());
    let pose = platform::mii3d::Pose { yaw: num(4).unwrap_or(0), walk: num(5).filter(|&v| v >= 0), wave: num(6).filter(|&v| v >= 0) };
    let (w, h) = (a.get(7).and_then(|v| v.parse().ok()).unwrap_or(300usize), a.get(8).and_then(|v| v.parse().ok()).unwrap_or(400usize));
    let t = std::time::Instant::now();
    let px = m.render(&pose, w, h, 0.93);
    eprintln!("{}: rendered {}x{} in {} ms", m.name, w, h, t.elapsed().as_millis());
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in px { rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]); }
    let mut enc = png::Encoder::new(std::fs::File::create(&a[3]).expect("create png"), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&rgba).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("--render") { return render_cli(&args); }
    // Screen size: passed by the session script (from Sway), else 1920x1080.
    let (w, h) = std::env::args().nth(1)
        .and_then(|s| { let (a, b) = s.split_once('x')?; Some((a.parse().ok()?, b.parse().ok()?)) })
        .unwrap_or((1920i32, 1080i32));
    let started = std::time::Instant::now();
    platform::window::open(w, h);
    platform::server::start(); // serves Mii Hub + your Miis on 127.0.0.1:8765

    let back: &'static mut [u32] = Box::leak(vec![0u32; (w * h) as usize].into_boxed_slice());
    let screen = gfx::screen::Screen::new(platform::window::front(), w as usize, back.as_mut_ptr(), w, h);

    let apps: Vec<(ui::AppId, Box<dyn ui::App>)> = vec![
        (ui::AppId::Pals, Box::new(ui::pals::Pals::new(w, h))),
        (ui::AppId::Miis, Box::new(ui::miis::Miis::new(w, h))),
        (ui::AppId::Write, Box::new(ui::write::Write::new(w, h))),
        (ui::AppId::Files, Box::new(ui::files::Files::new(w, h))),
        (ui::AppId::Clock, Box::new(ui::clockapp::ClockApp::new(w, h))),
        (ui::AppId::Settings, Box::new(ui::settings::Settings::new(w, h))),
        (ui::AppId::Wifi, Box::new(ui::wifi::Wifi::new(w, h))),
        (ui::AppId::About, Box::new(ui::about::About::new(w, h, 0))),
    ];
    let home = ui::home::Home::new(w, h);
    let mut shell = ui::shell::Shell::new(screen, home, apps);
    log!("ready in {} ms ({}x{})", started.elapsed().as_millis(), w, h);
    shell.run();
}
