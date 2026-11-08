//! "Write": a simple, fast text editor -- turns the laptop into a typing machine.
//!
//! The document is a list of lines (`Vec<Vec<u8>>`). Typing inserts into one
//! line, which is quick for any normal line length. For display we *wrap*
//! long lines into screen rows at word boundaries and keep a table of rows
//! (`rows`), rebuilt after each edit. Only rows whose content changed are
//! redrawn, so a keystroke costs a single text row on screen.
//!
//! Keys: arrows, Home/End, PgUp/PgDn, Ctrl+Home/End, Ctrl+Left/Right (by
//! word), Shift+movement selects, Ctrl+A/C/X/V, Ctrl+Z undo, Tab = 4 spaces,
//! Ctrl+S saves (once storage exists), Esc = back to home.

extern crate alloc;
use super::widgets;
use super::{Action, App, Shared, UiEvent};
use crate::gfx::color::*;
use crate::gfx::screen::Damage;
use crate::gfx::{font, Canvas, Color, Rect};
use crate::input::{Key, Mods};
use crate::util::FmtBuf;
use alloc::vec::Vec;
use core::fmt::Write as _;

const LH: i32 = 38; // line height in pixels
const FW: i32 = 16; // glyph width (LARGE font)
const UNDO_MAX: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos { pub line: usize, pub col: usize }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Row { line: usize, start: usize, end: usize }

/// A saved copy of the document for undo.
struct Snapshot { lines: Vec<Vec<u8>>, cursor: Pos }

pub struct Write {
    pub lines: Vec<Vec<u8>>,
    pub name: FmtBuf<32>,
    cursor: Pos,
    anchor: Option<Pos>,   // other end of the selection
    want_x: usize,         // column we try to keep when moving up/down
    rows: Vec<Row>,
    prev_rows: Vec<Row>,
    top: usize,            // first visible row
    cols: usize,           // characters per row
    clipboard: Vec<u8>,
    undo: Vec<Snapshot>,
    last_edit_kind: u8,    // groups typing into one undo step
    blink_on: bool,
    blink_t: u32,
    dragging: bool,
    hover_back: bool,
    words: usize,
    chars: usize,
    pub modified: bool,
    pub status: FmtBuf<64>,
    page: Rect,
    w: i32,
    h: i32,
}

impl Write {
    pub fn new(w: i32, h: i32) -> Write {
        let mut e = Write {
            lines: alloc::vec![Vec::new()], name: FmtBuf::new(), cursor: Pos::default(), anchor: None, want_x: 0,
            rows: Vec::with_capacity(1024), prev_rows: Vec::with_capacity(1024), top: 0, cols: 80,
            clipboard: Vec::new(), undo: Vec::new(), last_edit_kind: 0, blink_on: true, blink_t: 0,
            dragging: false, hover_back: false, words: 0, chars: 0, modified: false, status: FmtBuf::new(),
            page: Rect::default(), w, h,
        };
        let _ = e.name.write_str("notes.txt");
        e.set_text(b"Welcome to Write!\n\nThis is a tiny, fast text editor. Just start typing.\n\nShift+arrows select, Ctrl+C / Ctrl+X / Ctrl+V copy, cut and paste, Ctrl+Z undoes. Long lines wrap at word boundaries automatically, so you can write whole paragraphs without pressing Enter.\n");
        e.cursor = Pos { line: e.lines.len() - 1, col: 0 };
        e.modified = false;
        e.layout_page();
        e
    }

    pub fn set_text(&mut self, t: &[u8]) {
        self.lines.clear();
        for l in t.split(|&b| b == b'\n') { self.lines.push(l.iter().copied().filter(|&b| b >= 32 && b < 127).collect()); }
        if self.lines.is_empty() { self.lines.push(Vec::new()); }
        self.cursor = Pos::default();
        self.anchor = None;
        self.top = 0;
        self.undo.clear();
        self.modified = false;
        self.relayout();
        self.count();
    }

    /// The whole document as bytes with '\n' between lines.
    pub fn text(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(self.chars + self.lines.len());
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 { v.push(b'\n'); }
            v.extend_from_slice(l);
        }
        v
    }

    fn layout_page(&mut self) {
        let max_w = (self.w - 160).min(1500);
        self.cols = ((max_w - 96) / FW) as usize;
        let pw = self.cols as i32 * FW + 96;
        self.page = Rect::new((self.w - pw) / 2, 124, pw, self.h - 124 - 64);
        self.relayout();
    }

    fn text_x(&self) -> i32 { self.page.x + 48 }
    fn text_y(&self) -> i32 { self.page.y + 28 }
    fn visible_rows(&self) -> usize { ((self.page.h - 56) / LH).max(1) as usize }

    /// Wrap every line into rows of at most `cols` characters, breaking after spaces.
    fn relayout(&mut self) {
        core::mem::swap(&mut self.rows, &mut self.prev_rows);
        self.rows.clear();
        let cols = self.cols.max(8);
        for (li, l) in self.lines.iter().enumerate() {
            if l.is_empty() { self.rows.push(Row { line: li, start: 0, end: 0 }); continue; }
            let mut start = 0;
            while start < l.len() {
                let mut end = (start + cols).min(l.len());
                if end < l.len() {
                    if let Some(p) = l[start..end].iter().rposition(|&b| b == b' ') { if p > 0 { end = start + p + 1; } }
                }
                self.rows.push(Row { line: li, start, end });
                start = end;
            }
        }
    }

    fn row_of(&self, p: Pos) -> usize {
        let len = self.lines[p.line].len();
        // Binary search for the first row of this line, then walk.
        let mut i = self.rows.partition_point(|r| r.line < p.line);
        while i + 1 < self.rows.len() && self.rows[i + 1].line == p.line && self.rows[i].end <= p.col && !(p.col == len && self.rows[i].end == len) {
            i += 1;
        }
        i
    }

    fn row_rect(&self, vis: usize) -> Rect { Rect::new(self.page.x + 8, self.text_y() + vis as i32 * LH - 3, self.page.w - 16, LH) }

    fn damage_row(&self, row: usize, d: &mut Damage) {
        if row >= self.top && row < self.top + self.visible_rows() { d.add(self.row_rect(row - self.top)); }
    }

    fn status_rect(&self) -> Rect { Rect::new(0, self.page.bottom() + 4, self.w, self.h - self.page.bottom() - 4) }
    fn text_area(&self) -> Rect { self.page.inflate(-4) }

    fn count(&mut self) {
        self.chars = self.lines.iter().map(|l| l.len()).sum();
        self.words = self.lines.iter().map(|l| l.split(|&b| b == b' ').filter(|w| !w.is_empty()).count()).sum();
    }

    fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cursor { return None; }
        Some(if a < self.cursor { (a, self.cursor) } else { (self.cursor, a) })
    }

    fn save_undo(&mut self, kind: u8) {
        // Consecutive typing (kind 1) is grouped into one undo step per word.
        if kind == 1 && self.last_edit_kind == 1 { return; }
        self.last_edit_kind = kind;
        if self.undo.len() >= UNDO_MAX { self.undo.remove(0); }
        self.undo.push(Snapshot { lines: self.lines.clone(), cursor: self.cursor });
    }

    fn delete_range(&mut self, a: Pos, b: Pos) {
        if a.line == b.line {
            self.lines[a.line].drain(a.col..b.col);
        } else {
            let tail: Vec<u8> = self.lines[b.line][b.col..].to_vec();
            self.lines[a.line].truncate(a.col);
            self.lines[a.line].extend_from_slice(&tail);
            self.lines.drain(a.line + 1..=b.line);
        }
        self.cursor = a;
        self.anchor = None;
    }

    fn delete_selection(&mut self) -> bool {
        if let Some((a, b)) = self.selection() { self.delete_range(a, b); true } else { self.anchor = None; false }
    }

    fn insert_bytes(&mut self, t: &[u8]) {
        for &ch in t {
            if ch == b'\n' {
                let tail = self.lines[self.cursor.line].split_off(self.cursor.col);
                self.cursor.line += 1;
                self.cursor.col = 0;
                self.lines.insert(self.cursor.line, tail);
            } else if (32..127).contains(&ch) {
                self.lines[self.cursor.line].insert(self.cursor.col, ch);
                self.cursor.col += 1;
            }
        }
    }

    fn selected_bytes(&self) -> Vec<u8> {
        let Some((a, b)) = self.selection() else { return Vec::new() };
        let mut v = Vec::new();
        for li in a.line..=b.line {
            let l = &self.lines[li];
            let s = if li == a.line { a.col } else { 0 };
            let e = if li == b.line { b.col } else { l.len() };
            v.extend_from_slice(&l[s..e]);
            if li != b.line { v.push(b'\n'); }
        }
        v
    }

    /// After an edit: re-wrap, then damage exactly the rows that changed.
    fn after_edit(&mut self, d: &mut Damage) {
        if !self.modified { d.add(Rect::new(0, 0, self.w, 110)); } // title gets a "*"
        self.modified = true;
        self.relayout();
        self.count();
        let vis = self.visible_rows();
        let scrolled = self.scroll_to_cursor();
        if scrolled { d.add(self.text_area()); }
        else {
            for v in 0..vis {
                let r = self.top + v;
                if self.rows.get(r) != self.prev_rows.get(r) || self.rows.get(r).map_or(false, |x| x.line == self.cursor.line) {
                    d.add(self.row_rect(v));
                }
            }
        }
        d.add(self.status_rect());
        self.blink_on = true;
        self.blink_t = 0;
    }

    /// Scroll so the cursor is visible. Returns true if we scrolled.
    fn scroll_to_cursor(&mut self) -> bool {
        let r = self.row_of(self.cursor);
        let vis = self.visible_rows();
        let old = self.top;
        if r < self.top { self.top = r; }
        if r >= self.top + vis { self.top = r + 1 - vis; }
        self.top != old
    }

    fn move_cursor(&mut self, to: Pos, select: bool, keep_x: bool, d: &mut Damage) {
        let old_row = self.row_of(self.cursor);
        if let Some((a, b)) = self.selection() { self.damage_rows_between(a, b, d); }
        if select { if self.anchor.is_none() { self.anchor = Some(self.cursor); } } else { self.anchor = None; }
        self.cursor = to;
        if !keep_x { self.want_x = to.col - self.rows[self.row_of(to)].start; }
        if self.scroll_to_cursor() { d.add(self.text_area()); }
        self.damage_row(old_row, d);
        self.damage_row(self.row_of(self.cursor), d);
        if let Some((a, b)) = self.selection() { self.damage_rows_between(a, b, d); }
        d.add(self.status_rect());
        self.blink_on = true;
        self.blink_t = 0;
        self.last_edit_kind = 0;
    }

    fn damage_rows_between(&self, a: Pos, b: Pos, d: &mut Damage) {
        let (ra, rb) = (self.row_of(a), self.row_of(b));
        for r in ra..=rb { self.damage_row(r, d); }
    }

    fn pos_in_row(&self, row: usize, x: usize) -> Pos {
        let r = self.rows[row];
        let len = self.lines[r.line].len();
        let last_row_of_line = r.end == len;
        let max = if last_row_of_line { r.end } else { r.end.saturating_sub(1).max(r.start) };
        Pos { line: r.line, col: (r.start + x).min(max) }
    }

    fn pos_at(&self, x: i32, y: i32) -> Pos {
        let vr = ((y - self.text_y()).max(0) / LH) as usize;
        let row = (self.top + vr).min(self.rows.len() - 1);
        let col = ((x - self.text_x() + FW / 2).max(0) / FW) as usize;
        self.pos_in_row(row, col)
    }

    fn word_left(&self, p: Pos) -> Pos {
        if p.col == 0 { return if p.line > 0 { Pos { line: p.line - 1, col: self.lines[p.line - 1].len() } } else { p }; }
        let l = &self.lines[p.line];
        let mut c = p.col;
        while c > 0 && l[c - 1] == b' ' { c -= 1; }
        while c > 0 && l[c - 1] != b' ' { c -= 1; }
        Pos { line: p.line, col: c }
    }

    fn word_right(&self, p: Pos) -> Pos {
        let l = &self.lines[p.line];
        if p.col >= l.len() { return if p.line + 1 < self.lines.len() { Pos { line: p.line + 1, col: 0 } } else { p }; }
        let mut c = p.col;
        while c < l.len() && l[c] != b' ' { c += 1; }
        while c < l.len() && l[c] == b' ' { c += 1; }
        Pos { line: p.line, col: c }
    }

    /// Save to disk (if there is MiiOS storage).
    fn save(&mut self, d: &mut Damage) {
        self.status.clear();
        if !crate::fs::mounted() {
            let _ = write!(self.status, "No storage yet: set up a disk in Files");
        } else {
            match crate::fs::write(self.name.as_str(), &self.text()) {
                Ok(()) => { self.modified = false; d.add(Rect::new(0, 0, self.w, 110)); let _ = write!(self.status, "Saved {}", self.name.as_str()); }
                Err(e) => { let _ = write!(self.status, "Not saved: {}", e); }
            }
        }
        d.add(self.status_rect());
    }

    fn key(&mut self, key: Key, m: Mods, d: &mut Damage) -> Action {
        // Wherever the caret is drawn now must be redrawn afterwards, even if
        // that row's text doesn't change (e.g. pressing Enter at the end of a line).
        let old_vis = self.row_of(self.cursor) as isize - self.top as isize;
        let top_before = self.top;
        let a = self.key_inner(key, m, d);
        if old_vis >= 0 && (old_vis as usize) < self.visible_rows() && self.top == top_before {
            d.add(self.row_rect(old_vis as usize));
        }
        a
    }

    fn key_inner(&mut self, key: Key, m: Mods, d: &mut Damage) -> Action {
        let sel = m.shift;
        let cur = self.cursor;
        match key {
            Key::Escape => { if self.modified { self.save(d); } return Action::Back; }
            Key::Char(c) if m.ctrl => match c.to_ascii_lowercase() {
                'a' => {
                    self.anchor = Some(Pos::default());
                    let last = self.lines.len() - 1;
                    self.cursor = Pos { line: last, col: self.lines[last].len() };
                    d.add(self.text_area());
                }
                'c' => { self.clipboard = self.selected_bytes(); }
                'x' => {
                    if self.selection().is_some() {
                        self.save_undo(2);
                        self.clipboard = self.selected_bytes();
                        self.delete_selection();
                        self.after_edit(d);
                    }
                }
                'v' => {
                    if !self.clipboard.is_empty() {
                        self.save_undo(2);
                        self.delete_selection();
                        let clip = core::mem::take(&mut self.clipboard);
                        self.insert_bytes(&clip);
                        self.clipboard = clip;
                        self.after_edit(d);
                        d.add(self.text_area());
                    }
                }
                'z' => {
                    if let Some(s) = self.undo.pop() {
                        self.lines = s.lines;
                        self.cursor = s.cursor;
                        self.anchor = None;
                        self.last_edit_kind = 0;
                        self.after_edit(d);
                        d.add(self.text_area());
                    }
                }
                's' => { self.save(d); }
                _ => {}
            },
            Key::Char(c) => {
                if c == ' ' { self.last_edit_kind = 0; }
                self.save_undo(1);
                self.delete_selection();
                self.insert_bytes(&[c as u8]);
                self.after_edit(d);
            }
            Key::Tab => { self.save_undo(2); self.delete_selection(); self.insert_bytes(b"    "); self.after_edit(d); }
            Key::Enter => { self.save_undo(2); self.delete_selection(); self.insert_bytes(b"\n"); self.after_edit(d); }
            Key::Backspace => {
                self.save_undo(3);
                if !self.delete_selection() {
                    if cur.col > 0 {
                        let to = if m.ctrl { self.word_left(cur) } else { Pos { line: cur.line, col: cur.col - 1 } };
                        self.delete_range(to, cur);
                    } else if cur.line > 0 {
                        let prev = Pos { line: cur.line - 1, col: self.lines[cur.line - 1].len() };
                        self.delete_range(prev, cur);
                    }
                }
                self.after_edit(d);
            }
            Key::Delete => {
                self.save_undo(3);
                if !self.delete_selection() {
                    let len = self.lines[cur.line].len();
                    if cur.col < len { self.delete_range(cur, Pos { line: cur.line, col: cur.col + 1 }); }
                    else if cur.line + 1 < self.lines.len() { self.delete_range(cur, Pos { line: cur.line + 1, col: 0 }); }
                }
                self.after_edit(d);
            }
            Key::Left => {
                let to = if m.ctrl { self.word_left(cur) }
                    else if !sel && self.selection().is_some() { self.selection().unwrap().0 }
                    else if cur.col > 0 { Pos { line: cur.line, col: cur.col - 1 } }
                    else if cur.line > 0 { Pos { line: cur.line - 1, col: self.lines[cur.line - 1].len() } } else { cur };
                self.move_cursor(to, sel, false, d);
            }
            Key::Right => {
                let len = self.lines[cur.line].len();
                let to = if m.ctrl { self.word_right(cur) }
                    else if !sel && self.selection().is_some() { self.selection().unwrap().1 }
                    else if cur.col < len { Pos { line: cur.line, col: cur.col + 1 } }
                    else if cur.line + 1 < self.lines.len() { Pos { line: cur.line + 1, col: 0 } } else { cur };
                self.move_cursor(to, sel, false, d);
            }
            Key::Up | Key::Down | Key::PageUp | Key::PageDown => {
                let r = self.row_of(cur) as isize;
                let step = match key { Key::Up => -1, Key::Down => 1, Key::PageUp => -(self.visible_rows() as isize - 1), _ => self.visible_rows() as isize - 1 };
                let nr = (r + step).clamp(0, self.rows.len() as isize - 1) as usize;
                let to = self.pos_in_row(nr, self.want_x);
                self.move_cursor(to, sel, true, d);
            }
            Key::Home => {
                let to = if m.ctrl { Pos::default() } else { Pos { line: cur.line, col: self.rows[self.row_of(cur)].start } };
                self.move_cursor(to, sel, false, d);
            }
            Key::End => {
                let to = if m.ctrl { let l = self.lines.len() - 1; Pos { line: l, col: self.lines[l].len() } }
                    else { let r = self.row_of(cur); self.pos_in_row(r, usize::MAX / 2) };
                self.move_cursor(to, sel, false, d);
            }
            _ => {}
        }
        Action::None
    }
}

impl App for Write {
    fn enter(&mut self, s: &mut Shared) {
        self.w = s.w; self.h = s.h;
        // Opened from Files: load that document (or start a new, empty one).
        if let Some(name) = s.open_doc.take() {
            let data = crate::fs::read(&name).unwrap_or_default();
            self.set_text(&data);
            self.name.clear();
            let _ = self.name.write_str(&name);
            self.status.clear();
        }
        self.layout_page();
        self.scroll_to_cursor();
    }

    fn event(&mut self, e: &UiEvent, _s: &mut Shared, d: &mut Damage) -> Action {
        match *e {
            UiEvent::Key { key, mods } => return self.key(key, mods, d),
            UiEvent::Click { x, y } => {
                if widgets::back_button().contains(x, y) { if self.modified { self.save(d); } return Action::Back; }
                if self.page.contains(x, y) {
                    let p = self.pos_at(x, y);
                    self.move_cursor(p, false, false, d);
                    self.anchor = Some(p);
                    self.dragging = true;
                }
            }
            UiEvent::Release { .. } => { self.dragging = false; }
            UiEvent::PointerMove { x, y } => {
                let hb = widgets::back_button().contains(x, y);
                if hb != self.hover_back { self.hover_back = hb; d.add(widgets::back_button().inflate(12)); }
                if self.dragging {
                    let p = self.pos_at(x, y);
                    if p != self.cursor {
                        let a = self.anchor;
                        self.move_cursor(p, true, false, d);
                        if a.is_some() { self.anchor = a; }
                        d.add(self.text_area());
                    }
                }
            }
            UiEvent::Scroll { dy } => {
                let max_top = self.rows.len().saturating_sub(self.visible_rows());
                let nt = (self.top as isize + dy as isize).clamp(0, max_top as isize) as usize;
                if nt != self.top { self.top = nt; d.add(self.text_area()); }
            }
        }
        Action::None
    }

    fn tick(&mut self, _s: &mut Shared, d: &mut Damage) -> bool {
        // Blink the caret about twice a second.
        self.blink_t += 1;
        if self.blink_t >= 32 {
            self.blink_t = 0;
            self.blink_on = !self.blink_on;
            self.damage_row(self.row_of(self.cursor), d);
        }
        true
    }

    fn draw(&self, c: &mut Canvas, _s: &Shared) {
        widgets::app_background(c, self.color());
        widgets::header(c, "Write", self.color(), self.hover_back);
        let mut title = FmtBuf::<48>::new();
        let _ = write!(title, "{}{}", self.name.as_str(), if self.modified { " *" } else { "" });
        c.text(&font::MEDIUM, 166 + 6 * 16 + 20, 50, title.as_str(), INK_SOFT);

        // The page.
        let p = self.page;
        if p.inflate(16).overlaps(&c.clip) {
            c.shadow(p, 24, 10, 50);
            c.fill_round_rect(p, 24, WHITE);
        }
        let sel = self.selection();
        let vis = self.visible_rows();
        let (tx, ty) = (self.text_x(), self.text_y());
        for v in 0..vis {
            let ri = self.top + v;
            let Some(&row) = self.rows.get(ri) else { break };
            let rr = self.row_rect(v);
            if !rr.overlaps(&c.clip) { continue; }
            let y = ty + v as i32 * LH;
            // Selection highlight behind the text.
            if let Some((a, b)) = sel {
                let rs = Pos { line: row.line, col: row.start };
                let re = Pos { line: row.line, col: row.end };
                if b > rs && a <= re {
                    let s0 = if a > rs { a.col } else { row.start };
                    let mut s1 = if b < re { b.col } else { row.end };
                    let line_break = b.line > row.line && row.end == self.lines[row.line].len();
                    if line_break { s1 += 1; } // show the selected line break as one cell
                    let x0 = tx + (s0 - row.start) as i32 * FW;
                    let wsel = (s1 - s0) as i32 * FW;
                    c.fill_rect(Rect::new(x0, y - 2, wsel, LH - 2), hex(0xCDEBFF));
                }
            }
            let l = &self.lines[row.line][row.start..row.end];
            c.text(&font::LARGE, tx, y, core::str::from_utf8(l).unwrap_or(""), INK);
        }
        // Caret.
        let cr = self.row_of(self.cursor);
        if self.blink_on && cr >= self.top && cr < self.top + vis {
            let x = tx + (self.cursor.col - self.rows[cr].start) as i32 * FW;
            let y = ty + (cr - self.top) as i32 * LH;
            c.fill_round_rect(Rect::new(x - 1, y - 2, 3, LH - 2), 1, ACCENT_DARK);
        }
        // Scrollbar.
        if self.rows.len() > vis && p.overlaps(&c.clip) {
            let track = Rect::new(p.right() - 18, p.y + 24, 6, p.h - 48);
            let th = (track.h as usize * vis / self.rows.len()).max(30) as i32;
            let ty2 = track.y + ((track.h - th) as usize * self.top / (self.rows.len() - vis).max(1)) as i32;
            c.fill_round_rect(Rect::new(track.x, ty2, track.w, th), 3, hex(0xD5DAE6));
        }
        // Status line.
        let sr = self.status_rect();
        if sr.overlaps(&c.clip) {
            let mut st = FmtBuf::<128>::new();
            let _ = write!(st, "Line {}, Col {}   |   {} words   |   {} characters", self.cursor.line + 1, self.cursor.col + 1, self.words, self.chars);
            c.text(&font::MEDIUM, p.x + 8, sr.y + 18, st.as_str(), INK_SOFT);
            let msg = self.status.as_str();
            let mw = font::MEDIUM.text_width(msg) as i32;
            c.text(&font::MEDIUM, p.right() - mw - 8, sr.y + 18, msg, INK_SOFT);
        }
    }

    fn color(&self) -> Color { PASTELS[2] }
}
