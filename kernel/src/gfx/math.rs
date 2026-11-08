//! Fixed-point math.
//!
//! The kernel avoids floating point (`f32`) entirely: on this target it would
//! be emulated in software and slow. Instead we use *fixed-point* numbers:
//! an ordinary integer where the lowest 8 bits are the fraction.
//! So `256` means 1.0, `128` means 0.5, and `3 * 256 + 64` means 3.25.
//! Adding two fixed-point numbers is a normal add; multiplying needs a shift.

/// A 24.8 fixed-point number (stored in an i32).
pub type Fx = i32;
pub const ONE: Fx = 256;
pub const HALF: Fx = 128;

#[inline(always)] pub const fn fx(i: i32) -> Fx { i << 8 }
#[inline(always)] pub const fn fx_floor(v: Fx) -> i32 { v >> 8 }
#[inline(always)] pub const fn fx_ceil(v: Fx) -> i32 { (v + 255) >> 8 }
#[inline(always)] pub const fn fx_round(v: Fx) -> i32 { (v + 128) >> 8 }
#[inline(always)] pub const fn fx_mul(a: Fx, b: Fx) -> Fx { ((a as i64 * b as i64) >> 8) as i32 }
#[inline(always)] pub const fn fx_div(a: Fx, b: Fx) -> Fx { (((a as i64) << 8) / b as i64) as i32 }

/// Integer square root (rounded down), no floats needed.
pub fn isqrt(n: u64) -> u64 {
    if n < 2 { return n; }
    // Start from a good guess using the number of bits, then refine (Newton's method).
    let shift = (64 - n.leading_zeros() + 1) / 2;
    let mut x = 1u64 << shift;
    loop {
        let y = (x + n / x) / 2;
        if y >= x { return x; }
        x = y;
    }
}

/// Sine table for one full turn, 0..=255 "angle units" -> -256..=256 (fixed-point).
/// Generated at compile time with integer-only math (a Bhaskara-style approximation).
pub static SIN: [i16; 256] = {
    let mut t = [0i16; 256];
    let mut i = 0;
    while i < 256 {
        // x in 0..128 is the first half turn; Bhaskara I: sin(x) ~ 16x(pi-x)/(5pi^2-4x(pi-x))
        let half = i % 128;
        let x = half as i64; // 0..128 maps to 0..pi
        let p = 128i64;
        let num = 16 * x * (p - x);
        let den = 5 * p * p - 4 * x * (p - x);
        let v = (num * 256 / den) as i16;
        t[i] = if i < 128 { v } else { -v };
        i += 1;
    }
    t
};

/// sin of an angle in 1/256 turns, result in 24.8 fixed point.
#[inline] pub fn sin(a: i32) -> Fx { SIN[(a & 255) as usize] as Fx }
#[inline] pub fn cos(a: i32) -> Fx { SIN[((a + 64) & 255) as usize] as Fx }

/// Linear interpolation between a and b by t (0..=256).
#[inline] pub fn lerp(a: i32, b: i32, t: i32) -> i32 { a + (((b - a) as i64 * t as i64) >> 8) as i32 }

/// "Ease out cubic": fast at the start, gentle at the end. t and result in 0..=256.
pub fn ease_out(t: i32) -> i32 {
    let u = 256 - t.clamp(0, 256);
    256 - ((u as i64 * u as i64 * u as i64) >> 16) as i32
}

/// "Ease out back": overshoots a little then settles -- a bouncy, playful feel.
pub fn ease_out_back(t: i32) -> i32 {
    // f(t) = 1 + c3 (t-1)^3 + c1 (t-1)^2, with c1 = 1.70158, c3 = c1 + 1
    let t = t.clamp(0, 256) as i64;
    let u = t - 256;
    let c1 = 436; // 1.70158 * 256
    let c3 = 692; // 2.70158 * 256
    (256 + ((c3 * u * u * u) >> 24) + ((c1 * u * u) >> 16)) as i32
}
