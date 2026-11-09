//! The screen: back buffer, dirty rectangles, and "present".
//!
//! We never draw straight onto the real screen memory (the *framebuffer*):
//! it's slow to read and drawing there would show half-finished frames
//! (flicker). Instead we draw into a *back buffer* in normal RAM, remember
//! which rectangles changed (*damage*), and then copy just those rectangles
//! to the framebuffer. Copying a 300x300 tile is ~350 KB; copying the whole
//! 1080p screen is ~8 MB -- so this matters a lot.

use super::canvas::Canvas;
use super::rect::Rect;
use crate::arch::cpu;

const MAX_DAMAGE: usize = 32;

pub struct Damage {
    rects: [Rect; MAX_DAMAGE],
    n: usize,
    bounds: Rect,
}

impl Damage {
    pub const fn new(bounds: Rect) -> Damage { Damage { rects: [Rect::new(0, 0, 0, 0); MAX_DAMAGE], n: 0, bounds } }

    /// Mark a region as changed. Overlapping regions are merged; if the list
    /// is full we merge the new one into whichever grows the least.
    pub fn add(&mut self, r: Rect) {
        let mut r = r.intersect(&self.bounds);
        if r.is_empty() { return; }
        // Merge with any rect where the union isn't much bigger than the parts.
        let mut i = 0;
        while i < self.n {
            let u = self.rects[i].union(&r);
            if u.area() <= self.rects[i].area() + r.area() + 4096 {
                r = u;
                self.n -= 1;
                self.rects[i] = self.rects[self.n];
                i = 0; // the bigger rect may now touch others
                continue;
            }
            i += 1;
        }
        if self.n < MAX_DAMAGE {
            self.rects[self.n] = r;
            self.n += 1;
        } else {
            let mut best = 0;
            let mut best_grow = i64::MAX;
            for (j, e) in self.rects.iter().enumerate() {
                let g = e.union(&r).area() - e.area();
                if g < best_grow { best_grow = g; best = j; }
            }
            self.rects[best] = self.rects[best].union(&r);
        }
    }

    pub fn add_all(&mut self) { self.n = 0; self.add(self.bounds); }
    pub fn rects(&self) -> &[Rect] { &self.rects[..self.n] }
    pub fn clear(&mut self) { self.n = 0; }
    pub fn is_empty(&self) -> bool { self.n == 0 }
    pub fn total_area(&self) -> i64 { self.rects().iter().map(|r| r.area()).sum() }
}

pub struct Screen {
    pub canvas: Canvas,     // the back buffer we draw into
    front: *mut u32,        // the real framebuffer
    front_stride: usize,    // pixels per row in the framebuffer
    pub damage: Damage,
    pub frames: u64,
    pub pixels_copied: u64,
}

impl Screen {
    /// `back` must point to width*height u32s of RAM.
    pub fn new(front: *mut u32, front_stride: usize, back: *mut u32, width: i32, height: i32) -> Screen {
        let canvas = Canvas::new(back, width, height, width);
        let bounds = canvas.bounds();
        Screen { canvas, front, front_stride, damage: Damage::new(bounds), frames: 0, pixels_copied: 0 }
    }

    pub fn width(&self) -> i32 { self.canvas.width }
    pub fn height(&self) -> i32 { self.canvas.height }

    /// Redraw every damaged region by calling `draw` with the clip set to it,
    /// then copy those regions to the real screen. Returns pixels copied.
    pub fn render(&mut self, mut draw: impl FnMut(&mut Canvas)) -> u64 {
        if self.damage.is_empty() { return 0; }
        let n = self.damage.n;
        let rects = self.damage.rects;
        for r in &rects[..n] {
            self.canvas.set_clip(*r);
            draw(&mut self.canvas);
        }
        self.canvas.set_clip(self.canvas.bounds());
        let mut copied = 0;
        for r in &rects[..n] { copied += self.present_rect(*r); }
        crate::host::presented(&rects[..n]); // the Linux edition tells the display what changed
        self.damage.clear();
        self.frames += 1;
        self.pixels_copied += copied;
        copied
    }

    /// Copy one rectangle from the back buffer to the framebuffer.
    pub fn present_rect(&mut self, r: Rect) -> u64 {
        let r = r.intersect(&self.canvas.bounds());
        if r.is_empty() { return 0; }
        for y in r.y..r.bottom() {
            unsafe {
                let src = self.canvas.buf.add(y as usize * self.canvas.stride as usize + r.x as usize);
                let dst = self.front.add(y as usize * self.front_stride + r.x as usize);
                cpu::copy_bytes(dst as *mut u8, src as *const u8, r.w as usize * 4);
            }
        }
        r.area() as u64
    }

    /// Change where the front buffer lives (after remapping it write-combining).
    pub fn set_front(&mut self, front: *mut u32) { self.front = front; }
}
