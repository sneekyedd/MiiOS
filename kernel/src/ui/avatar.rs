//! Avatars ("Pals"): cheerful cartoon people built from simple shapes.
//!
//! An avatar is just a handful of small numbers (which face shape, which
//! hair style, which colour...), so it takes ~30 bytes and is trivial to
//! save. Drawing turns those numbers into ellipses, rounded rectangles,
//! arcs and thick lines. Everything is measured relative to the head width
//! `s`, so the same code draws a tiny icon or a big preview.
//!
//! The style is our own original design: round friendly faces, dot eyes,
//! big heads on small bodies.

use crate::gfx::color::*;
use crate::gfx::math::*;
use crate::gfx::{Canvas, Color, Rect};

pub const SKIN: [Color; 8] = [hex(0xFFE5D0), hex(0xFDD3B5), hex(0xF2C09A), hex(0xE2A97F), hex(0xC88B60), hex(0xA86C46), hex(0x8A5236), hex(0x6B3F2A)];
pub const HAIR: [Color; 8] = [hex(0x2E2B36), hex(0x5A3825), hex(0x8D5A2E), hex(0xE6BE6E), hex(0xC0532D), hex(0xA3A9B5), hex(0xFF8FB8), hex(0x6FA8FF)];
pub const EYES: [Color; 8] = [hex(0x5B3A29), hex(0x2E2B36), hex(0x3D8BD9), hex(0x4AA36C), hex(0x9C7A3C), hex(0x7A8494), hex(0x8E6BD9), hex(0xC98A1E)];
pub const SHIRT: [Color; 8] = [hex(0x35B6F2), hex(0xFF6F91), hex(0x7ED957), hex(0xFFC94D), hex(0x9C8CF0), hex(0xFF9F45), hex(0x3A3F58), hex(0xF7F8FC)];
pub const FRAMES: [Color; 8] = [hex(0x2E2B36), hex(0xC0532D), hex(0x35B6F2), hex(0xFF6F91), hex(0x7ED957), hex(0x9C8CF0), hex(0xE6BE6E), hex(0xA3A9B5)];

pub const FACE_STYLES: u8 = 5;
pub const HAIR_STYLES: u8 = 8;
pub const EYE_STYLES: u8 = 6;
pub const BROW_STYLES: u8 = 4;
pub const NOSE_STYLES: u8 = 4;
pub const MOUTH_STYLES: u8 = 6;
pub const GLASSES_STYLES: u8 = 4;
pub const CHEEK_STYLES: u8 = 4;
pub const BODY_STYLES: u8 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Avatar {
    pub name: [u8; 16],
    pub name_len: u8,
    pub face: u8, pub skin: u8,
    pub hair: u8, pub hair_color: u8,
    pub eyes: u8, pub eye_color: u8,
    pub brows: u8,
    pub nose: u8,
    pub mouth: u8,
    pub glasses: u8, pub glasses_color: u8,
    pub cheeks: u8,
    pub body: u8, pub shirt: u8,
}

impl Avatar {
    pub const fn default_me() -> Avatar {
        Avatar { name: *b"Me\0\0\0\0\0\0\0\0\0\0\0\0\0\0", name_len: 2, face: 0, skin: 1, hair: 1, hair_color: 1,
                 eyes: 0, eye_color: 0, brows: 1, nose: 0, mouth: 0, glasses: 0, glasses_color: 0, cheeks: 1, body: 2, shirt: 0 }
    }

    pub fn name(&self) -> &str { core::str::from_utf8(&self.name[..self.name_len as usize]).unwrap_or("?") }

    pub fn set_name(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(16);
        self.name = [0; 16];
        self.name[..n].copy_from_slice(&b[..n]);
        self.name_len = n as u8;
    }

    /// A random avatar (for "Surprise me!" and new pals).
    pub fn random(rng: &mut Rng) -> Avatar {
        const NAMES: [&str; 12] = ["Pip", "Juno", "Milo", "Kiki", "Bea", "Otto", "Nori", "Lulu", "Taro", "Ivy", "Rex", "Momo"];
        let mut a = Avatar::default_me();
        a.face = rng.below(FACE_STYLES as u32) as u8;
        a.skin = rng.below(8) as u8;
        a.hair = rng.below(HAIR_STYLES as u32) as u8;
        a.hair_color = rng.below(8) as u8;
        a.eyes = rng.below(EYE_STYLES as u32) as u8;
        a.eye_color = rng.below(8) as u8;
        a.brows = rng.below(BROW_STYLES as u32) as u8;
        a.nose = rng.below(NOSE_STYLES as u32) as u8;
        a.mouth = rng.below(MOUTH_STYLES as u32) as u8;
        a.glasses = if rng.below(3) == 0 { 1 + rng.below(3) as u8 } else { 0 };
        a.glasses_color = rng.below(8) as u8;
        a.cheeks = rng.below(CHEEK_STYLES as u32) as u8;
        a.body = rng.below(BODY_STYLES as u32) as u8;
        a.shirt = rng.below(8) as u8;
        a.set_name(NAMES[rng.below(NAMES.len() as u32) as usize]);
        a
    }
}

/// Tiny random number generator (xorshift). Good enough for picking hair styles!
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Rng { Rng(seed | 1) }
    pub fn next(&mut self) -> u64 { let mut x = self.0; x ^= x << 13; x ^= x >> 7; x ^= x << 17; self.0 = x; x }
    pub fn below(&mut self, n: u32) -> u32 { (self.next() % n as u64) as u32 }
}

/// How the avatar is posed right now.
#[derive(Clone, Copy, Default)]
pub struct Pose { pub blink: bool, pub wave: i32 /* 0 = arms down, else animation phase */, pub bounce: Fx }

/// Run `f` with drawing limited to `r` as well as the current clip.
fn clipped(c: &mut Canvas, r: Rect, f: impl FnOnce(&mut Canvas)) {
    let saved = c.clip;
    c.clip = saved.intersect(&r);
    if !c.clip.is_empty() { f(c); }
    c.clip = saved;
}

/// Draw just the head, centred at (cx, cy) (pixels), `s` pixels wide.
pub fn draw_head(c: &mut Canvas, cx: i32, cy: i32, s: i32, a: &Avatar, pose: Pose) {
    let bb = Rect::new(cx - s, cy - s, s * 2, s * 2);
    if !bb.overlaps(&c.clip) || s < 8 { return; }
    let (cx, cy) = (fx(cx), fx(cy) + pose.bounce);
    // u(k) = k percent of the head width, in fixed point.
    let u = |k: i32| -> Fx { s * k * ONE / 100 };
    let skin = SKIN[a.skin as usize % 8];
    let skin_dark = shade(skin, -40);
    let hair = HAIR[a.hair_color as usize % 8];
    let hair_dark = shade(hair, -30);

    // ---- Hair behind the head (long styles) ----
    match a.hair {
        2 => c.fill_round_rect_fx(cx - u(56), cy - u(52), u(112), u(92), u(44), hair),          // bob
        4 => c.fill_round_rect_fx(cx - u(57), cy - u(50), u(114), u(128), u(48), hair),         // long
        6 => { // curly: a ring of puffs
            for i in 0..12 {
                let ang = i * 256 / 12;
                let (dx, dy) = (sin(ang) * s * 50 / 100, -cos(ang) * s * 48 / 100);
                if dy > u(20) { continue; }
                c.fill_ellipse_fx(cx + dx, cy - u(6) + dy, u(18), u(18), hair);
            }
        }
        _ => {}
    }

    // ---- Ears ----
    let (rx, ry) = match a.face { 1 => (u(44), u(54)), 2 => (u(52), u(46)), 4 => (u(42), u(58)), _ => (u(48), u(51)) };
    c.fill_ellipse_fx(cx - rx + u(2), cy + u(4), u(10), u(13), skin_dark);
    c.fill_ellipse_fx(cx + rx - u(2), cy + u(4), u(10), u(13), skin_dark);
    c.fill_ellipse_fx(cx - rx + u(2), cy + u(4), u(7), u(10), skin);
    c.fill_ellipse_fx(cx + rx - u(2), cy + u(4), u(7), u(10), skin);

    // ---- Face ----
    match a.face {
        3 => c.fill_round_rect_fx(cx - u(47), cy - u(50), u(94), u(102), u(36), skin), // soft square
        _ => c.fill_ellipse_fx(cx, cy, rx, ry, skin),
    }

    // ---- Cheeks ----
    if a.cheeks & 1 != 0 {
        let blush = with_alpha(hex(0xFF7F9E), 90);
        c.fill_ellipse_fx(cx - u(29), cy + u(17), u(10), u(6), blush);
        c.fill_ellipse_fx(cx + u(29), cy + u(17), u(10), u(6), blush);
    }
    if a.cheeks & 2 != 0 {
        let fr = with_alpha(shade(skin, -90), 120);
        for (dx, dy) in [(-30, 10), (-24, 14), (-34, 15), (30, 10), (24, 14), (34, 15)] {
            c.fill_ellipse_fx(cx + u(dx), cy + u(dy), u(2) + 64, u(2) + 64, fr);
        }
    }

    // ---- Eyes ----
    let ey = cy + u(2);
    for side in [-1, 1] { draw_eye(c, a, cx + side * u(18), ey, s, pose.blink, skin); }

    // ---- Eyebrows (hair coloured) ----
    let by = cy - u(13);
    let bw = u(4) + 64;
    for side in [-1, 1] {
        let bx = cx + side * u(18);
        match a.brows {
            1 => c.line_fx(bx - u(7), by, bx + u(7), by, bw, hair_dark),
            2 => c.arc_fx(bx, by + u(5), u(8), u(2), true, bw, hair_dark),
            3 => c.line_fx(bx - side * u(7), by - u(1), bx + side * u(7), by - u(4), bw, hair_dark), // worried
            _ => {}
        }
    }

    // ---- Nose ----
    let ny = cy + u(14);
    match a.nose {
        0 => c.fill_ellipse_fx(cx, ny, u(4), u(3), skin_dark),
        1 => { c.fill_ellipse_fx(cx, ny, u(6), u(5), shade(skin, -18)); c.fill_ellipse_fx(cx - u(2), ny - u(2), u(2), u(1) + 64, with_alpha(WHITE, 90)); }
        2 => c.arc_fx(cx + u(1), ny - u(3), u(4), u(0), false, u(2) + 64, skin_dark),
        _ => {}
    }

    // ---- Mouth ----
    let my = cy + u(26);
    let lip = hex(0xC2445E);
    match a.mouth {
        0 => c.arc_fx(cx, my - u(9), u(12), u(6), false, u(4), INK),                  // smile
        1 => { // open grin
            clipped(c, Rect::new(fx_floor(cx - u(14)), fx_floor(my - u(4)), fx_ceil(u(28)) + 2, fx_ceil(u(16)) + 2), |c| {
                c.fill_ellipse_fx(cx, my - u(4), u(12), u(12), lip);
                c.fill_ellipse_fx(cx, my + u(6), u(7), u(4), hex(0xFF8FA3));
            });
        }
        2 => c.fill_ellipse_fx(cx, my, u(5), u(6), lip),                              // "o"
        3 => c.line_fx(cx - u(9), my, cx + u(9), my, u(4), INK),                      // flat
        4 => { c.arc_fx(cx - u(5), my - u(3), u(5), u(1), false, u(3) + 64, INK); c.arc_fx(cx + u(5), my - u(3), u(5), u(1), false, u(3) + 64, INK); } // cat
        _ => { // big toothy smile
            clipped(c, Rect::new(fx_floor(cx - u(16)), fx_floor(my - u(4)), fx_ceil(u(32)) + 2, fx_ceil(u(18)) + 2), |c| {
                c.fill_ellipse_fx(cx, my - u(4), u(14), u(14), lip);
                c.fill_ellipse_fx(cx, my - u(8), u(12), u(10), WHITE);
            });
        }
    }

    // ---- Hair on top ----
    let top = cy - u(52);
    let fringe = |c: &mut Canvas, line: Fx| {
        // A cap of hair: a big ellipse over the head, cut off at `line`.
        clipped(c, Rect::new(fx_floor(cx - u(70)), fx_floor(top - u(30)), fx_ceil(u(140)), fx_floor(line - top + u(30))), |c| {
            c.fill_ellipse_fx(cx, cy - u(2), u(55), u(57), hair);
        });
    };
    match a.hair {
        0 => {}
        1 => { // short with a small side swoop
            fringe(c, cy - u(22));
            c.fill_ellipse_fx(cx + u(14), cy - u(27), u(24), u(10), hair);
            c.fill_round_rect_fx(cx - u(52), cy - u(30), u(9), u(22), u(4), hair);
            c.fill_round_rect_fx(cx + u(43), cy - u(30), u(9), u(22), u(4), hair);
        }
        2 => { // bob with straight bangs
            fringe(c, cy - u(18));
            c.fill_round_rect_fx(cx - u(56), cy - u(30), u(16), u(62), u(8), hair);
            c.fill_round_rect_fx(cx + u(40), cy - u(30), u(16), u(62), u(8), hair);
        }
        3 => { // spiky
            fringe(c, cy - u(24));
            for i in 0..7 {
                let ang = 256 - 48 + i * 16; // fan across the top
                let (dx, dy) = (sin(ang) * s / 100, -cos(ang) * s / 100);
                let (bx, byy) = (cx + dx * 40, cy - u(8) + dy * 40);
                c.line_fx(bx, byy, bx + dx * 18, byy + dy * 18, u(17), hair);
            }
        }
        4 => { // long, parted in the middle
            fringe(c, cy - u(20));
            c.fill_ellipse_fx(cx - u(24), cy - u(28), u(28), u(14), hair);
            c.fill_ellipse_fx(cx + u(24), cy - u(28), u(28), u(14), hair);
        }
        5 => { // bun on top
            c.fill_ellipse_fx(cx, top - u(6), u(18), u(16), hair);
            fringe(c, cy - u(24));
            c.fill_ellipse_fx(cx, top - u(6), u(10), u(4), hair_dark);
        }
        6 => { // curly: puffs along the hairline
            fringe(c, cy - u(26));
            for i in 0..7 {
                let x = cx - u(36) + i * u(12);
                c.fill_ellipse_fx(x, cy - u(28) + if i % 2 == 0 { u(2) } else { 0 }, u(10), u(9), hair);
            }
        }
        _ => { // side part swoop
            fringe(c, cy - u(30));
            c.fill_ellipse_fx(cx - u(16), cy - u(24), u(34), u(14), hair);
            c.fill_ellipse_fx(cx + u(38), cy - u(30), u(12), u(16), hair);
        }
    }

    // ---- Glasses (outlines, so the eyes still show through) ----
    let frame = FRAMES[a.glasses_color as usize % 8];
    let gw = u(4) + 32;
    match a.glasses {
        1 => {
            for side in [-1, 1] { c.ring_fx(cx + side * u(18), ey, u(12), gw, frame); }
            c.line_fx(cx - u(6), ey - u(2), cx + u(6), ey - u(2), gw, frame);
        }
        2 => {
            for side in [-1, 1] {
                let ex = fx_round(cx + side * u(18));
                let (w2, h2) = (fx_round(u(14)), fx_round(u(11)));
                c.stroke_round_rect(Rect::new(ex - w2, fx_round(ey) - h2, w2 * 2, h2 * 2), fx_round(u(6)).max(1), fx_round(gw).max(1), frame);
            }
            c.line_fx(cx - u(4), ey - u(2), cx + u(4), ey - u(2), gw, frame);
        }
        3 => { // sunglasses
            for side in [-1, 1] {
                let ex = cx + side * u(18);
                c.fill_round_rect_fx(ex - u(14), ey - u(10), u(28), u(20), u(9), hex(0x2E2B36));
                c.fill_round_rect_fx(ex - u(10), ey - u(7), u(9), u(4), u(2), with_alpha(WHITE, 110));
            }
            c.line_fx(cx - u(5), ey - u(3), cx + u(5), ey - u(3), gw, hex(0x2E2B36));
        }
        _ => {}
    }
}

/// One eye at (ex, ey).
fn draw_eye(c: &mut Canvas, a: &Avatar, ex: Fx, ey: Fx, s: i32, blink: bool, skin: Color) {
    let u = |k: i32| -> Fx { s * k * ONE / 100 };
    let iris = EYES[a.eye_color as usize % 8];
    if blink {
        c.arc_fx(ex, ey - u(4), u(6), u(2), false, u(3) + 96, INK);
        return;
    }
    match a.eyes {
        0 => { c.fill_ellipse_fx(ex, ey, u(5), u(7), INK); c.fill_ellipse_fx(ex - u(1), ey - u(3), u(2), u(2), with_alpha(WHITE, 220)); }
        1 => {
            c.fill_ellipse_fx(ex, ey, u(9), u(10), WHITE);
            c.fill_ellipse_fx(ex, ey + u(1), u(6), u(7), iris);
            c.fill_ellipse_fx(ex, ey + u(1), u(3), u(4), INK);
            c.fill_ellipse_fx(ex - u(2), ey - u(2), u(2), u(2), WHITE);
        }
        2 => c.arc_fx(ex, ey + u(4), u(7), u(1), true, u(3) + 96, INK),                 // happy ^ ^
        3 => {
            c.fill_ellipse_fx(ex, ey, u(7), u(9), iris);
            c.fill_ellipse_fx(ex, ey + u(1), u(4), u(6), INK);
            c.fill_ellipse_fx(ex - u(2), ey - u(4), u(3), u(3), WHITE);
            c.fill_ellipse_fx(ex + u(3), ey + u(3), u(1) + 64, u(1) + 64, WHITE);
        }
        4 => { // sleepy: half-closed
            c.fill_ellipse_fx(ex, ey + u(2), u(7), u(5), INK);
            c.fill_rect(Rect::new(fx_floor(ex - u(9)), fx_floor(ey - u(6)), fx_ceil(u(18)), fx_ceil(u(7))), skin);
            c.line_fx(ex - u(8), ey + u(1), ex + u(8), ey + u(1), u(3), INK);
        }
        _ => c.fill_round_rect_fx(ex - u(7), ey - u(2), u(14), u(4) + 64, u(2), INK), // calm lines
    }
}

/// Body proportions for each body style: (height %, width %).
const BODIES: [(i32, i32); 6] = [(88, 100), (88, 128), (100, 100), (100, 128), (110, 100), (110, 128)];

/// The rectangle a full-body avatar of height `h` standing at (cx, feet_y) covers.
pub fn body_rect(cx: i32, feet_y: i32, h: i32) -> Rect {
    let hh = h * 112 / 100;
    Rect::new(cx - hh * 45 / 100, feet_y - hh - 8, hh * 90 / 100, hh + 16)
}

/// Draw a whole avatar standing with its feet at (cx, feet_y), `h` pixels tall.
pub fn draw_body(c: &mut Canvas, cx: i32, feet_y: i32, h: i32, a: &Avatar, pose: Pose) {
    if !body_rect(cx, feet_y, h).overlaps(&c.clip) { return; }
    let (hp, wp) = BODIES[a.body as usize % 6];
    let h = h * hp / 100;
    let head = h * 46 / 100;
    let skin = SKIN[a.skin as usize % 8];
    let shirt = SHIRT[a.shirt as usize % 8];
    let pants = hex(0x4A5170);
    let (cxf, fy) = (fx(cx), fx(feet_y));
    let leg_h = h * 22 / 100;
    let torso_h = h * 30 / 100;
    let torso_w = head * 70 / 100 * wp / 100;
    let torso_top = feet_y - leg_h - torso_h;

    // Shadow on the ground.
    c.fill_ellipse_fx(cxf, fy, fx(torso_w * 9 / 10), fx(h / 22 + 2), with_alpha(0x303858, 36));
    // Legs + shoes.
    for side in [-1, 1] {
        let lx = cx + side * torso_w / 4;
        c.fill_round_rect(Rect::new(lx - torso_w / 7, feet_y - leg_h - 4, torso_w * 2 / 7, leg_h), torso_w / 8, pants);
        c.fill_ellipse_fx(fx(lx + side * torso_w / 20), fy - fx(h / 40), fx(torso_w / 6 + 2), fx(h / 34 + 2), hex(0x3A3F58));
    }
    // Arms (thick lines with round hands). A waving arm swings back and forth.
    let shoulder_y = torso_top + torso_h / 6;
    let arm_w = fx(torso_w / 4);
    for side in [-1, 1] {
        let sx = cx + side * torso_w / 2;
        let (hx, hy) = if side == 1 && pose.wave != 0 {
            let ang = 24 + (sin(pose.wave * 10) * 14 >> 8); // swing around "up and out"
            (fx(sx) + sin(ang) * (torso_h * 8 / 10), fx(shoulder_y) - cos(ang) * (torso_h * 8 / 10))
        } else {
            (fx(sx + side * torso_w / 5), fx(shoulder_y + torso_h * 3 / 4))
        };
        c.line_fx(fx(sx), fx(shoulder_y), hx, hy, arm_w, shade(shirt, -12));
        c.fill_ellipse_fx(hx, hy, arm_w * 6 / 10, arm_w * 6 / 10, skin);
    }
    // Torso.
    c.fill_round_rect(Rect::new(cx - torso_w / 2, torso_top, torso_w, torso_h + torso_h / 10), torso_w / 3, shirt);
    c.fill_ellipse_fx(cxf, fx(torso_top + 2), fx(torso_w / 6), fx(torso_h / 12 + 1), shade(skin, -10)); // neck
    // Head on top.
    let head_cy = torso_top - head * 40 / 100;
    draw_head(c, cx, head_cy, head, a, pose);
}

pub const MAX_PALS: usize = 8;

/// All your pals. Pal 0 is "you".
pub struct PalList { pub list: [Avatar; MAX_PALS], pub count: usize, pub current: usize }

impl PalList {
    pub fn new(seed: u64) -> PalList {
        let mut rng = Rng::new(seed);
        let mut list = [Avatar::default_me(); MAX_PALS];
        list[1] = Avatar::random(&mut rng);
        list[2] = Avatar::random(&mut rng);
        PalList { list, count: 3, current: 0 }
    }
    pub fn me(&self) -> &Avatar { &self.list[0] }
    pub fn current(&self) -> &Avatar { &self.list[self.current] }
    pub fn current_mut(&mut self) -> &mut Avatar { &mut self.list[self.current] }
    /// Add a pal and make it current. False if the list is full.
    pub fn add(&mut self, a: Avatar) -> bool {
        if self.count >= MAX_PALS { return false; }
        self.list[self.count] = a;
        self.current = self.count;
        self.count += 1;
        true
    }
}
