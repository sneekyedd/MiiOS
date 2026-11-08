//! Milestone 2 demo scene: shows every drawing primitive.
use crate::gfx::color::*;
use crate::gfx::font;
use crate::gfx::math::*;
use crate::gfx::{Canvas, Rect};

pub fn ball_rect(frame: i32) -> Rect {
    let (x, y) = ball_pos(frame);
    Rect::new(fx_floor(x) - 46, fx_floor(y) - 46, 92, 96)
}

fn ball_pos(frame: i32) -> (Fx, Fx) {
    let x = fx(1200) + sin(frame * 2) * 300;
    let y = fx(760) - (sin(frame * 3).abs() * 220);
    (x, y)
}

pub fn draw(c: &mut Canvas, frame: i32) {
    // Background with a soft dot pattern.
    let clip = c.clip;
    c.fill_gradient(Rect::new(0, 0, c.width, c.height), hex(0xFAFBFE), hex(0xEEF1F8));
    let mut y = 24;
    while y < c.height {
        if y + 4 >= clip.y && y - 4 <= clip.bottom() {
            let mut x = 24;
            while x < c.width {
                if x + 4 >= clip.x && x - 4 <= clip.right() { c.fill_circle(x, y, 3, BG_DOT); }
                x += 48;
            }
        }
        y += 48;
    }

    c.text(&font::LARGE, 64, 48, "MiiOS graphics test", INK);
    c.text(&font::MEDIUM, 64, 90, "Rounded tiles, shadows, circles, arcs, lines, three font sizes.", INK_SOFT);
    c.text(&font::SMALL, 64, 120, "Small 8x16 font: The quick brown fox jumps over the lazy dog 0123456789", INK_SOFT);

    // A row of pastel tiles.
    for i in 0..6 {
        let r = Rect::new(64 + i * 300, 180, 260, 200);
        c.shadow(r, 36, 12, 70);
        c.fill_round_rect(r, 36, WHITE);
        c.fill_round_rect(r.inflate(-14), 26, PASTELS[i as usize]);
        c.text_centered(&font::MEDIUM, r.x + r.w / 2, r.bottom() - 52, ["Home", "Pals", "Write", "Files", "Clock", "Settings"][i as usize], INK);
    }
    // Selection ring around the first tile.
    c.stroke_round_rect(Rect::new(64 - 10, 180 - 10, 280, 220), 44, 6, ACCENT);

    // A little face made of shapes (preview of the avatar system).
    let (fx0, fy0) = (fx(300), fx(620));
    c.fill_ellipse_fx(fx0, fy0, fx(110), fx(130), hex(0xFFDDBE));
    c.fill_ellipse_fx(fx0 - fx(40), fy0 - fx(20), fx(12), fx(18), INK);
    c.fill_ellipse_fx(fx0 + fx(40), fy0 - fx(20), fx(12), fx(18), INK);
    c.fill_ellipse_fx(fx0 - fx(70), fy0 + fx(25), fx(20), fx(12), with_alpha(hex(0xFF8FA3), 110));
    c.fill_ellipse_fx(fx0 + fx(70), fy0 + fx(25), fx(20), fx(12), with_alpha(hex(0xFF8FA3), 110));
    c.arc_fx(fx0, fy0 + fx(10), fx(45), fx(25), false, fx(7), INK);

    // Lines of different widths.
    for i in 0..6 {
        c.line(560, 520 + i * 40, 900, 560 + i * 30, 1 + i * 2, PASTELS[(i + 2) as usize % 8] & 0xFF7F7FFF | 0xFF000000);
    }

    // The bouncing ball (redrawn every frame using dirty rectangles).
    let (bx, by) = ball_pos(frame);
    c.fill_ellipse_fx(bx, by + fx(4), fx(38), fx(38), with_alpha(0x303858, 40));
    c.fill_ellipse_fx(bx, by, fx(36), fx(36), ACCENT);
    c.fill_ellipse_fx(bx - fx(10), by - fx(12), fx(10), fx(8), with_alpha(WHITE, 160));
}
