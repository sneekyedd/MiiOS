/// An axis-aligned rectangle in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect { Rect { x, y, w, h } }
    pub const fn right(&self) -> i32 { self.x + self.w }
    pub const fn bottom(&self) -> i32 { self.y + self.h }
    pub const fn is_empty(&self) -> bool { self.w <= 0 || self.h <= 0 }
    pub const fn area(&self) -> i64 { if self.is_empty() { 0 } else { self.w as i64 * self.h as i64 } }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        Rect::new(x, y, (r - x).max(0), (b - y).max(0))
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() { return *o; }
        if o.is_empty() { return *self; }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }

    pub fn overlaps(&self, o: &Rect) -> bool { !self.intersect(o).is_empty() }
    pub fn contains(&self, px: i32, py: i32) -> bool { px >= self.x && py >= self.y && px < self.right() && py < self.bottom() }
    pub fn inflate(&self, d: i32) -> Rect { Rect::new(self.x - d, self.y - d, self.w + 2 * d, self.h + 2 * d) }
    pub fn offset(&self, dx: i32, dy: i32) -> Rect { Rect::new(self.x + dx, self.y + dy, self.w, self.h) }
}
