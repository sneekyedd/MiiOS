//! GPT ("GUID Partition Table"): how modern disks are divided into partitions.
//!
//! Sector 0 holds a "protective MBR" (so old tools see the disk as in use),
//! sector 1 holds the GPT header, and after it a table of partition entries
//! (128 bytes each: type GUID, unique GUID, first and last sector, name).
//! A backup copy lives at the very end of the disk.
//!
//! MiiOS stores its files in a partition with its own type GUID:
//! 4D69694F-5344-6174-6150-617274000001 (it spells "MiiOSDataPart" in hex!).

extern crate alloc;
use super::crc32::crc32;
use crate::drivers::block::{BlockDevice, SECTOR};
use alloc::vec;

/// The MiiOS data partition type GUID, as stored on disk (first three
/// fields are little-endian, so the bytes look shuffled).
pub const MIIOS_TYPE: [u8; 16] = [0x4F, 0x69, 0x69, 0x4D, 0x44, 0x53, 0x74, 0x61, 0x61, 0x50, 0x61, 0x72, 0x74, 0x00, 0x00, 0x01];

#[derive(Clone, Copy, Debug)]
pub struct Partition { pub first: u64, pub last: u64 }

fn u32at(b: &[u8], o: usize) -> u32 { u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) }
fn u64at(b: &[u8], o: usize) -> u64 { u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) }

/// Look for a MiiOS data partition on this disk.
pub fn find_miios(d: &mut dyn BlockDevice) -> Option<Partition> {
    let mut hdr = vec![0u8; 4096];
    d.read(0, &mut hdr).ok()?; // read 8 sectors: MBR + header + first entries
    let h = &hdr[SECTOR..2 * SECTOR];
    if &h[0..8] != b"EFI PART" { return None; }
    let entries_lba = u64at(h, 72);
    let count = u32at(h, 80).min(256) as usize;
    let esize = u32at(h, 84) as usize;
    if esize < 128 { return None; }
    let bytes = (count * esize).next_multiple_of(4096);
    let mut table = vec![0u8; bytes];
    d.read(entries_lba, &mut table).ok()?;
    for i in 0..count {
        let e = &table[i * esize..i * esize + 128];
        if e[0..16] == MIIOS_TYPE {
            return Some(Partition { first: u64at(e, 32), last: u64at(e, 40) });
        }
    }
    None
}

/// ERASE the disk's partition table and create one big MiiOS data partition.
/// Only called after the user explicitly confirms on screen.
pub fn create_miios_disk(d: &mut dyn BlockDevice) -> Result<Partition, &'static str> {
    let total = d.sectors();
    if total < 64 * 2048 { return Err("disk too small"); }
    let first = 2048u64;                 // start at 1 MiB (well aligned)
    let last = total - 34 - 2048;        // leave room for the backup table
    // Partition entry array (128 entries x 128 bytes = 16 KiB = 32 sectors).
    let mut entries = vec![0u8; 128 * 128];
    entries[0..16].copy_from_slice(&MIIOS_TYPE);
    let tsc = crate::arch::cpu::rdtsc().to_le_bytes();
    entries[16..24].copy_from_slice(&tsc); entries[24..32].copy_from_slice(&tsc); // unique id
    entries[32..40].copy_from_slice(&first.to_le_bytes());
    entries[40..48].copy_from_slice(&last.to_le_bytes());
    for (i, ch) in "MIIOS-DATA".encode_utf16().enumerate() { entries[56 + i * 2..58 + i * 2].copy_from_slice(&ch.to_le_bytes()); }
    let entries_crc = crc32(&entries);

    let header = |this: u64, other: u64, table_lba: u64| -> [u8; SECTOR] {
        let mut h = [0u8; SECTOR];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // revision 1.0
        h[12..16].copy_from_slice(&92u32.to_le_bytes());          // header size
        h[24..32].copy_from_slice(&this.to_le_bytes());
        h[32..40].copy_from_slice(&other.to_le_bytes());
        h[40..48].copy_from_slice(&34u64.to_le_bytes());           // first usable
        h[48..56].copy_from_slice(&(total - 34).to_le_bytes());    // last usable
        h[56..64].copy_from_slice(&tsc); h[64..72].copy_from_slice(&tsc); // disk id
        h[72..80].copy_from_slice(&table_lba.to_le_bytes());
        h[80..84].copy_from_slice(&128u32.to_le_bytes());
        h[84..88].copy_from_slice(&128u32.to_le_bytes());
        h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let c = crc32(&h[0..92]);
        h[16..20].copy_from_slice(&c.to_le_bytes());
        h
    };

    // Protective MBR: one partition of type 0xEE covering the disk.
    let mut first8 = vec![0u8; 4096];
    let mbr = &mut first8[0..SECTOR];
    mbr[446 + 4] = 0xEE;
    mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&((total - 1).min(0xFFFF_FFFF) as u32).to_le_bytes());
    mbr[510] = 0x55; mbr[511] = 0xAA;
    first8[SECTOR..2 * SECTOR].copy_from_slice(&header(1, total - 1, 2));
    first8[2 * SECTOR..8 * SECTOR].copy_from_slice(&entries[..6 * SECTOR]);
    d.write(0, &first8)?;
    // The rest of the primary entry array: sectors 8..34 (26 sectors), written as 4 KiB blocks.
    let mut rest = vec![0u8; 28 * SECTOR];
    rest[..26 * SECTOR].copy_from_slice(&entries[6 * SECTOR..]);
    d.write(8, &rest[..24 * SECTOR])?;
    let mut tail = vec![0u8; 8 * SECTOR];
    d.read(32, &mut tail)?;
    tail[..2 * SECTOR].copy_from_slice(&entries[30 * SECTOR..]);
    d.write(32, &tail)?;
    // Backup: entries at total-33, header at total-1. Write the last 40 sectors as aligned 4 KiB blocks.
    let tail_start = (total - 40) & !7;
    let mut end = vec![0u8; ((total - tail_start) as usize) * SECTOR];
    let off = ((total - 33 - tail_start) as usize) * SECTOR;
    end[off..off + 32 * SECTOR].copy_from_slice(&entries);
    let hoff = ((total - 1 - tail_start) as usize) * SECTOR;
    end[hoff..hoff + SECTOR].copy_from_slice(&header(total - 1, 1, total - 33));
    let whole = end.len() / 4096 * 4096;
    if whole == end.len() { d.write(tail_start, &end)?; } else { return Err("disk size not a multiple of 4 KiB"); }
    d.flush()?;
    Ok(Partition { first, last })
}
