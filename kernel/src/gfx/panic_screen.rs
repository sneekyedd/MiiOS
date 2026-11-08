//! A friendly crash screen. On the T470 there's no serial port to read the
//! log from, so when the kernel panics we draw the message on screen.

use super::canvas::Canvas;
use super::color::*;
use super::font;
use super::rect::Rect;
use core::sync::atomic::{AtomicU64, Ordering};

static FB: AtomicU64 = AtomicU64::new(0);
static W: AtomicU64 = AtomicU64::new(0);
static H: AtomicU64 = AtomicU64::new(0);
static STRIDE: AtomicU64 = AtomicU64::new(0);

/// Remember where the real screen is, for use when everything else is broken.
pub fn register(fb: *mut u32, w: i32, h: i32, stride: usize) {
    FB.store(fb as u64, Ordering::Relaxed);
    W.store(w as u64, Ordering::Relaxed);
    H.store(h as u64, Ordering::Relaxed);
    STRIDE.store(stride as u64, Ordering::Relaxed);
}

pub fn show(msg: &core::fmt::Arguments) {
    let fb = FB.load(Ordering::Relaxed) as *mut u32;
    if fb.is_null() { return; }
    let (w, h) = (W.load(Ordering::Relaxed) as i32, H.load(Ordering::Relaxed) as i32);
    let mut c = Canvas::new(fb, w, h, STRIDE.load(Ordering::Relaxed) as i32);
    c.fill_rect(Rect::new(0, 0, w, h), hex(0xFFF1F3));
    // A sad little face.
    let (cx, cy) = (w / 2, h / 2 - 160);
    c.fill_circle(cx, cy, 90, hex(0xFFD6E0));
    c.fill_circle(cx - 30, cy - 15, 9, INK);
    c.fill_circle(cx + 30, cy - 15, 9, INK);
    use super::math::fx;
    c.arc_fx(fx(cx), fx(cy + 70), fx(40), fx(20), true, fx(6), INK);
    c.text_centered(&font::LARGE, cx, cy + 130, "MiiOS hit a problem and stopped.", INK);
    let mut buf = crate::util::FmtBuf::<512>::new();
    let _ = core::fmt::write(&mut buf, *msg);
    // Wrap the message over several lines.
    let per_line = ((w - 160) / font::MEDIUM.width as i32) as usize;
    let mut y = cy + 190;
    for line in buf.as_str().split('\n') {
        let mut start = 0;
        while start < line.len() {
            let end = (start + per_line).min(line.len());
            c.text_centered(&font::MEDIUM, cx, y, &line[start..end], INK_SOFT);
            start = end;
            y += 30;
        }
    }
    c.text_centered(&font::MEDIUM, cx, y + 30, "Hold the power button to turn off.", INK_SOFT);
}
