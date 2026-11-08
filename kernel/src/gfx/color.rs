//! Colours are 32-bit numbers 0xAARRGGBB: alpha (opacity), red, green, blue.
//! Alpha 0xFF = solid. The screen itself ignores alpha; we use it for blending.

pub type Color = u32;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color { 0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32 }
pub const fn hex(v: u32) -> Color { 0xFF00_0000 | v }
pub const fn with_alpha(c: Color, a: u8) -> Color { (c & 0x00FF_FFFF) | (a as u32) << 24 }
pub const fn alpha(c: Color) -> u32 { c >> 24 }

/// Mix `src` over `dst` with strength `a` (0..=256). Works on red+blue together
/// and green separately -- two multiplies per pixel instead of three.
#[inline(always)]
pub fn blend(dst: u32, src: u32, a: u32) -> u32 {
    if a >= 256 { return src; }
    let ia = 256 - a;
    let rb = ((src & 0xFF00FF) * a + (dst & 0xFF00FF) * ia) >> 8;
    let g = ((src & 0x00FF00) * a + (dst & 0x00FF00) * ia) >> 8;
    (rb & 0xFF00FF) | (g & 0x00FF00)
}

/// Mix two colours: t=0 -> a, t=256 -> b.
pub fn mix(a: Color, b: Color, t: u32) -> Color { 0xFF00_0000 | blend(a, b, t) }

/// Lighten (amount>0) or darken (amount<0) a colour by up to 256.
pub fn shade(c: Color, amount: i32) -> Color {
    if amount >= 0 { mix(c, 0xFFFFFFFF, amount as u32) } else { mix(c, 0xFF000000, (-amount) as u32) }
}

// ---- The MiiOS palette: soft whites and pastels ----
pub const BG: Color = hex(0xF4F6FB);
pub const BG_DOT: Color = hex(0xE8ECF5);
pub const WHITE: Color = hex(0xFFFFFF);
pub const INK: Color = hex(0x3A3F58);
pub const INK_SOFT: Color = hex(0x8A90A8);
pub const ACCENT: Color = hex(0x35B6F2);    // friendly sky blue (selection ring)
pub const ACCENT_DARK: Color = hex(0x1A8FD0);
pub const SHADOW: Color = 0x2A30_3858;      // translucent shadow
pub const PASTELS: [Color; 8] = [
    hex(0xFFD6E0), hex(0xFFE9C7), hex(0xFFF6B8), hex(0xD7F5D0),
    hex(0xCDEBFF), hex(0xDCD6FF), hex(0xF5D6FF), hex(0xD0F2EE),
];
