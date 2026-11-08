//! The mouse pointer: a small hand-drawn bitmap.
//! '#' = outline, '.' = fill, ' ' = transparent.

use super::canvas::Canvas;
use super::color::{INK, WHITE};
use super::rect::Rect;

const ART: [&str; 21] = [
    "#",
    "##",
    "#.#",
    "#..#",
    "#...#",
    "#....#",
    "#.....#",
    "#......#",
    "#.......#",
    "#........#",
    "#.........#",
    "#..........#",
    "#...........#",
    "#.......#####",
    "#...#...#",
    "#..##...#",
    "#.#  #...#",
    "##   #...#",
    "#     #...#",
    "      #...#",
    "       ###",
];

pub const W: i32 = 13;
pub const H: i32 = 21;

/// Area the cursor covers at (x, y) -- used to mark dirty rectangles.
pub fn rect(x: i32, y: i32) -> Rect { Rect::new(x, y, W, H) }

pub fn draw(c: &mut Canvas, x: i32, y: i32) {
    if !rect(x, y).overlaps(&c.clip) { return; }
    for (row, line) in ART.iter().enumerate() {
        for (col, ch) in line.bytes().enumerate() {
            let (px, py) = (x + col as i32, y + row as i32);
            match ch { b'#' => c.put(px, py, INK), b'.' => c.put(px, py, WHITE), _ => {} }
        }
    }
}
