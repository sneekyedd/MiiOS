//! MiiFS: a tiny, crash-safe filesystem.
//!
//! The partition is divided into 4 KiB *blocks*:
//!
//!   block 0: file table, copy A
//!   block 1: file table, copy B
//!   block 2...: file data, each file stored in one run of consecutive blocks
//!
//! A file table holds up to 62 files (name, size, first block, block count,
//! time), a *generation* number and a CRC-32 checksum.
//!
//! **Why two copies?** To survive a power cut. Saving a file goes:
//!   1. write the new data into FREE blocks (the old data is untouched),
//!   2. write the updated table into whichever copy is OLDER, with
//!      generation + 1.
//! When mounting, we use the valid copy with the highest generation. If the
//! power dies during step 1 or 2, the other copy is still complete and
//! points at the old, untouched data. So a file is either fully the old
//! version or fully the new one: never half-written. ("Copy-on-write".)

extern crate alloc;
use super::crc32::crc32;
use super::gpt::Partition;
use crate::drivers::block::{Disk, SECTOR};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const BLOCK: usize = 4096;
const SPB: u64 = (BLOCK / SECTOR) as u64; // sectors per block
const MAGIC: &[u8; 8] = b"MIIFS001";
pub const MAX_FILES: usize = 62;
const ENTRY: usize = 64;
pub const NAME_MAX: usize = 39;

#[derive(Clone, Copy, Debug)]
pub struct Entry {
    pub name: [u8; 40],
    pub size: u32,
    pub start: u32,
    pub blocks: u32,
    pub mtime: i64,
}

impl Default for Entry {
    fn default() -> Entry { Entry { name: [0; 40], size: 0, start: 0, blocks: 0, mtime: 0 } }
}

impl Entry {
    pub fn name(&self) -> &str {
        let n = self.name.iter().position(|&b| b == 0).unwrap_or(40);
        core::str::from_utf8(&self.name[..n]).unwrap_or("?")
    }
    fn used(&self) -> bool { self.name[0] != 0 }
}

pub struct MiiFs {
    disk: Disk,
    part: Partition,
    pub total_blocks: u32,
    generation: u64,
    slot: u8, // which table copy is current (0 = A, 1 = B)
    pub files: [Entry; MAX_FILES],
}

impl MiiFs {
    fn read_block(&mut self, b: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        self.disk.read(self.part.first + b as u64 * SPB, buf)
    }
    fn write_block(&mut self, b: u32, buf: &[u8]) -> Result<(), &'static str> {
        self.disk.write(self.part.first + b as u64 * SPB, buf)
    }

    /// Open the filesystem on a partition, creating an empty one if the partition is blank.
    pub fn mount(disk: Disk, part: Partition) -> Result<MiiFs, &'static str> {
        let blocks = ((part.last + 1 - part.first) / SPB).min(u32::MAX as u64) as u32;
        let mut fs = MiiFs { disk, part, total_blocks: blocks, generation: 0, slot: 1, files: [Entry::default(); MAX_FILES] };
        let mut best: Option<(u64, u8, [Entry; MAX_FILES])> = None;
        let mut buf = vec![0u8; BLOCK];
        for slot in 0..2u8 {
            fs.read_block(slot as u32, &mut buf)?;
            if let Some((g, files)) = parse_table(&buf) {
                if best.as_ref().map_or(true, |b| g > b.0) { best = Some((g, slot, files)); }
            }
        }
        match best {
            Some((g, slot, files)) => { fs.generation = g; fs.slot = slot; fs.files = files; }
            None => { crate::log!("miifs: blank partition, formatting"); fs.commit()?; }
        }
        Ok(fs)
    }

    /// Write the current file list as a new table generation into the older slot.
    fn commit(&mut self) -> Result<(), &'static str> {
        self.disk.flush()?; // make sure file data is on disk before the table points at it
        let next = 1 - self.slot;
        let generation_no = self.generation + 1;
        let mut buf = vec![0u8; BLOCK];
        buf[0..8].copy_from_slice(MAGIC);
        buf[8..16].copy_from_slice(&generation_no.to_le_bytes());
        buf[16..20].copy_from_slice(&self.total_blocks.to_le_bytes());
        for (i, e) in self.files.iter().enumerate() {
            let o = ENTRY + i * ENTRY;
            buf[o..o + 40].copy_from_slice(&e.name);
            buf[o + 40..o + 44].copy_from_slice(&e.size.to_le_bytes());
            buf[o + 44..o + 48].copy_from_slice(&e.start.to_le_bytes());
            buf[o + 48..o + 52].copy_from_slice(&e.blocks.to_le_bytes());
            buf[o + 52..o + 60].copy_from_slice(&e.mtime.to_le_bytes());
        }
        let c = crc32(&buf[..BLOCK - 4]);
        buf[BLOCK - 4..].copy_from_slice(&c.to_le_bytes());
        self.write_block(next as u32, &buf)?;
        self.disk.flush()?;
        self.slot = next;
        self.generation = generation_no;
        Ok(())
    }

    pub fn find(&self, name: &str) -> Option<usize> { self.files.iter().position(|e| e.used() && e.name() == name) }

    pub fn list(&self) -> Vec<Entry> { self.files.iter().filter(|e| e.used()).copied().collect() }

    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, &'static str> {
        let e = self.files[self.find(name).ok_or("no such file")?];
        let mut v = vec![0u8; e.blocks as usize * BLOCK];
        if e.blocks > 0 { self.disk.read(self.part.first + e.start as u64 * SPB, &mut v)?; }
        v.truncate(e.size as usize);
        Ok(v)
    }

    /// Find `n` free blocks in a row, never overlapping any file in the table
    /// (including the old version of the file being replaced).
    fn find_free(&self, n: u32) -> Option<u32> {
        let mut used: Vec<(u32, u32)> = self.files.iter().filter(|e| e.used() && e.blocks > 0).map(|e| (e.start, e.start + e.blocks)).collect();
        used.sort_unstable();
        let mut pos = 2; // blocks 0 and 1 are the tables
        for (s, e) in used {
            if s >= pos + n { return Some(pos); }
            pos = pos.max(e);
        }
        if pos + n <= self.total_blocks { Some(pos) } else { None }
    }

    pub fn write(&mut self, name: &str, data: &[u8], now: i64) -> Result<(), &'static str> {
        if name.is_empty() || name.len() > NAME_MAX { return Err("bad file name"); }
        let n = data.len().div_ceil(BLOCK) as u32;
        let slot = match self.find(name) {
            Some(i) => i,
            None => self.files.iter().position(|e| !e.used()).ok_or("too many files")?,
        };
        let start = if n == 0 { 0 } else { self.find_free(n).ok_or("disk full")? };
        if n > 0 {
            let mut buf = vec![0u8; n as usize * BLOCK];
            buf[..data.len()].copy_from_slice(data);
            self.disk.write(self.part.first + start as u64 * SPB, &buf)?;
        }
        let mut e = Entry { name: [0; 40], size: data.len() as u32, start, blocks: n, mtime: now };
        e.name[..name.len()].copy_from_slice(name.as_bytes());
        let old = self.files[slot];
        self.files[slot] = e;
        if let Err(err) = self.commit() { self.files[slot] = old; return Err(err); }
        Ok(())
    }

    pub fn delete(&mut self, name: &str) -> Result<(), &'static str> {
        let i = self.find(name).ok_or("no such file")?;
        let old = self.files[i];
        self.files[i] = Entry::default();
        if let Err(e) = self.commit() { self.files[i] = old; return Err(e); }
        Ok(())
    }

    pub fn used_blocks(&self) -> u32 { 2 + self.files.iter().filter(|e| e.used()).map(|e| e.blocks).sum::<u32>() }
    pub fn disk_name(&self) -> String { String::from(self.disk.name()) }
}

fn parse_table(buf: &[u8]) -> Option<(u64, [Entry; MAX_FILES])> {
    if &buf[0..8] != MAGIC { return None; }
    let c = u32::from_le_bytes(buf[BLOCK - 4..].try_into().unwrap());
    if crc32(&buf[..BLOCK - 4]) != c { return None; }
    let generation_no = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let mut files = [Entry::default(); MAX_FILES];
    for (i, e) in files.iter_mut().enumerate() {
        let o = ENTRY + i * ENTRY;
        e.name.copy_from_slice(&buf[o..o + 40]);
        e.size = u32::from_le_bytes(buf[o + 40..o + 44].try_into().unwrap());
        e.start = u32::from_le_bytes(buf[o + 44..o + 48].try_into().unwrap());
        e.blocks = u32::from_le_bytes(buf[o + 48..o + 52].try_into().unwrap());
        e.mtime = i64::from_le_bytes(buf[o + 52..o + 60].try_into().unwrap());
    }
    Some((generation_no, files))
}
