//! About: live facts about this machine and how fast MiiOS is running.

use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::Key;
use crate::util::FmtBuf;
use core::fmt::Write as _;

pub struct About { cpu: FmtBuf<48>, hover_back: bool, w: i32, h: i32, boot_kernel_us: u64 }

/// The CPU's own name, e.g. "Intel(R) Core(TM) i5-7200U CPU @ 2.50GHz".
fn cpu_name() -> FmtBuf<48> {
    let mut b = FmtBuf::<48>::new();
    let max = core::arch::x86_64::__cpuid(0x8000_0000).eax;
    if max >= 0x8000_0004 {
        for leaf in 0x8000_0002u32..=0x8000_0004 {
            let r = core::arch::x86_64::__cpuid(leaf);
            for reg in [r.eax, r.ebx, r.ecx, r.edx] {
                for byte in reg.to_le_bytes() { if byte != 0 { let _ = b.write_char(byte as char); } }
            }
        }
    }
    let t = FmtBuf::<48>::new();
    let trimmed = b.as_str().trim();
    let mut out = t;
    let _ = out.write_str(if trimmed.is_empty() { "unknown CPU" } else { trimmed });
    out
}

impl About {
    pub fn new(w: i32, h: i32, boot_kernel_us: u64) -> About { About { cpu: cpu_name(), hover_back: false, w, h, boot_kernel_us } }
}

impl App for About {
    fn enter(&mut self, s: &mut Shared) { self.w = s.w; self.h = s.h; }

    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key: Key::Escape | Key::Backspace, .. } => Action::Back,
            UiEvent::Click { x, y } if widgets::back_button().contains(x, y) => Action::Back,
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
                Action::None
            }
            _ => Action::None,
        }
    }

    fn second(&mut self, _s: &mut Shared, d: &mut Damage) { d.add(Rect::new(0, 280, self.w, self.h - 280)); }

    fn draw(&self, c: &mut Canvas, s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "About", self.color(), self.hover_back);
        c.text_centered(&font::HUGE, s.w / 2, 140, "MiiOS", INK);
        c.text_centered(&font::MEDIUM, s.w / 2, 214, "A tiny, fast hobby OS written in Rust. Made for a ThinkPad T470.", INK_SOFT);

        let card = Rect::new((s.w - 1200) / 2, 280, 1200, s.h - 400);
        c.fill_round_rect(card, 36, WHITE);
        let (total, used) = crate::mem::frame::stats();
        let (hs, hu) = crate::mem::heap::stats();
        let (tasks, switches) = crate::task::stats();
        let up_ms = crate::time::uptime_us() / 1000;
        let since_irq = crate::time::ticks_ms().max(1);
        let idle = (crate::task::idle_us() / 10 / since_irq).min(100);
        let storage = crate::fs::info();
        let mut rows: [(&str, FmtBuf<80>); 9] = Default::default();
        let _ = write!(rows[0].1, "{}", self.cpu.as_str()); rows[0].0 = "Processor";
        let _ = write!(rows[1].1, "{} MiB usable, {} MiB in use", total >> 20, used >> 20); rows[1].0 = "Memory";
        let _ = write!(rows[2].1, "{} KiB mapped, {} KiB used", hs >> 10, hu >> 10); rows[2].0 = "Kernel heap";
        let _ = write!(rows[3].1, "{} x {}, drawn by the CPU", s.w, s.h); rows[3].0 = "Screen";
        let _ = write!(rows[4].1, "{} ms firmware + loader, {} ms MiiOS kernel", crate::time::us_before_kernel() / 1000, self.boot_kernel_us / 1000); rows[4].0 = "Boot";
        let _ = write!(rows[5].1, "{} min {} s, CPU idle {}%", up_ms / 60_000, up_ms / 1000 % 60, idle); rows[5].0 = "Uptime";
        let _ = write!(rows[6].1, "{} tasks, {} switches", tasks, switches); rows[6].0 = "Scheduler";
        match storage {
            Some(i) => { let _ = write!(rows[7].1, "{} ({} files)", i.disk, i.files); }
            None => { let _ = write!(rows[7].1, "none set up (see Files)"); }
        }
        rows[7].0 = "Storage";
        let _ = write!(rows[8].1, "Rust (no_std), Limine bootloader, Spleen font"); rows[8].0 = "Built with";
        for (i, (k, v)) in rows.iter().enumerate() {
            let y = card.y + 40 + i as i32 * 52;
            c.text(&font::LARGE, card.x + 48, y, k, INK_SOFT);
            c.text(&font::LARGE, card.x + 300, y, v.as_str(), INK);
        }
    }

    fn color(&self) -> Color { PASTELS[3] }
}
