//! NVMe SSD driver (polled, one I/O queue).
//!
//! How NVMe works, in short:
//!  * The controller's registers sit in memory (BAR0). We give it the
//!    physical addresses of two rings in RAM for each queue: a *submission
//!    queue* (SQ, 64-byte commands) and a *completion queue* (CQ, 16-byte results).
//!  * To run a command: write it into the next SQ slot, then write the new
//!    tail index to the queue's *doorbell* register. The drive fetches the
//!    command, moves the data by DMA (directly to/from RAM), and writes a
//!    result into the CQ. Each result has a "phase" bit that flips every time
//!    the ring wraps around, so we can tell new results from old ones.
//!  * Queue 0 is the *admin* queue (setup commands). We create queue 1 for
//!    reads and writes.
//! We poll for completions instead of using interrupts: simple, and with
//! NVMe latencies of ~0.1 ms it's perfectly fast for our needs.

use super::block::{BlockDevice, SECTOR};
use super::pci::{self, Device};
use crate::mem::{early::phys_to_virt, frame, paging};
use crate::{log, time};
use crate::util::FmtBuf;
use core::fmt::Write;
use core::ptr::{read_volatile, write_volatile};

const QSIZE: u16 = 16;

struct Queue { sq: u64, cq: u64, tail: u16, head: u16, phase: bool, id: u16 }

pub struct Nvme {
    regs: *mut u8,
    stride: usize,
    admin: Queue,
    io: Queue,
    cid: u16,
    lba_size: usize,
    lbas: u64,
    bounce: u64, // one 4 KiB page for data transfers
    name: FmtBuf<48>,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct Cmd { dw: [u32; 16] }

impl Nvme {
    fn r32(&self, off: usize) -> u32 { unsafe { read_volatile(self.regs.add(off) as *const u32) } }
    fn w32(&self, off: usize, v: u32) { unsafe { write_volatile(self.regs.add(off) as *mut u32, v) } }
    fn r64(&self, off: usize) -> u64 { self.r32(off) as u64 | (self.r32(off + 4) as u64) << 32 }
    fn w64(&self, off: usize, v: u64) { self.w32(off, v as u32); self.w32(off + 4, (v >> 32) as u32); }

    fn doorbell(&self, q: u16, completion: bool) -> usize { 0x1000 + (2 * q as usize + completion as usize) * self.stride }

    /// Run one command on a queue and wait for its result. Returns dword 0 of the result.
    fn submit(&mut self, admin: bool, mut cmd: Cmd) -> Result<u32, &'static str> {
        self.cid = self.cid.wrapping_add(1);
        cmd.dw[0] |= (self.cid as u32) << 16;
        let q = if admin { &mut self.admin } else { &mut self.io };
        unsafe {
            let slot = phys_to_virt::<Cmd>(q.sq).add(q.tail as usize);
            write_volatile(slot, cmd);
        }
        q.tail = (q.tail + 1) % QSIZE;
        let (qid, tail) = (q.id, q.tail);
        self.w32(self.doorbell(qid, false), tail as u32);

        let deadline = time::uptime_us() + 3_000_000;
        loop {
            let q = if admin { &mut self.admin } else { &mut self.io };
            let entry = unsafe { phys_to_virt::<[u32; 4]>(q.cq).add(q.head as usize) };
            let dw3 = unsafe { read_volatile(&(*entry)[3]) };
            if ((dw3 >> 16) & 1 != 0) == q.phase {
                let dw0 = unsafe { read_volatile(&(*entry)[0]) };
                q.head = (q.head + 1) % QSIZE;
                if q.head == 0 { q.phase = !q.phase; }
                let (qid, head) = (q.id, q.head);
                self.w32(self.doorbell(qid, true), head as u32);
                let status = (dw3 >> 17) & 0x7FF;
                return if status == 0 { Ok(dw0) } else { log!("nvme: command failed, status {:#x}", status); Err("disk command failed") };
            }
            if time::uptime_us() > deadline { return Err("disk did not answer"); }
            crate::task::yield_now();
        }
    }

    pub fn init(d: &Device) -> Result<Nvme, &'static str> {
        pci::enable(d);
        let phys = pci::bar(d, 0);
        let regs = paging::map_mmio(phys, 0x4000)?;
        let mut n = Nvme {
            regs, stride: 4, admin: Queue { sq: 0, cq: 0, tail: 0, head: 0, phase: true, id: 0 },
            io: Queue { sq: 0, cq: 0, tail: 0, head: 0, phase: true, id: 1 },
            cid: 0, lba_size: 512, lbas: 0, bounce: 0, name: FmtBuf::new(),
        };
        let cap = n.r64(0);
        n.stride = 4 << ((cap >> 32) & 0xF);
        let timeout_us = ((cap >> 24) & 0xFF).max(1) * 500_000;

        // 1. Turn the controller off and wait until it says it's off.
        n.w32(0x14, n.r32(0x14) & !1);
        n.wait_ready(false, timeout_us)?;

        // 2. Give it the admin queues.
        let f = |_: ()| frame::alloc().ok_or("out of memory");
        n.admin.sq = f(())?;
        n.admin.cq = f(())?;
        n.w32(0x24, ((QSIZE as u32 - 1) << 16) | (QSIZE as u32 - 1));
        n.w64(0x28, n.admin.sq);
        n.w64(0x30, n.admin.cq);

        // 3. Turn it on: NVM command set, 4 KiB pages, 64-byte commands, 16-byte results.
        n.w32(0x14, 1 | (6 << 16) | (4 << 20));
        n.wait_ready(true, timeout_us)?;

        n.bounce = f(())?;

        // 4. Identify controller: get the model name.
        let mut c = Cmd::default();
        c.dw[0] = 0x06; c.dw[6] = n.bounce as u32; c.dw[7] = (n.bounce >> 32) as u32; c.dw[10] = 1;
        n.submit(true, c)?;
        let id = unsafe { core::slice::from_raw_parts(phys_to_virt::<u8>(n.bounce), 4096) };
        let model = core::str::from_utf8(&id[24..64]).unwrap_or("NVMe").trim();
        let _ = write!(n.name, "NVMe {}", model);

        // 5. Identify namespace 1: its size and sector size.
        let mut c = Cmd::default();
        c.dw[0] = 0x06; c.dw[1] = 1; c.dw[6] = n.bounce as u32; c.dw[7] = (n.bounce >> 32) as u32; c.dw[10] = 0;
        n.submit(true, c)?;
        let ns = unsafe { core::slice::from_raw_parts(phys_to_virt::<u8>(n.bounce), 4096) };
        n.lbas = u64::from_le_bytes(ns[0..8].try_into().unwrap());
        let flbas = (ns[26] & 0xF) as usize;
        let lbaf = u32::from_le_bytes(ns[128 + flbas * 4..132 + flbas * 4].try_into().unwrap());
        n.lba_size = 1 << ((lbaf >> 16) & 0xFF);

        // 6. Create I/O completion queue 1, then submission queue 1 (no interrupts).
        n.io.sq = f(())?;
        n.io.cq = f(())?;
        let mut c = Cmd::default();
        c.dw[0] = 0x05; c.dw[6] = n.io.cq as u32; c.dw[7] = (n.io.cq >> 32) as u32;
        c.dw[10] = ((QSIZE as u32 - 1) << 16) | 1; c.dw[11] = 1;
        n.submit(true, c)?;
        let mut c = Cmd::default();
        c.dw[0] = 0x01; c.dw[6] = n.io.sq as u32; c.dw[7] = (n.io.sq >> 32) as u32;
        c.dw[10] = ((QSIZE as u32 - 1) << 16) | 1; c.dw[11] = (1 << 16) | 1;
        n.submit(true, c)?;

        log!("nvme: {} - {} MiB, {}-byte sectors", n.name.as_str(), n.lbas * n.lba_size as u64 >> 20, n.lba_size);
        Ok(n)
    }

    fn wait_ready(&self, ready: bool, timeout_us: u64) -> Result<(), &'static str> {
        let end = time::uptime_us() + timeout_us;
        while (self.r32(0x1C) & 1 != 0) != ready {
            if self.r32(0x1C) & 2 != 0 { return Err("controller fatal error"); }
            if time::uptime_us() > end { return Err("controller timeout"); }
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Read or write up to one page at native LBA `lba`.
    fn io(&mut self, write: bool, lba: u64, count: u32) -> Result<(), &'static str> {
        let mut c = Cmd::default();
        c.dw[0] = if write { 0x01 } else { 0x02 };
        c.dw[1] = 1;
        c.dw[6] = self.bounce as u32; c.dw[7] = (self.bounce >> 32) as u32;
        c.dw[10] = lba as u32; c.dw[11] = (lba >> 32) as u32;
        c.dw[12] = count - 1;
        self.submit(false, c).map(|_| ())
    }

    /// Split a 512-byte-sector request into page-sized native requests.
    fn transfer(&mut self, write: bool, lba: u64, buf: *mut u8, len: usize) -> Result<(), &'static str> {
        let ls = self.lba_size;
        let byte = lba * SECTOR as u64;
        if byte % ls as u64 != 0 || len % ls != 0 { return Err("unaligned disk access"); }
        let mut native = byte / ls as u64;
        let mut done = 0;
        while done < len {
            let chunk = (len - done).min(4096);
            let page = phys_to_virt::<u8>(self.bounce);
            unsafe {
                if write { core::ptr::copy_nonoverlapping(buf.add(done), page, chunk); }
                self.io(write, native, (chunk / ls) as u32)?;
                if !write { core::ptr::copy_nonoverlapping(page, buf.add(done), chunk); }
            }
            native += (chunk / ls) as u64;
            done += chunk;
        }
        Ok(())
    }
}

impl BlockDevice for Nvme {
    fn name(&self) -> &str { self.name.as_str() }
    fn sectors(&self) -> u64 { self.lbas * self.lba_size as u64 / SECTOR as u64 }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str> { self.transfer(false, lba, buf.as_mut_ptr(), buf.len()) }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str> { self.transfer(true, lba, buf.as_ptr() as *mut u8, buf.len()) }
    fn flush(&mut self) -> Result<(), &'static str> {
        let mut c = Cmd::default();
        c.dw[0] = 0x00; c.dw[1] = 1;
        self.submit(false, c).map(|_| ())
    }
}

// Safety: the driver is only used from one task at a time (behind a lock).
unsafe impl Send for Nvme {}
