//! USB 3 host controller (xHCI) driver, with USB keyboard + mouse support.
//!
//! The big picture:
//!  * The controller reads *rings* of 16-byte "TRBs" (Transfer Request
//!    Blocks) from RAM. There's a **command ring** (we ask the controller to
//!    do things: enable a device slot, give it an address, configure
//!    endpoints...), one **transfer ring per endpoint** (data to/from a
//!    device), and an **event ring** where the controller reports back
//!    ("command done", "transfer done").
//!  * A *doorbell* register tells the controller "new TRBs are waiting".
//!  * Each ring has a *cycle bit*: the producer flips it on every lap, so
//!    the consumer can tell fresh entries from stale ones.
//!  * Every device gets a *slot* with a *device context* (state of the device
//!    and each endpoint). We describe changes using an *input context*.
//!
//! Enumerating a device: reset its port -> Enable Slot -> Address Device ->
//! read its descriptors over the control endpoint -> pick the configuration
//! -> Configure Endpoint for its interrupt endpoint -> keep a request queued
//! there. Keyboards and mice in "boot protocol" send tiny fixed reports
//! (8 bytes for keyboards, 3-4 for mice), so no complex HID parsing needed.
//!
//! Limits: devices plugged directly into the laptop's ports only (no USB
//! hubs yet), boot-protocol keyboards and mice only.

extern crate alloc;
use super::pci::{self, Device};
use crate::input::{Event, Key, Mods, EVENTS};
use crate::mem::{early::phys_to_virt, frame, paging};
use crate::{log, task, time};
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

const RING_TRBS: usize = 256;

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Trb { param: u64, status: u32, control: u32 }

/// A producer ring (command ring or transfer ring): one page, last TRB links back.
struct Ring { phys: u64, idx: usize, cycle: bool }

impl Ring {
    fn new() -> Option<Ring> {
        let phys = frame::alloc()?;
        let r = Ring { phys, idx: 0, cycle: true };
        // Link TRB at the end: points back to the start, "toggle cycle" set.
        unsafe {
            let last = phys_to_virt::<Trb>(phys).add(RING_TRBS - 1);
            write_volatile(last, Trb { param: phys, status: 0, control: (6 << 10) | (1 << 1) });
        }
        Some(r)
    }
    /// Add a TRB; returns its physical address.
    fn push(&mut self, mut t: Trb) -> u64 {
        if self.cycle { t.control |= 1; } else { t.control &= !1; }
        let addr = self.phys + (self.idx * 16) as u64;
        unsafe { write_volatile(phys_to_virt::<Trb>(addr), t) };
        self.idx += 1;
        if self.idx == RING_TRBS - 1 {
            // Hand the link TRB to the controller with the current cycle, then wrap.
            unsafe {
                let link = phys_to_virt::<Trb>(self.phys).add(RING_TRBS - 1);
                let mut l = read_volatile(link);
                if self.cycle { l.control |= 1; } else { l.control &= !1; }
                write_volatile(link, l);
            }
            self.idx = 0;
            self.cycle = !self.cycle;
        }
        addr
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind { Keyboard, Mouse }

struct HidDev { slot: u8, dci: u8, ring: Ring, buf: u64, kind: Kind, prev: [u8; 8], last_trb: u64 }

pub struct Xhci {
    cap: *mut u8,
    op: *mut u8,
    rt: *mut u8,
    db: *mut u32,
    ctx_size: usize,
    max_ports: u8,
    dcbaa: u64,
    cmd: Ring,
    ev_phys: u64,
    ev_idx: usize,
    ev_cycle: bool,
    hid: Vec<HidDev>,
    // keyboard state for USB keyboards
    caps: bool,
    repeat_key: Option<(u8, u64)>, // (usage, next repeat time in ms)
}

unsafe impl Send for Xhci {}

fn r32(p: *mut u8, off: usize) -> u32 { unsafe { read_volatile(p.add(off) as *const u32) } }
fn w32(p: *mut u8, off: usize, v: u32) { unsafe { write_volatile(p.add(off) as *mut u32, v) } }
fn w64(p: *mut u8, off: usize, v: u64) { w32(p, off, v as u32); w32(p, off + 4, (v >> 32) as u32); }

impl Xhci {
    pub fn init(d: &Device) -> Result<Xhci, &'static str> {
        pci::enable(d);
        let cap = paging::map_mmio(pci::bar(d, 0), 0x10000)?;
        let caplen = r32(cap, 0) & 0xFF;
        let hcs1 = r32(cap, 4);
        let hcs2 = r32(cap, 8);
        let hcc1 = r32(cap, 0x10);
        let op = unsafe { cap.add(caplen as usize) };
        let rt = unsafe { cap.add((r32(cap, 0x18) & !0x1F) as usize) };
        let db = unsafe { cap.add((r32(cap, 0x14) & !0x3) as usize) } as *mut u32;
        let max_slots = (hcs1 & 0xFF) as u8;
        let max_ports = (hcs1 >> 24) as u8;
        let ctx_size = if hcc1 & (1 << 2) != 0 { 64 } else { 32 };

        // 1. Take the controller from the firmware ("BIOS handoff").
        let mut ecp = ((hcc1 >> 16) & 0xFFFF) as usize * 4;
        while ecp != 0 {
            let v = r32(cap, ecp);
            if v & 0xFF == 1 {
                w32(cap, ecp, v | (1 << 24)); // "OS owned"
                let end = time::uptime_us() + 1_000_000;
                while r32(cap, ecp) & (1 << 16) != 0 && time::uptime_us() < end { task::yield_now(); }
                w32(cap, ecp + 4, r32(cap, ecp + 4) & !0x1F | 0xE000_0000); // SMIs off, clear status
            }
            let next = ((v >> 8) & 0xFF) as usize * 4;
            ecp = if next == 0 { 0 } else { ecp + next };
        }

        // 2. Stop and reset.
        w32(op, 0, r32(op, 0) & !1);
        let end = time::uptime_us() + 1_000_000;
        while r32(op, 4) & 1 == 0 { if time::uptime_us() > end { return Err("xhci: won't halt"); } task::yield_now(); }
        w32(op, 0, 1 << 1);
        let end = time::uptime_us() + 2_000_000;
        while r32(op, 0) & (1 << 1) != 0 || r32(op, 4) & (1 << 11) != 0 {
            if time::uptime_us() > end { return Err("xhci: reset timeout"); }
            task::yield_now();
        }

        // 3. Device context array (+ scratchpad memory the controller may need).
        w32(op, 0x38, max_slots as u32);
        let dcbaa = frame::alloc().ok_or("oom")?;
        let scratch = (((hcs2 >> 21) & 0x1F) << 5 | ((hcs2 >> 27) & 0x1F)) as usize;
        if scratch > 0 {
            let arr = frame::alloc().ok_or("oom")?;
            for i in 0..scratch.min(512) {
                let p = frame::alloc().ok_or("oom")?;
                unsafe { *phys_to_virt::<u64>(arr).add(i) = p };
            }
            unsafe { *phys_to_virt::<u64>(dcbaa) = arr };
        }
        w64(op, 0x30, dcbaa);

        // 4. Command ring.
        let cmd = Ring::new().ok_or("oom")?;
        w64(op, 0x18, cmd.phys | 1);

        // 5. Event ring (one segment) for interrupter 0. We poll it.
        let ev_phys = frame::alloc().ok_or("oom")?;
        let erst = frame::alloc().ok_or("oom")?;
        unsafe {
            *phys_to_virt::<u64>(erst) = ev_phys;
            *phys_to_virt::<u32>(erst).add(2) = RING_TRBS as u32;
        }
        let ir = unsafe { rt.add(0x20) };
        w32(ir, 8, 1);                 // ERST size
        w64(ir, 0x18, ev_phys);        // dequeue pointer
        w64(ir, 0x10, erst);           // ERST base (write last)

        // 6. Run.
        w32(op, 0, r32(op, 0) | 1);
        let mut x = Xhci { cap, op, rt, db, ctx_size, max_ports, dcbaa, cmd, ev_phys, ev_idx: 0, ev_cycle: true, hid: Vec::new(), caps: false, repeat_key: None };
        log!("xhci: {} ports, {} slots, {}-byte contexts, {} scratchpad pages", max_ports, max_slots, ctx_size, scratch);
        time::spin_us(20_000); // let ports notice what's connected
        x.scan_ports();
        Ok(x)
    }

    fn portsc(&self, port: u8) -> usize { 0x400 + 0x10 * (port as usize - 1) }

    fn ring_doorbell(&self, slot: u8, target: u32) { unsafe { write_volatile(self.db.add(slot as usize), target) } }

    /// Next event from the event ring, if any.
    fn poll_event(&mut self) -> Option<Trb> {
        let t = unsafe { read_volatile(phys_to_virt::<Trb>(self.ev_phys).add(self.ev_idx)) };
        if (t.control & 1 != 0) != self.ev_cycle { return None; }
        self.ev_idx += 1;
        if self.ev_idx == RING_TRBS { self.ev_idx = 0; self.ev_cycle = !self.ev_cycle; }
        let ir = unsafe { self.rt.add(0x20) };
        w64(ir, 0x18, (self.ev_phys + self.ev_idx as u64 * 16) | (1 << 3));
        Some(t)
    }

    /// Wait for the completion event of the TRB at `addr` (command or transfer).
    /// Other events that arrive meanwhile (key presses!) are handled too.
    fn wait_for(&mut self, addr: u64) -> Result<Trb, &'static str> {
        let end = time::uptime_us() + 1_000_000;
        loop {
            while let Some(e) = self.poll_event() {
                let ty = (e.control >> 10) & 0x3F;
                if (ty == 33 || ty == 32) && e.param == addr {
                    let code = e.status >> 24;
                    return if code == 1 || code == 13 { Ok(e) } else { log!("xhci: completion code {}", code); Err("usb request failed") };
                }
                if ty == 32 { self.on_transfer(e); }
            }
            if time::uptime_us() > end { return Err("usb timeout"); }
            task::yield_now();
        }
    }

    fn command(&mut self, t: Trb) -> Result<Trb, &'static str> {
        let a = self.cmd.push(t);
        self.ring_doorbell(0, 0);
        self.wait_for(a)
    }

    /// A control transfer on endpoint 0 of `slot`. `data` is a page we read into / write from.
    fn control(&mut self, ring: &mut Ring, slot: u8, req_type: u8, req: u8, value: u16, index: u16, len: u16, data: u64) -> Result<(), &'static str> {
        let setup = req_type as u64 | (req as u64) << 8 | (value as u64) << 16 | (index as u64) << 32 | (len as u64) << 48;
        let input = req_type & 0x80 != 0;
        let trt = if len == 0 { 0 } else if input { 3 } else { 2 };
        ring.push(Trb { param: setup, status: 8, control: (2 << 10) | (1 << 6) | (trt << 16) });
        if len > 0 { ring.push(Trb { param: data, status: len as u32, control: (3 << 10) | if input { 1 << 16 } else { 0 } }); }
        let status_in = len == 0 || !input;
        let last = ring.push(Trb { param: 0, status: 0, control: (4 << 10) | (1 << 5) | if status_in { 1 << 16 } else { 0 } });
        self.ring_doorbell(slot, 1);
        self.wait_for(last).map(|_| ())
    }

    fn ctx(&self, base: u64, index: usize) -> *mut u32 { phys_to_virt::<u8>(base).wrapping_add(index * self.ctx_size) as *mut u32 }

    fn scan_ports(&mut self) {
        for port in 1..=self.max_ports {
            let off = self.portsc(port);
            let sc = r32(self.op, off);
            if sc & 1 == 0 { continue; } // nothing connected
            if sc & (1 << 1) == 0 {
                // Port reset (USB 2 ports need this to become enabled).
                w32(self.op, off, (sc & 0x0E00_C3E0) | (1 << 4));
                let end = time::uptime_us() + 500_000;
                while r32(self.op, off) & (1 << 21) == 0 && time::uptime_us() < end { task::yield_now(); }
                time::spin_us(10_000);
            }
            let sc = r32(self.op, off);
            w32(self.op, off, (sc & 0x0E00_C3E0) | (0x7F << 17)); // clear change bits
            if sc & (1 << 1) == 0 { continue; }
            let speed = ((sc >> 10) & 0xF) as u8;
            if let Err(e) = self.setup_device(port, speed) { log!("usb: port {}: {}", port, e); }
        }
    }

    fn setup_device(&mut self, port: u8, speed: u8) -> Result<(), &'static str> {
        // Enable Slot.
        let ev = self.command(Trb { param: 0, status: 0, control: 9 << 10 })?;
        let slot = (ev.control >> 24) as u8;
        // Device context (output) and input context.
        let out_ctx = frame::alloc().ok_or("oom")?;
        let inp = frame::alloc().ok_or("oom")?;
        unsafe { *phys_to_virt::<u64>(self.dcbaa).add(slot as usize) = out_ctx };
        let mut ep0 = Ring::new().ok_or("oom")?;
        let mps0: u32 = match speed { 4 => 512, 3 => 64, _ => 8 };
        unsafe {
            write_volatile(self.ctx(inp, 0).add(1), 0b11); // add slot + EP0
            let s = self.ctx(inp, 1);
            write_volatile(s, (speed as u32) << 20 | 1 << 27);
            write_volatile(s.add(1), (port as u32) << 16);
            let e = self.ctx(inp, 2);
            write_volatile(e.add(1), (3 << 1) | (4 << 3) | (mps0 << 16)); // CErr=3, control EP
            write_volatile(e.add(2), (ep0.phys as u32) | 1);
            write_volatile(e.add(3), (ep0.phys >> 32) as u32);
            write_volatile(e.add(4), 8);
        }
        // Address Device.
        self.command(Trb { param: inp, status: 0, control: (11 << 10) | ((slot as u32) << 24) })?;

        // Device descriptor (first 8 bytes tell us EP0's real max packet size).
        let page = frame::alloc().ok_or("oom")?;
        self.control(&mut ep0, slot, 0x80, 6, 0x0100, 0, 8, page)?;
        let real_mps = unsafe { *phys_to_virt::<u8>(page).add(7) } as u32;
        if speed < 3 && real_mps != 0 && real_mps != mps0 {
            unsafe {
                core::ptr::write_bytes(phys_to_virt::<u8>(inp), 0, 4096);
                write_volatile(self.ctx(inp, 0).add(1), 0b10);
                write_volatile(self.ctx(inp, 2).add(1), (3 << 1) | (4 << 3) | (real_mps << 16));
            }
            self.command(Trb { param: inp, status: 0, control: (13 << 10) | ((slot as u32) << 24) })?; // Evaluate Context
        }
        // Configuration descriptor: find a boot keyboard or mouse interface.
        self.control(&mut ep0, slot, 0x80, 6, 0x0200, 0, 255, page)?;
        let cfg = unsafe { core::slice::from_raw_parts(phys_to_virt::<u8>(page), 255) };
        let total = (u16::from_le_bytes([cfg[2], cfg[3]]) as usize).min(255);
        let config_value = cfg[5];
        let mut i = 0;
        let mut found: Option<(Kind, u8, u8, u16, u8)> = None; // kind, interface, ep addr, mps, interval
        let mut cur: Option<(Kind, u8)> = None;
        while i + 2 <= total {
            let len = cfg[i] as usize;
            if len < 2 { break; }
            match cfg[i + 1] {
                4 => { // interface
                    let (class, sub, proto) = (cfg[i + 5], cfg[i + 6], cfg[i + 7]);
                    cur = if class == 3 && sub == 1 && proto == 1 { Some((Kind::Keyboard, cfg[i + 2])) }
                          else if class == 3 && sub == 1 && proto == 2 { Some((Kind::Mouse, cfg[i + 2])) } else { None };
                }
                5 => { // endpoint
                    if let (Some((k, iface)), None) = (cur, found) {
                        let addr = cfg[i + 2];
                        if addr & 0x80 != 0 && cfg[i + 3] & 3 == 3 {
                            found = Some((k, iface, addr, u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]) & 0x7FF, cfg[i + 6]));
                        }
                    }
                }
                _ => {}
            }
            i += len;
        }
        let Some((kind, iface, ep_addr, mps, interval)) = found else { return Err("not a keyboard or mouse (skipped)") };

        self.control(&mut ep0, slot, 0x00, 9, config_value as u16, 0, 0, 0)?;       // SET_CONFIGURATION
        let _ = self.control(&mut ep0, slot, 0x21, 0x0B, 0, iface as u16, 0, 0);     // SET_PROTOCOL: boot
        let _ = self.control(&mut ep0, slot, 0x21, 0x0A, 0, iface as u16, 0, 0);     // SET_IDLE

        // Configure the interrupt IN endpoint.
        let dci = (ep_addr & 0xF) * 2 + 1;
        let ring = Ring::new().ok_or("oom")?;
        let ivl = if speed >= 3 { (interval.max(1) - 1).min(15) as u32 }
                  else { let ms = interval.max(1) as u32 * 8; (31 - ms.leading_zeros()).clamp(3, 10) };
        unsafe {
            core::ptr::write_bytes(phys_to_virt::<u8>(inp), 0, 4096);
            write_volatile(self.ctx(inp, 0).add(1), 1 | (1 << dci));
            let s = self.ctx(inp, 1);
            write_volatile(s, (speed as u32) << 20 | (dci as u32) << 27);
            write_volatile(s.add(1), (port as u32) << 16);
            let e = self.ctx(inp, 1 + dci as usize);
            write_volatile(e, ivl << 16);
            write_volatile(e.add(1), (3 << 1) | (7 << 3) | ((mps as u32) << 16)); // interrupt IN
            write_volatile(e.add(2), (ring.phys as u32) | 1);
            write_volatile(e.add(3), (ring.phys >> 32) as u32);
            write_volatile(e.add(4), (mps as u32) << 16 | 8);
        }
        self.command(Trb { param: inp, status: 0, control: (12 << 10) | ((slot as u32) << 24) })?;

        let buf = frame::alloc().ok_or("oom")?;
        let mut h = HidDev { slot, dci, ring, buf, kind, prev: [0; 8], last_trb: 0 };
        h.last_trb = h.ring.push(Trb { param: buf, status: 8, control: (1 << 10) | (1 << 5) | (1 << 2) });
        self.ring_doorbell(slot, dci as u32);
        log!("usb: port {}: {} ready (slot {}, speed {})", port, if kind == Kind::Keyboard { "keyboard" } else { "mouse" }, slot, speed);
        self.hid.push(h);
        Ok(())
    }

    /// A transfer finished: decode the report and queue the next request.
    fn on_transfer(&mut self, e: Trb) {
        let slot = (e.control >> 24) as u8;
        let dci = ((e.control >> 16) & 0x1F) as u8;
        let Some(i) = self.hid.iter().position(|h| h.slot == slot && h.dci == dci) else { return };
        let mut report = [0u8; 8];
        unsafe { core::ptr::copy_nonoverlapping(phys_to_virt::<u8>(self.hid[i].buf), report.as_mut_ptr(), 8) };
        let code = e.status >> 24;
        if code == 1 || code == 13 {
            match self.hid[i].kind {
                Kind::Keyboard => { let prev = self.hid[i].prev; self.keyboard_report(&prev, &report); self.hid[i].prev = report; }
                Kind::Mouse => EVENTS.push(Event::Mouse {
                    dx: report[1] as i8 as i32, dy: report[2] as i8 as i32, wheel: -(report[3] as i8 as i32).signum(),
                    left: report[0] & 1 != 0, right: report[0] & 2 != 0, middle: report[0] & 4 != 0,
                }),
            }
        }
        let h = &mut self.hid[i];
        h.last_trb = h.ring.push(Trb { param: h.buf, status: 8, control: (1 << 10) | (1 << 5) | (1 << 2) });
        let (s, d) = (h.slot, h.dci);
        self.ring_doorbell(s, d as u32);
    }

    fn keyboard_report(&mut self, prev: &[u8; 8], now: &[u8; 8]) {
        let m = now[0];
        let mods = Mods { shift: m & 0x22 != 0, ctrl: m & 0x11 != 0, alt: m & 0x44 != 0 };
        // Modifier keys changing.
        for (bit, key) in [(0x22u8, Key::Shift), (0x11, Key::Ctrl), (0x44, Key::Alt)] {
            if (prev[0] & bit != 0) != (m & bit != 0) { EVENTS.push(Event::Key { key, pressed: m & bit != 0, mods }); }
        }
        for &u in &now[2..8] {
            if u > 3 && !prev[2..8].contains(&u) {
                if u == 57 { self.caps = !self.caps; }
                EVENTS.push(Event::Key { key: usage_to_key(u, mods.shift, self.caps), pressed: true, mods });
                self.repeat_key = Some((u, time::ticks_ms() + 500)); // auto-repeat after 0.5 s
            }
        }
        for &u in &prev[2..8] {
            if u > 3 && !now[2..8].contains(&u) {
                EVENTS.push(Event::Key { key: usage_to_key(u, mods.shift, self.caps), pressed: false, mods });
                if self.repeat_key.map(|r| r.0) == Some(u) { self.repeat_key = None; }
            }
        }
    }

    /// Call regularly: handles reports and key auto-repeat.
    pub fn poll(&mut self) {
        while let Some(e) = self.poll_event() {
            if (e.control >> 10) & 0x3F == 32 { self.on_transfer(e); }
        }
        // USB keyboards don't repeat keys by themselves; we do it (30 per second).
        if let Some((u, at)) = self.repeat_key {
            let now = time::ticks_ms();
            if now >= at {
                let m = self.hid.iter().find(|h| h.kind == Kind::Keyboard).map(|h| h.prev[0]).unwrap_or(0);
                let mods = Mods { shift: m & 0x22 != 0, ctrl: m & 0x11 != 0, alt: m & 0x44 != 0 };
                EVENTS.push(Event::Key { key: usage_to_key(u, mods.shift, self.caps), pressed: true, mods });
                self.repeat_key = Some((u, now + 33));
            }
        }
    }

    pub fn has_devices(&self) -> bool { !self.hid.is_empty() }
}

/// USB "usage" codes (US layout) -> our keys.
fn usage_to_key(u: u8, shift: bool, caps: bool) -> Key {
    const NORMAL: &[u8; 53] = b"abcdefghijklmnopqrstuvwxyz1234567890\n\x1b\x08\t -=[]\\#;'`,./";
    const SHIFTED: &[u8; 53] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*()\n\x1b\x08\t _+{}|~:\"~<>?";
    match u {
        40 => Key::Enter, 41 => Key::Escape, 42 => Key::Backspace, 43 => Key::Tab, 57 => Key::CapsLock,
        58..=69 => Key::F(u - 57),
        73 => Key::Unknown(u), 74 => Key::Home, 75 => Key::PageUp, 76 => Key::Delete, 77 => Key::End,
        78 => Key::PageDown, 79 => Key::Right, 80 => Key::Left, 81 => Key::Down, 82 => Key::Up,
        4..=56 => {
            let i = (u - 4) as usize;
            let letter = u <= 29;
            let sh = if letter { shift ^ caps } else { shift };
            Key::Char((if sh { SHIFTED[i] } else { NORMAL[i] }) as char)
        }
        _ => Key::Unknown(u),
    }
}

/// Background task: find USB controllers, then poll them ~250 times a second.
pub fn usb_task() {
    let mut ctrls: Vec<Xhci> = Vec::new();
    for d in pci::scan_quiet() {
        if (d.class, d.subclass, d.prog_if) == (0x0C, 0x03, 0x30) {
            match Xhci::init(&d) { Ok(x) => ctrls.push(x), Err(e) => log!("xhci: {}", e) }
        }
    }
    ctrls.retain(|c| c.has_devices());
    if ctrls.is_empty() { log!("usb: no USB keyboard or mouse found"); return; }
    loop {
        for c in ctrls.iter_mut() { c.poll(); }
        if crate::input::pending() { task::kick(); }
        task::sleep_ms(4);
    }
}
