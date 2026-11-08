//! Bitmap fonts.
//!
//! A bitmap font stores each letter as a little grid of on/off pixels:
//! one bit per pixel, one row after another. We embed "Spleen" (BSD licence,
//! see assets/fonts/LICENSE-spleen) in two sizes, in the standard Linux
//! console format "PSF". `include_bytes!` copies the file into the kernel at
//! compile time, so there is no file loading at boot.

pub struct Font {
    pub width: u32,
    pub height: u32,
    bytes_per_row: u32,
    bytes_per_glyph: u32,
    glyph_count: u32,
    glyphs: &'static [u8],
}

static SMALL_DATA: &[u8] = include_bytes!("../../assets/fonts/spleen-8x16.psfu");
static MEDIUM_DATA: &[u8] = include_bytes!("../../assets/fonts/spleen-12x24.psfu");
static LARGE_DATA: &[u8] = include_bytes!("../../assets/fonts/spleen-16x32.psfu");

/// 8x16 pixels per letter.
pub static SMALL: Font = Font::psf1(SMALL_DATA);
/// 12x24 pixels per letter.
pub static MEDIUM: Font = Font::psf2(MEDIUM_DATA);
/// 16x32 pixels per letter (good for the 1080p screen).
pub static LARGE: Font = Font::psf2(LARGE_DATA);

const fn u32le(d: &[u8], o: usize) -> u32 {
    d[o] as u32 | (d[o + 1] as u32) << 8 | (d[o + 2] as u32) << 16 | (d[o + 3] as u32) << 24
}

impl Font {
    /// PSF version 1: 4-byte header, always 8 pixels wide.
    const fn psf1(d: &'static [u8]) -> Font {
        assert!(d[0] == 0x36 && d[1] == 0x04);
        let h = d[3] as u32;
        let count = if d[2] & 1 != 0 { 512 } else { 256 };
        let (_, glyphs) = d.split_at(4);
        Font { width: 8, height: h, bytes_per_row: 1, bytes_per_glyph: h, glyph_count: count, glyphs }
    }

    /// PSF version 2: 32-byte header with explicit sizes.
    const fn psf2(d: &'static [u8]) -> Font {
        assert!(u32le(d, 0) == 0x864ab572);
        let header = u32le(d, 8) as usize;
        let count = u32le(d, 16);
        let bpg = u32le(d, 20);
        let h = u32le(d, 24);
        let w = u32le(d, 28);
        let (_, glyphs) = d.split_at(header);
        Font { width: w, height: h, bytes_per_row: (w + 7) / 8, bytes_per_glyph: bpg, glyph_count: count, glyphs }
    }

    /// The bitmap rows for one character. Unknown characters become '?'.
    #[inline]
    pub fn glyph(&self, c: char) -> &'static [u8] {
        let mut i = c as u32;
        if i >= self.glyph_count || i >= 256 { i = b'?' as u32; }
        let s = (i * self.bytes_per_glyph) as usize;
        &self.glyphs[s..s + self.bytes_per_glyph as usize]
    }

    /// Is pixel (x, y) of a glyph switched on?
    #[inline(always)]
    pub fn bit(&self, g: &[u8], x: u32, y: u32) -> bool {
        let byte = g[(y * self.bytes_per_row + x / 8) as usize];
        byte & (0x80 >> (x & 7)) != 0
    }

    pub fn text_width(&self, s: &str) -> u32 { s.chars().count() as u32 * self.width }
}
