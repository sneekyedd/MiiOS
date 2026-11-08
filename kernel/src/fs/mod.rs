//! Storage: finds disks, mounts MiiFS, and offers simple file functions to apps.
//!
//! SAFETY RULES (your real drives matter!):
//!  * MiiOS only ever writes to a partition marked with the MiiOS type GUID.
//!  * The only exception is `prepare_disk()`, which ERASES a disk to create
//!    such a partition, and the UI calls it only after you confirm on screen.

extern crate alloc;
pub mod crc32;
pub mod gpt;
pub mod miifs;

use crate::drivers::{ahci, block::Disk, nvme, pci};
use crate::log;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use miifs::{Entry, MiiFs};
use spin::Mutex;

struct Storage { fs: Option<MiiFs>, others: Vec<Disk> }
static STORAGE: Mutex<Storage> = Mutex::new(Storage { fs: None, others: Vec::new() });

/// 0 = still looking for disks, 1 = done.
static PROBED: AtomicU8 = AtomicU8::new(0);
static JUST_MOUNTED: AtomicBool = AtomicBool::new(false);

/// Background task: find disk controllers and disks, mount MiiFS if present.
pub fn probe_task() {
    let t0 = crate::time::uptime_us();
    let devices = pci::scan();
    let mut disks: Vec<Disk> = Vec::new();
    for d in &devices {
        match (d.class, d.subclass, d.prog_if) {
            (0x01, 0x08, 0x02) => match nvme::Nvme::init(d) {
                Ok(n) => disks.push(Box::new(n)),
                Err(e) => log!("nvme: init failed: {}", e),
            },
            (0x01, 0x06, 0x01) => for p in ahci::init(d) { disks.push(Box::new(p)); },
            _ => {}
        }
        crate::task::yield_now();
    }
    let mut fs = None;
    let mut others = Vec::new();
    for mut disk in disks {
        if fs.is_none() {
            if let Some(part) = gpt::find_miios(&mut *disk) {
                log!("storage: MiiOS partition on {} ({} MiB)", disk.name(), (part.last - part.first) >> 11);
                match MiiFs::mount(disk, part) {
                    Ok(m) => { fs = Some(m); continue; }
                    Err(e) => { log!("storage: mount failed: {}", e); continue; }
                }
            }
        }
        others.push(disk);
    }
    let mounted = fs.is_some();
    {
        let mut s = STORAGE.lock();
        s.fs = fs;
        s.others = others;
    }
    PROBED.store(1, Ordering::Release);
    if mounted { JUST_MOUNTED.store(true, Ordering::Release); }
    log!("storage: probe finished in {} us", crate::time::uptime_us() - t0);
    crate::task::kick();
}

pub fn probing() -> bool { PROBED.load(Ordering::Acquire) == 0 }
/// True once, right after a filesystem was mounted (so the UI can load saved data).
pub fn take_just_mounted() -> bool { JUST_MOUNTED.swap(false, Ordering::AcqRel) }
pub fn mounted() -> bool { STORAGE.lock().fs.is_some() }

pub struct Info { pub disk: String, pub used_kib: u64, pub total_kib: u64, pub files: usize }
pub fn info() -> Option<Info> {
    let s = STORAGE.lock();
    let fs = s.fs.as_ref()?;
    Some(Info { disk: fs.disk_name(), used_kib: fs.used_blocks() as u64 * 4, total_kib: fs.total_blocks as u64 * 4, files: fs.list().len() })
}

/// Disks that could be set up for MiiOS: (name, size in MiB).
pub fn other_disks() -> Vec<(String, u64)> {
    STORAGE.lock().others.iter().map(|d| (String::from(d.name()), d.sectors() >> 11)).collect()
}

pub fn list() -> Vec<Entry> {
    let s = STORAGE.lock();
    let mut v = s.fs.as_ref().map(|f| f.list()).unwrap_or_default();
    v.sort_unstable_by(|a, b| b.mtime.cmp(&a.mtime)); // newest first
    v
}

pub fn read(name: &str) -> Result<Vec<u8>, &'static str> {
    STORAGE.lock().fs.as_mut().ok_or("no storage")?.read(name)
}

pub fn write(name: &str, data: &[u8]) -> Result<(), &'static str> {
    let now = crate::time::unix_now();
    STORAGE.lock().fs.as_mut().ok_or("no storage")?.write(name, data, now)
}

pub fn delete(name: &str) -> Result<(), &'static str> {
    STORAGE.lock().fs.as_mut().ok_or("no storage")?.delete(name)
}

/// ERASE disk number `i` (from `other_disks()`) and set it up for MiiOS.
pub fn prepare_disk(i: usize) -> Result<(), &'static str> {
    let mut s = STORAGE.lock();
    if s.fs.is_some() { return Err("already have storage"); }
    if i >= s.others.len() { return Err("no such disk"); }
    let mut disk = s.others.remove(i);
    log!("storage: ERASING {} to create a MiiOS partition", disk.name());
    let part = match gpt::create_miios_disk(&mut *disk) {
        Ok(p) => p,
        Err(e) => { s.others.insert(i, disk); return Err(e); }
    };
    s.fs = Some(MiiFs::mount(disk, part)?);
    Ok(())
}
