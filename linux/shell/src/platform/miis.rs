//! Your Miis: the .glb files in ~/MiiOS/miis, loaded with the 3D renderer
//! (mii3d.rs) and drawn into the MiiOS screen.
//!
//! Rendering a Mii takes a few milliseconds, so finished pictures are kept in
//! a cache keyed by (Mii, size, angle, step of the walk). A walking Mii
//! repeats the same ~16 steps, so after one lap it costs almost nothing.

use super::mii3d::{Model, Pose};
use crate::gfx::{Canvas, Rect};
use crate::host::MiiPose;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub fn dir() -> PathBuf {
    let d = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join("MiiOS").join("miis");
    let _ = std::fs::create_dir_all(&d);
    d
}

struct Entry { file: String, name: String, model: Option<Model>, stamp: u64 }

struct Store {
    list: Vec<Entry>,
    cache: HashMap<(usize, i32, i32, i32, i32, i32), Vec<u32>>,
    order: Vec<(usize, i32, i32, i32, i32, i32)>,
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(|| Mutex::new(Store { list: Vec::new(), cache: HashMap::new(), order: Vec::new() }));
const CACHE_MAX: usize = 160;

fn scan() -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = std::fs::read_dir(dir()).into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".glb") { return None; }
        let md = e.metadata().ok()?;
        let stamp = md.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() ^ md.len();
        Some((name, stamp))
    }).collect();
    v.sort();
    v
}

/// Re-read the folder. Loads new/changed files. True if the list changed.
pub fn rescan() -> bool {
    let found = scan();
    let mut s = STORE.lock().unwrap();
    let same = found.len() == s.list.len() && found.iter().zip(&s.list).all(|((f, st), e)| *f == e.file && *st == e.stamp);
    if same { return false; }
    let mut list = Vec::new();
    for (file, stamp) in found {
        let old = s.list.iter().position(|e| e.file == file && e.stamp == stamp);
        let e = match old {
            Some(i) => s.list.swap_remove(i),
            None => {
                let model = std::fs::read(dir().join(&file)).ok().and_then(|b| match Model::load(&b) {
                    Ok(m) => Some(m),
                    Err(err) => { crate::log!("couldn't load {}: {}", file, err); None }
                });
                let name = model.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| file.trim_end_matches(".glb").to_string());
                Entry { file, name, model, stamp }
            }
        };
        list.push(e);
    }
    s.list = list;
    s.cache.clear();
    s.order.clear();
    true
}

pub fn names() -> Vec<String> {
    let s = STORE.lock().unwrap();
    s.list.iter().filter(|e| e.model.is_some()).map(|e| e.name.clone()).collect()
}

fn nth_loaded(s: &Store, i: usize) -> Option<usize> { s.list.iter().enumerate().filter(|(_, e)| e.model.is_some()).nth(i).map(|(k, _)| k) }

pub fn file_of(i: usize) -> Option<String> { let s = STORE.lock().unwrap(); nth_loaded(&s, i).map(|k| s.list[k].file.clone()) }

/// Draw Mii `i` with its feet at the bottom of `dst`.
pub fn draw(c: &mut Canvas, i: usize, pose: MiiPose, dst: Rect) {
    if !dst.overlaps(&c.clip) || dst.w < 4 || dst.h < 4 { return; }
    // Round angles/phases so the same pictures get reused.
    let yaw = ((pose.yaw.rem_euclid(256) + 4) / 8 * 8) % 256;
    let walk = pose.walk.map(|p| (p.rem_euclid(256) / 16) * 16).unwrap_or(-1);
    let wave = pose.wave.map(|p| (p.rem_euclid(256) / 16) * 16).unwrap_or(-1);
    let key = (i, dst.w, dst.h, yaw, walk, wave);
    let mut s = STORE.lock().unwrap();
    let Some(k) = nth_loaded(&s, i) else { return };
    if !s.cache.contains_key(&key) {
        let model = s.list[k].model.as_ref().unwrap();
        let p = Pose { yaw, walk: if walk >= 0 { Some(walk) } else { None }, wave: if wave >= 0 { Some(wave) } else { None } };
        let px = model.render(&p, dst.w as usize, dst.h as usize, 0.93);
        if s.order.len() >= CACHE_MAX { let old = s.order.remove(0); s.cache.remove(&old); }
        s.order.push(key);
        s.cache.insert(key, px);
    }
    let px = s.cache.get(&key).unwrap();
    // 1:1 copy with alpha (the picture is already the right size).
    for y in 0..dst.h {
        for x in 0..dst.w {
            let p = px[(y * dst.w + x) as usize];
            let a = p >> 24;
            if a == 0 { continue; }
            c.blend_pub(dst.x + x, dst.y + y, 0xFF00_0000 | p, a + 1);
        }
    }
}

// ---- "This is me" ----
fn me_file() -> PathBuf { dir().join(".me") }
/// Which Mii is you: the one picked with "This is me", otherwise the Mii
/// whose name matches who created it (your own Mii, made by you).
pub fn me() -> Option<usize> {
    let s = STORE.lock().unwrap();
    let loaded: Vec<&Entry> = s.list.iter().filter(|e| e.model.is_some()).collect();
    if let Ok(want) = std::fs::read_to_string(me_file()) {
        if let Some(i) = loaded.iter().position(|e| e.file == want.trim()) { return Some(i); }
    }
    loaded.iter().position(|e| {
        let m = e.model.as_ref().unwrap();
        m.creator.as_deref().map_or(false, |c| c.eq_ignore_ascii_case(&m.name))
    })
}
pub fn set_me(i: usize) { if let Some(f) = file_of(i) { let _ = std::fs::write(me_file(), f); } }
pub fn remove(i: usize) {
    if let Some(f) = file_of(i) {
        let trash = dir().join(".removed");
        let _ = std::fs::create_dir_all(&trash);
        let _ = std::fs::rename(dir().join(&f), trash.join(&f)); // kept, just in case
    }
    rescan();
}

// ---- Importing new Miis ----
static IMPORT: Mutex<Option<String>> = Mutex::new(None);

pub fn import_start() {
    *IMPORT.lock().unwrap() = None;
    std::thread::spawn(|| {
        let mut n = 0;
        // 1. Downloads folder (e.g. Miis saved from the Mii Creator website).
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        for e in std::fs::read_dir(home.join("Downloads")).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase().ends_with(".glb") && !dir().join(&name).exists() {
                if std::fs::copy(e.path(), dir().join(&name)).is_ok() { n += 1; }
            }
        }
        // 2. USB sticks (mounted read-only by a small helper that runs as root).
        let user = std::env::var("USER").unwrap_or_else(|_| "mii".into());
        let usb = std::process::Command::new("sudo").args(["-n", "/usr/local/sbin/miios-import-usb", &user]).output().ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<usize>().ok()).unwrap_or(0);
        n += usb;
        let msg = match n {
            0 => String::from("No new Miis found. Put .glb files on a USB stick or in Downloads."),
            1 => String::from("Found 1 Mii!"),
            k => format!("Found {} Miis!", k),
        };
        *IMPORT.lock().unwrap() = Some(msg);
    });
}

pub fn import_result() -> Option<String> { IMPORT.lock().unwrap().take() }
