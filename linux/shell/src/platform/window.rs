//! The window, talking to the Wayland display (Sway) directly.
//!
//! Why not a simple window library? Those hand the display a *whole* new
//! screen image for every frame (8 MB at 1080p, 60 times a second), which
//! is what made animations stutter. Here we:
//!   * keep two shared-memory buffers (one the display shows, one we update),
//!     copying in only the rectangles that changed;
//!   * tell the display exactly which rectangles changed ("damage"), so it
//!     only re-uploads those to the graphics chip;
//!   * draw at most one frame per screen refresh (the display sends a
//!     "frame callback" when it's ready for the next one);
//!   * get keyboard and mouse input as events (nothing can be missed).

use crate::gfx::Rect;
use crate::input::{Event, Key, Mods, EVENTS};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{window::{Window, WindowConfigure, WindowDecorations, WindowHandler}, XdgShell},
        WaylandSurface,
    },
    shm::{slot::{Buffer, SlotPool}, Shm, ShmHandler},
};
use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static mut FRONT: Vec<u32> = Vec::new();
/// Rectangles changed since the last time we showed a frame.
static NEW_DAMAGE: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

struct Win {
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    shm: Shm,
    window: Window,
    pool: SlotPool,
    bufs: Vec<(Buffer, Vec<Rect>)>, // each buffer + what it's still missing
    w: i32,
    h: i32,
    configured: bool,
    frame_pending: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// Keyboards that went away: released after event handling (doing it
    /// during event handling would crash inside the event loop).
    gone: Vec<wl_keyboard::WlKeyboard>,
    /// The key being held down, and when it next repeats.
    held: Option<(u32, KeyEvent, Instant)>,
    pointer: Option<wl_pointer::WlPointer>,
    mods: Mods,
    ptr: (i32, i32),
    buttons: (bool, bool, bool),
    loop_handle: LoopHandle<'static, Win>,
}

thread_local! {
    static STATE: RefCell<Option<(EventLoop<'static, Win>, Win)>> = const { RefCell::new(None) };
}

pub fn open(w: i32, h: i32) {
    let conn = Connection::connect_to_env().expect("could not connect to the display (is Sway running?)");
    let (globals, queue) = registry_queue_init(&conn).expect("wayland registry");
    let qh = queue.handle();
    QH.with(|q| *q.borrow_mut() = Some(qh.clone()));
    let event_loop: EventLoop<Win> = EventLoop::try_new().expect("event loop");
    WaylandSource::new(conn.clone(), queue).insert(event_loop.handle()).expect("wayland source");
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg = XdgShell::bind(&globals, &qh).expect("xdg shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let window = xdg.create_window(compositor.create_surface(&qh), WindowDecorations::None, &qh);
    window.set_title("MiiOS");
    window.set_app_id("miios");
    window.commit();
    let mut pool = SlotPool::new((w * h * 4 * 2) as usize, &shm).expect("shm pool");
    let mut bufs = Vec::new();
    for _ in 0..2 {
        let (b, _) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Xrgb8888).expect("buffer");
        bufs.push((b, vec![Rect::new(0, 0, w, h)]));
    }
    #[allow(static_mut_refs)]
    unsafe { FRONT = vec![0; (w * h) as usize]; }
    let win = Win {
        registry: RegistryState::new(&globals), seat: SeatState::new(&globals, &qh), output: OutputState::new(&globals, &qh),
        shm, window, pool, bufs, w, h, configured: false, frame_pending: false, keyboard: None, gone: Vec::new(), held: None, pointer: None,
        mods: Mods::default(), ptr: (w / 2, h / 2), buttons: (false, false, false), loop_handle: event_loop.handle(),
    };
    STATE.with(|s| *s.borrow_mut() = Some((event_loop, win)));
    // Wait for the first "configure" (the display telling us we may draw).
    for _ in 0..200 {
        if STATE.with(|s| s.borrow().as_ref().map(|(_, w)| w.configured).unwrap_or(true)) { break; }
        dispatch(Duration::from_millis(10));
    }
}

/// The buffer the UI copies finished frames into.
pub fn front() -> *mut u32 {
    #[allow(static_mut_refs)]
    unsafe { FRONT.as_mut_ptr() }
}

/// Called by the screen code after it copied these rectangles into `front()`.
pub fn damaged(rects: &[Rect]) { NEW_DAMAGE.lock().unwrap().extend_from_slice(rects); }

fn dispatch(t: Duration) {
    STATE.with(|s| {
        let mut b = s.borrow_mut();
        if let Some((el, win)) = b.as_mut() {
            let _ = el.dispatch(Some(t), win);
            for k in win.gone.drain(..) { k.release(); }
        }
    });
}

impl Win {
    /// Show the new frame: copy the changed rectangles into a free buffer and
    /// tell the display only those changed.
    fn present(&mut self, qh: &QueueHandle<Win>) {
        if !self.configured || self.frame_pending { return; }
        let new: Vec<Rect> = std::mem::take(&mut *NEW_DAMAGE.lock().unwrap());
        if new.is_empty() { return; }
        let full = Rect::new(0, 0, self.w, self.h);
        // Which buffer can we write to? (The display may still be reading the other one.)
        let Some(k) = (0..self.bufs.len()).find(|&k| self.pool.canvas(&self.bufs[k].0).is_some()) else {
            NEW_DAMAGE.lock().unwrap().extend(new); // try again next time
            return;
        };
        for (j, (_, missing)) in self.bufs.iter_mut().enumerate() {
            missing.extend(new.iter().copied());
            if missing.len() > 64 { missing.clear(); missing.push(full); }
            let _ = j;
        }
        let todo = std::mem::take(&mut self.bufs[k].1);
        let (w, stride) = (self.w as usize, self.w as usize);
        #[allow(static_mut_refs)]
        let front = unsafe { &FRONT };
        let canvas = self.pool.canvas(&self.bufs[k].0).unwrap();
        for r in &todo {
            let r = r.intersect(&full);
            for y in r.y..r.bottom() {
                let src = &front[y as usize * w + r.x as usize..y as usize * w + r.right() as usize];
                let dst = &mut canvas[(y as usize * stride + r.x as usize) * 4..(y as usize * stride + r.right() as usize) * 4];
                for (d, s) in dst.chunks_exact_mut(4).zip(src) { d.copy_from_slice(&s.to_le_bytes()); }
            }
        }
        let surf = self.window.wl_surface();
        for r in &new { let r = r.intersect(&full); surf.damage_buffer(r.x, r.y, r.w, r.h); }
        surf.frame(qh, surf.clone());
        self.bufs[k].0.attach_to(surf).expect("attach");
        self.window.commit();
        self.frame_pending = true;
    }
}

/// Show the latest frame, then wait up to `ms` for input (or the next frame slot).
pub fn present_and_wait(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    STATE.with(|s| {
        let mut b = s.borrow_mut();
        if let Some((el, win)) = b.as_mut() {
            let qh = win_qh(el);
            win.present(&qh);
            let _ = el.dispatch(Some(Duration::ZERO), win);
            for k in win.gone.drain(..) { k.release(); }
        }
    });
    while !crate::input::pending() {
        let now = Instant::now();
        if now >= end { break; }
        let rep = STATE.with(|s| s.borrow_mut().as_mut().and_then(|(_, win)| repeat_keys(win)));
        if crate::input::pending() { break; }
        dispatch(rep.map_or(end, |r| r.min(end)).saturating_duration_since(now));
        // A frame slot opened up while we waited and something changed? Show it.
        STATE.with(|s| {
            let mut b = s.borrow_mut();
            if let Some((el, win)) = b.as_mut() { let qh = win_qh(el); if !win.frame_pending { win.present(&qh); } }
        });
    }
}

thread_local! { static QH: RefCell<Option<QueueHandle<Win>>> = const { RefCell::new(None) }; }
fn win_qh(_el: &EventLoop<'static, Win>) -> QueueHandle<Win> { QH.with(|q| q.borrow().clone().expect("queue handle")) }

/// Our key events from a Wayland key (by symbol), plus typed text.
const REPEAT_DELAY: Duration = Duration::from_millis(500);
const REPEAT_EVERY: Duration = Duration::from_millis(40);

/// A held key repeats (25 times a second, after half a second).
/// Returns when the next repeat is due.
fn repeat_keys(win: &mut Win) -> Option<Instant> {
    let mods = win.mods;
    let (_, ev, next) = win.held.as_mut()?;
    let now = Instant::now();
    while *next <= now { key_event(mods, ev); *next += REPEAT_EVERY; }
    Some(*next)
}

fn key_event(mods: Mods, ev: &KeyEvent) {
    let k = match ev.keysym {
        Keysym::Return | Keysym::KP_Enter => Some(Key::Enter), Keysym::BackSpace => Some(Key::Backspace),
        Keysym::Tab | Keysym::ISO_Left_Tab => Some(Key::Tab), Keysym::Escape => Some(Key::Escape), Keysym::Delete => Some(Key::Delete),
        Keysym::Up => Some(Key::Up), Keysym::Down => Some(Key::Down), Keysym::Left => Some(Key::Left), Keysym::Right => Some(Key::Right),
        Keysym::Home => Some(Key::Home), Keysym::End => Some(Key::End), Keysym::Page_Up => Some(Key::PageUp), Keysym::Page_Down => Some(Key::PageDown),
        _ => None,
    };
    let raw = ev.keysym.raw();
    let fkey = if (Keysym::F1.raw()..=Keysym::F12.raw()).contains(&raw) { Some(Key::F((raw - Keysym::F1.raw() + 1) as u8)) } else { None };
    if let Some(key) = k.or(fkey) { EVENTS.push(Event::Key { key, pressed: true, mods }); return; }
    if mods.ctrl {
        // Ctrl+letter shortcuts: use the key's own letter (the "text" would be a control code).
        if let Some(c) = ev.keysym.key_char().filter(|c| c.is_ascii_alphabetic()) {
            EVENTS.push(Event::Key { key: Key::Char(c.to_ascii_lowercase()), pressed: true, mods });
        }
        return;
    }
    if let Some(t) = &ev.utf8 {
        for c in t.chars().filter(|c| (' '..='~').contains(c)) { EVENTS.push(Event::Key { key: Key::Char(c), pressed: true, mods }); }
    }
}

impl CompositorHandler for Win {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.present(qh); // anything new waiting? show it right away
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}
impl OutputHandler for Win {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl WindowHandler for Win {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {}
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &Window, _c: WindowConfigure, _: u32) {
        if !self.configured {
            self.configured = true;
            NEW_DAMAGE.lock().unwrap().push(Rect::new(0, 0, self.w, self.h));
        }
        self.present(qh);
    }
}
impl SeatHandler for Win {
    fn seat_state(&mut self) -> &mut SeatState { &mut self.seat }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            // Key repeat is done by us (see `repeat_keys`): the toolkit's own
            // repeat timer crashes when a keyboard is unplugged.
            self.keyboard = self.seat.get_keyboard(qh, &seat, None).ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard { if let Some(k) = self.keyboard.take() { self.gone.push(k); } }
        if cap == Capability::Pointer { if let Some(p) = self.pointer.take() { p.release(); } }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}
impl KeyboardHandler for Win {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        super::audio::set_focused(true);
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {
        super::audio::set_focused(false); // another app is in front: fade the music out
        self.held = None;
    }
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        key_event(self.mods, &ev);
        self.held = Some((ev.raw_code, ev, Instant::now() + REPEAT_DELAY));
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, ev: KeyEvent) {
        if self.held.as_ref().map_or(false, |h| h.0 == ev.raw_code) { self.held = None; }
    }
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: u32) {
        self.mods = Mods { shift: m.shift, ctrl: m.ctrl, alt: m.alt };
    }
}
impl PointerHandler for Win {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, p: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let mut wheel = 0;
        for e in events {
            self.ptr = (e.position.0 as i32, e.position.1 as i32);
            match e.kind {
                PointerEventKind::Enter { serial } => p.set_cursor(serial, None, 0, 0), // MiiOS draws its own pointer
                PointerEventKind::Press { button, .. } | PointerEventKind::Release { button, .. } => {
                    let down = matches!(e.kind, PointerEventKind::Press { .. });
                    match button { 0x110 => self.buttons.0 = down, 0x111 => self.buttons.1 = down, 0x112 => self.buttons.2 = down, _ => {} }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let v = if vertical.discrete != 0 { vertical.discrete } else { (vertical.absolute / 10.0) as i32 };
                    wheel += v.signum() * 3;
                }
                _ => {}
            }
        }
        let (l, r, m) = self.buttons;
        EVENTS.push(Event::Pointer { x: self.ptr.0, y: self.ptr.1, wheel, left: l, right: r, middle: m });
    }
}
impl ShmHandler for Win { fn shm_state(&mut self) -> &mut Shm { &mut self.shm } }
impl ProvidesRegistryState for Win {
    fn registry(&mut self) -> &mut RegistryState { &mut self.registry }
    registry_handlers![OutputState, SeatState];
}
delegate_compositor!(Win);
delegate_output!(Win);
delegate_shm!(Win);
delegate_seat!(Win);
delegate_keyboard!(Win);
delegate_pointer!(Win);
delegate_xdg_shell!(Win);
delegate_xdg_window!(Win);
delegate_registry!(Win);
