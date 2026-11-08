//! PCI: the bus that connects the CPU to devices like disk controllers,
//! network cards and USB controllers.
//!
//! Every device has a small "configuration space" (256 bytes) that says who
//! made it (vendor id), what it is (class code) and where its registers live
//! (BARs = Base Address Registers). We read it through two I/O ports:
//! write the address of the register to 0xCF8, then read/write it at 0xCFC.

extern crate alloc;
use crate::arch::cpu::{inl, outl};
use crate::log;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub bus: u8, pub dev: u8, pub func: u8,
    pub vendor: u16, pub device: u16,
    pub class: u8, pub subclass: u8, pub prog_if: u8,
}

fn addr(bus: u8, dev: u8, func: u8, off: u8) -> u32 {
    0x8000_0000 | (bus as u32) << 16 | (dev as u32) << 11 | (func as u32) << 8 | (off as u32 & 0xFC)
}

pub fn read32(d: &Device, off: u8) -> u32 { outl(0xCF8, addr(d.bus, d.dev, d.func, off)); inl(0xCFC) }
pub fn write32(d: &Device, off: u8, v: u32) { outl(0xCF8, addr(d.bus, d.dev, d.func, off)); outl(0xCFC, v) }

fn read_raw(bus: u8, dev: u8, func: u8, off: u8) -> u32 { outl(0xCF8, addr(bus, dev, func, off)); inl(0xCFC) }

/// Find every device: start at bus 0 and follow PCI-to-PCI bridges to the
/// buses behind them (much faster than trying all 256 buses).
pub fn scan() -> Vec<Device> {
    let mut out = Vec::new();
    let mut seen = [false; 256];
    scan_bus(0, &mut out, &mut seen);
    for d in &out {
        log!("pci {:02x}:{:02x}.{} {:04x}:{:04x} class {:02x}.{:02x}.{:02x} {}", d.bus, d.dev, d.func, d.vendor, d.device, d.class, d.subclass, d.prog_if, describe(d));
    }
    out
}

fn scan_bus(bus: u8, out: &mut Vec<Device>, seen: &mut [bool; 256]) {
    if seen[bus as usize] { return; }
    seen[bus as usize] = true;
    for dev in 0..32u8 {
        let id = read_raw(bus, dev, 0, 0);
        if id & 0xFFFF == 0xFFFF { continue; }
        let header = (read_raw(bus, dev, 0, 0x0C) >> 16) & 0xFF;
        let funcs = if header & 0x80 != 0 { 8 } else { 1 };
        for func in 0..funcs {
            let id = read_raw(bus, dev, func, 0);
            if id & 0xFFFF == 0xFFFF { continue; }
            let cls = read_raw(bus, dev, func, 0x08);
            let d = Device {
                bus, dev, func, vendor: id as u16, device: (id >> 16) as u16,
                class: (cls >> 24) as u8, subclass: (cls >> 16) as u8, prog_if: (cls >> 8) as u8,
            };
            out.push(d);
            if d.class == 0x06 && d.subclass == 0x04 {
                let secondary = ((read_raw(bus, dev, func, 0x18) >> 8) & 0xFF) as u8;
                if secondary != 0 { scan_bus(secondary, out, seen); }
            }
        }
    }
}

pub fn describe(d: &Device) -> &'static str {
    match (d.class, d.subclass, d.prog_if) {
        (0x01, 0x08, 0x02) => "NVMe SSD controller",
        (0x01, 0x06, 0x01) => "SATA (AHCI) controller",
        (0x02, 0x00, _) => "Ethernet controller",
        (0x02, 0x80, _) => "Wi-Fi / other network",
        (0x03, _, _) => "display controller",
        (0x04, _, _) => "audio",
        (0x06, _, _) => "bridge",
        (0x0C, 0x03, 0x30) => "USB 3 (xHCI) controller",
        (0x0C, 0x03, _) => "USB controller",
        (0x0C, 0x05, _) => "SMBus",
        _ => "",
    }
}

/// Physical address of a memory BAR (handles 64-bit BARs).
pub fn bar(d: &Device, n: u8) -> u64 {
    let off = 0x10 + n * 4;
    let lo = read32(d, off);
    if lo & 1 != 0 { return (lo & !3) as u64; } // I/O port BAR
    let mut a = (lo & !0xF) as u64;
    if (lo >> 1) & 3 == 2 { a |= (read32(d, off + 4) as u64) << 32; }
    a
}

/// Let the device read/write RAM by itself ("bus mastering", needed for DMA)
/// and respond to memory accesses.
pub fn enable(d: &Device) {
    let cmd = read32(d, 0x04);
    write32(d, 0x04, (cmd & 0xFFFF_0000) | (cmd & 0xFFFF) | 0b110);
}
