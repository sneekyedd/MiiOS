//! Small helpers.

/// A fixed-size text buffer you can `write!` into without a heap.
pub struct FmtBuf<const N: usize> { buf: [u8; N], len: usize }
impl<const N: usize> FmtBuf<N> {
    pub const fn new() -> Self { Self { buf: [0; N], len: 0 } }
    pub fn as_str(&self) -> &str { core::str::from_utf8(&self.buf[..self.len]).unwrap_or("") }
    pub fn as_bytes(&self) -> &[u8] { &self.buf[..self.len] }
    pub fn len(&self) -> usize { self.len }
    pub fn clear(&mut self) { self.len = 0; }
}
impl<const N: usize> core::fmt::Write for FmtBuf<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() { if self.len < N { self.buf[self.len] = b; self.len += 1; } }
        Ok(())
    }
}
