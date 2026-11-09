//! Files: on Linux, MiiOS keeps your notes, pals and settings as ordinary
//! files in the folder ~/MiiOS (so you can also see them from the Terminal).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub mod miifs {
    /// Same shape as the bare-metal filesystem's entry, so the Files app works unchanged.
    #[derive(Clone, Copy, Debug)]
    pub struct Entry { pub name: [u8; 40], pub size: u32, pub start: u32, pub blocks: u32, pub mtime: i64 }
    impl Entry {
        pub fn name(&self) -> &str {
            let n = self.name.iter().position(|&b| b == 0).unwrap_or(40);
            core::str::from_utf8(&self.name[..n]).unwrap_or("?")
        }
    }
}
use miifs::Entry;

fn dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
    let d = PathBuf::from(home).join("MiiOS");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn ok_name(name: &str) -> bool { !name.is_empty() && name.len() < 40 && !name.contains('/') && !name.starts_with('.') }

static LOADED: AtomicBool = AtomicBool::new(false);
/// True once at start-up, so the shell loads your saved pals and settings.
pub fn take_just_mounted() -> bool { !LOADED.swap(true, Ordering::Relaxed) }
pub fn probing() -> bool { false }
pub fn mounted() -> bool { true }

pub struct Info { pub disk: String, pub used_kib: u64, pub total_kib: u64, pub files: usize }
pub fn info() -> Option<Info> {
    let files = list();
    let used: u64 = files.iter().map(|e| e.size as u64).sum::<u64>() / 1024 + 1;
    // Free space of the drive holding ~/MiiOS, via `df`.
    let total = std::process::Command::new("df").args(["-k", "--output=size"]).arg(dir()).output().ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().nth(1)?.trim().parse::<u64>().ok()).unwrap_or(0);
    Some(Info { disk: String::from("This laptop, folder ~/MiiOS"), used_kib: used, total_kib: total, files: files.len() })
}

pub fn list() -> Vec<Entry> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !ok_name(&name) { continue; }
            let Ok(md) = e.metadata() else { continue };
            if !md.is_file() { continue; }
            let mut n = [0u8; 40];
            n[..name.len()].copy_from_slice(name.as_bytes());
            let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
            v.push(Entry { name: n, size: md.len() as u32, start: 0, blocks: 0, mtime });
        }
    }
    v.sort_unstable_by(|a, b| b.mtime.cmp(&a.mtime));
    v
}

pub fn read(name: &str) -> Result<Vec<u8>, &'static str> {
    if !ok_name(name) { return Err("bad file name"); }
    std::fs::read(dir().join(name)).map_err(|_| "no such file")
}

/// Write via a temporary file + rename, so a power cut never leaves half a file.
pub fn write(name: &str, data: &[u8]) -> Result<(), &'static str> {
    if !ok_name(name) { return Err("bad file name"); }
    let tmp = dir().join(format!(".{}.tmp", name));
    std::fs::write(&tmp, data).map_err(|_| "could not write")?;
    std::fs::rename(&tmp, dir().join(name)).map_err(|_| "could not save")
}

pub fn delete(name: &str) -> Result<(), &'static str> {
    if !ok_name(name) { return Err("bad file name"); }
    std::fs::remove_file(dir().join(name)).map_err(|_| "could not delete")
}

pub fn other_disks() -> Vec<(String, u64)> { Vec::new() }
pub fn prepare_disk(_i: usize) -> Result<(), &'static str> { Err("not needed on the Linux edition") }
