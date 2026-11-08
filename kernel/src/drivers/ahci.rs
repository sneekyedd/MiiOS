//! SATA driver (AHCI, polled). Many T470s shipped with a SATA SSD instead of
//! NVMe, so we support both.
//!
//! AHCI in short: the controller has up to 32 *ports* (one per SATA
//! connector). For each port we give it, in RAM: a *command list* (32
//! slots), a buffer where the drive's replies land, and a *command table*
//! holding the actual ATA command plus a list of RAM buffers ("PRDT") to
//! move data to/from. We fill slot 0, set bit 0 in the port's "command
//! issue" register, and wait for the bit to clear.

extern crate alloc;
use super::block::{BlockDevice, SECTOR};
use super::pci::{self, Device};
use crate::mem::{early::phys_to_virt, frame, paging};
use crate::util::FmtBuf;
use crate::{log, time};
use alloc::vec::Vec;
use core::fmt::Write;
use core::ptr::{read_volatile, write_volatile};

pub struct AhciPort {
    port: *mut u8,
    list: u64,   // command list (1 KiB) + received FIS (256 B) share a page
    table: u64,  // command table page
    bounce: u64, // data page
    sectors: u64,
    name: FmtBuf<48>,
}

impl AhciPort {
    fn r(&self, off: usize) -> u32 { unsafe { read_volatile(self.port.add(off) as *const u32) } }
    fn w(&self, off: usize, v: u32) { unsafe { write_volatile(self.port.add(off) as *mut u32, v) } }

    /// Run one ATA command. `count` sectors, data in the bounce page.
    fn command(&mut self, cmd: u8, lba: u64, count: u16, write: bool, bytes: u32) -> Result<(), &'static str> {
        // Wait while the drive is busy.
        let end = time::uptime_us() + 2_000_000;
        while self.r(0x20) & 0x88 != 0 { if time::uptime_us() > end { return Err("SATA drive busy"); } }
        self.w(0x10, u32::MAX); // clear interrupt status
        unsafe {
            // Command header 0: FIS length 5 dwords, write flag, 1 PRDT entry.
            let hdr = phys_to_virt::<u32>(self.list);
            write_volatile(hdr, 5 | if write { 1 << 6 } else { 0 } | (1 << 16));
            write_volatile(hdr.add(1), 0);
            write_volatile(hdr.add(2), self.table as u32);
            write_volatile(hdr.add(3), (self.table >> 32) as u32);
            // Command table: the ATA command as a "Register Host to Device FIS".
            let t = phys_to_virt::<u8>(self.table);
            core::ptr::write_bytes(t, 0, 0x90);
            let fis = [
                0x27, 0x80, cmd, 0,
                lba as u8, (lba >> 8) as u8, (lba >> 16) as u8, 1 << 6,
                (lba >> 24) as u8, (lba >> 32) as u8, (lba >> 40) as u8, 0,
                count as u8, (count >> 8) as u8, 0, 0,
            ];
            core::ptr::copy_nonoverlapping(fis.as_ptr(), t, fis.len());
            // One PRDT entry pointing at the bounce page.
            let prdt = t.add(0x80) as *mut u32;
            write_volatile(prdt, self.bounce as u32);
            write_volatile(prdt.add(1), (self.bounce >> 32) as u32);
            write_volatile(prdt.add(3), bytes.max(2) - 1);
        }
        self.w(0x38, 1); // issue slot 0
        let end = time::uptime_us() + 5_000_000;
        loop {
            if self.r(0x10) & (1 << 30) != 0 { return Err("SATA command failed"); }
            if self.r(0x38) & 1 == 0 { break; }
            if time::uptime_us() > end { return Err("SATA drive did not answer"); }
            crate::task::yield_now();
        }
        if self.r(0x20) & 1 != 0 { return Err("SATA error"); }
        Ok(())
    }

    fn transfer(&mut self, write: bool, mut lba: u64, buf: *mut u8, len: usize) -> Result<(), &'static str> {
        let mut done = 0;
        while done < len {
            let chunk = (len - done).min(4096);
            let page = phys_to_virt::<u8>(self.bounce);
            unsafe {
                if write { core::ptr::copy_nonoverlapping(buf.add(done), page, chunk); }
                // READ DMA EXT (0x25) / WRITE DMA EXT (0x35): 48-bit addresses.
                self.command(if write { 0x35 } else { 0x25 }, lba, (chunk / SECTOR) as u16, write, chunk as u32)?;
                if !write { core::ptr::copy_nonoverlapping(page, buf.add(done), chunk); }
            }
            lba += (chunk / SECTOR) as u64;
            done += chunk;
        }
        Ok(())
    }
}

impl BlockDevice for AhciPort {
    fn name(&self) -> &str { self.name.as_str() }
    fn sectors(&self) -> u64 { self.sectors }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> { self.transfer(false, lba, buf.as_mut_ptr(), buf.len()) }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str> { self.transfer(true, lba, buf.as_ptr() as *mut u8, buf.len()) }
    fn flush(&mut self) -> Result<(), &'static str> { self.command(0xEA, 0, 0, false, 0) } // FLUSH CACHE EXT
}

/// Find every SATA disk on an AHCI controller.
pub fn init(d: &Device) -> Vec<AhciPort> {
    let mut out = Vec::new();
    pci::enable(d);
    let Ok(abar) = paging::map_mmio(pci::bar(d, 5), 0x1100) else { return out };
    let r = |off: usize| unsafe { read_volatile(abar.add(off) as *const u32) };
    let w = |off: usize, v: u32| unsafe { write_volatile(abar.add(off) as *mut u32, v) };
    w(0x04, r(0x04) | (1 << 31)); // AHCI mode on
    let implemented = r(0x0C);
    for i in 0..32 {
        if implemented & (1 << i) == 0 { continue; }
        let port = unsafe { abar.add(0x100 + i * 0x80) };
        let pr = |off: usize| unsafe { read_volatile(port.add(off) as *const u32) };
        let pw = |off: usize, v: u32| unsafe { write_volatile(port.add(off) as *mut u32, v) };
        if pr(0x28) & 0xF != 3 || pr(0x24) != 0x0000_0101 { continue; } // no SATA disk attached
        // Stop the port, give it our memory, start it again.
        pw(0x18, pr(0x18) & !0x11);
        let end = time::uptime_us() + 500_000;
        while pr(0x18) & 0xC000 != 0 && time::uptime_us() < end {}
        let (Some(list), Some(table), Some(bounce)) = (frame::alloc(), frame::alloc(), frame::alloc()) else { break };
        pw(0x00, list as u32); pw(0x04, (list >> 32) as u32);
        pw(0x08, (list + 1024) as u32); pw(0x0C, ((list + 1024) >> 32) as u32);
        pw(0x30, u32::MAX);
        pw(0x18, pr(0x18) | 0x10); // receive FIS on
        pw(0x18, pr(0x18) | 0x01); // start
        let mut p = AhciPort { port, list, table, bounce, sectors: 0, name: FmtBuf::new() };
        // IDENTIFY DEVICE (0xEC): model name and size.
        if p.command(0xEC, 0, 0, false, 512).is_err() { continue; }
        let id = unsafe { core::slice::from_raw_parts(phys_to_virt::<u16>(bounce), 256) };
        p.sectors = id[100] as u64 | (id[101] as u64) << 16 | (id[102] as u64) << 32 | (id[103] as u64) << 48;
        let mut model = [0u8; 40];
        for k in 0..20 { model[k * 2] = (id[27 + k] >> 8) as u8; model[k * 2 + 1] = id[27 + k] as u8; }
        let _ = write!(p.name, "SATA {}", core::str::from_utf8(&model).unwrap_or("disk").trim());
        log!("ahci: port {}: {} - {} MiB", i, p.name.as_str(), p.sectors >> 11);
        out.push(p);
    }
    out
}

// Safety: the driver is only used from one task at a time (behind a lock).
unsafe impl Send for AhciPort {}
