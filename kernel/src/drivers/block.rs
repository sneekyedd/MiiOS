//! A common interface for disks, so the filesystem doesn't care whether the
//! drive is NVMe or SATA. Disks are arrays of 512-byte *sectors*.

extern crate alloc;
use alloc::boxed::Box;

pub const SECTOR: usize = 512;

pub trait BlockDevice {
    fn name(&self) -> &str;
    /// Total size in 512-byte sectors.
    fn sectors(&self) -> u64;
    /// Read `buf.len() / 512` sectors starting at `lba`.
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), &'static str>;
    fn flush(&mut self) -> Result<(), &'static str> { Ok(()) }
}

pub type Disk = Box<dyn BlockDevice + Send>;
