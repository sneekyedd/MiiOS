//! Your Miis: the .glb files in ~/MiiOS/miis, loaded with the 3D renderer
//! (mii3d.rs) and drawn into the MiiOS screen.
//!
//! Drawing a Mii takes a few milliseconds, so the UI never does it itself:
//!   * `draw` asks for a picture of (Mii, size, pose). If it's in the cache,
//!     it's shown at once. If not, a *job* is queued and the Mii's most
//!     recent picture is shown meanwhile (one frame later the new one is there).
//!   * 3 worker threads take jobs from the queue and render them, in parallel.
//!     The i5-7200U has 2 cores / 4 threads: the UI keeps one for itself.
//!   * Poses are rounded a little, and recent pictures are kept, so repeating
//!     movements (walking, swinging legs) cost almost nothing after one cycle.

use super::mii3d::{Model, Pose};
use crate::gfx::{Canvas, Rect};
use crate::host::MiiPose;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

pub fn dir() -> PathBuf {
    let d = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())).join("MiiOS").join("miis");
    let _ = std::fs::create_dir_all(&d);
    d
}

struct Entry { file: String, name: String, model: Option<Arc<Model>>, stamp: u64 }

type Key = (usize, i32, i32, MiiPose); // (Mii, width, height, rounded pose)

struct Store {
    list: Vec<Entry>,
    cache: HashMap<Key, Arc<Vec<u32>>>,
    order: VecDeque<Key>,
    latest: HashMap<(usize, i32, i32), Arc<Vec<u32>>>, // newest picture per Mii+size (shown while waiting)
    queue: VecDeque<(Key, Arc<Model>)>,
    generation: u64, // bumps when the Mii list changes (old jobs are dropped)
}

static STORE: std::sync::LazyLock<Mutex<Store>> = std::sync::LazyLock::new(|| {
    Mutex::new(Store { list: Vec::new(), cache: HashMap::new(), order: VecDeque::new(), latest: HashMap::new(), queue: VecDeque::new(), generation: 0 })
});
static WAKE: Condvar = Condvar::new();
static BUSY: AtomicUsize = AtomicUsize::new(0);
const CACHE_MAX: usize = 400; // ~ 400 small pictures, a few tens of MB
const WORKERS: usize = 3;

fn start_workers() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for _ in 0..WORKERS {
            std::thread::spawn(|| loop {
                let (key, model, generation) = {
                    let mut s = STORE.lock().unwrap();
                    loop {
                        if let Some((k, m)) = s.queue.pop_front() { break (k, m, s.generation); }
                        s = WAKE.wait(s).unwrap();
                    }
                };
                BUSY.fetch_add(1, Ordering::Relaxed);
                let (i, w, h, pose) = key;
                let fill = if pose.pitch != 0 || pose.roll != 0 { 0.62 } else { 0.92 };
                let px = Arc::new(model.render(&Pose { test: None, mii: pose }, w as usize, h as usize, fill));
                {
                    let mut s = STORE.lock().unwrap();
                    if s.generation == generation {
                        s.latest.insert((i, w, h), px.clone());
                        if s.order.len() >= CACHE_MAX { if let Some(old) = s.order.pop_front() { s.cache.remove(&old); } }
                        s.order.push_back(key);
                        s.cache.insert(key, px);
                    }
                }
                BUSY.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
}

/// True while pictures are still being drawn (the UI keeps refreshing until done).
pub fn rendering() -> bool {
    BUSY.load(Ordering::Relaxed) > 0 || !STORE.lock().unwrap().queue.is_empty()
}

/// Round a pose so that tiny differences share one picture.
fn round(mut p: MiiPose) -> MiiPose {
    p.yaw = (p.yaw.rem_euclid(256) + 2) / 4 * 4 % 256;
    p.pitch = (p.pitch.rem_euclid(256) + 2) / 4 * 4 % 256;
    p.roll = (p.roll.rem_euclid(256) + 2) / 4 * 4 % 256;
    p.squash = (p.squash + 4) / 8 * 8;
    for v in p.j.iter_mut() { *v = ((*v as i32 + if *v >= 0 { 1 } else { -1 }) / 3 * 3).clamp(-126, 126) as i8; }
    p
}

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
    auto_import();
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
                    Ok(m) => Some(Arc::new(m)),
                    Err(err) => { crate::log!("couldn't load {}: {}", file, err); None }
                });
                let mut name = model.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| file.trim_end_matches(".glb").to_string());
                // Guest Miis are named with just a letter ("C"): call them "Guest C".
                if name.chars().count() == 1 && name.chars().all(|c| c.is_ascii_uppercase()) { name = format!("Guest {}", name); }
                Entry { file, name, model, stamp }
            }
        };
        list.push(e);
    }
    s.list = list;
    s.cache.clear();
    s.order.clear();
    s.latest.clear();
    s.queue.clear();
    s.generation += 1;
    true
}

pub fn names() -> Vec<String> {
    let s = STORE.lock().unwrap();
    s.list.iter().filter(|e| e.model.is_some()).map(|e| e.name.clone()).collect()
}

fn nth_loaded(s: &Store, i: usize) -> Option<usize> { s.list.iter().enumerate().filter(|(_, e)| e.model.is_some()).nth(i).map(|(k, _)| k) }

pub fn file_of(i: usize) -> Option<String> { let s = STORE.lock().unwrap(); nth_loaded(&s, i).map(|k| s.list[k].file.clone()) }

/// Draw Mii `i` with its feet at the bottom of `dst` (never waits for rendering).
pub fn draw(c: &mut Canvas, i: usize, pose: MiiPose, dst: Rect) {
    if !dst.overlaps(&c.clip) || dst.w < 4 || dst.h < 4 { return; }
    start_workers();
    let key: Key = (i, dst.w, dst.h, round(pose));
    let px = {
        let mut s = STORE.lock().unwrap();
        let Some(k) = nth_loaded(&s, i) else { return };
        match s.cache.get(&key) {
            Some(p) => { let p = p.clone(); s.latest.insert((i, dst.w, dst.h), p.clone()); Some(p) }
            None => {
                // Queue it (replacing an older, not-yet-started job for the same Mii and size).
                let model = s.list[k].model.clone().unwrap();
                s.queue.retain(|(qk, _)| !(qk.0 == i && qk.1 == dst.w && qk.2 == dst.h));
                s.queue.push_back((key, model));
                WAKE.notify_one();
                s.latest.get(&(i, dst.w, dst.h)).cloned()
            }
        }
    };
    let Some(px) = px else { return };
    let vis = dst.intersect(&c.clip);
    for y in vis.y..vis.bottom() {
        let row = ((y - dst.y) * dst.w) as usize;
        for x in vis.x..vis.right() {
            let p = px[row + (x - dst.x) as usize];
            let a = p >> 24;
            if a != 0 { c.blend_pub(x, y, 0xFF00_0000 | p, a + 1); }
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
    // ".me" holds the file you picked, or "auto:<file>" when we guessed.
    let saved = std::fs::read_to_string(me_file()).unwrap_or_default();
    let (auto, want) = match saved.trim().strip_prefix("auto:") { Some(f) => (true, f.to_string()), None => (false, saved.trim().to_string()) };
    let picked = loaded.iter().position(|e| !want.is_empty() && e.file == want);
    if let (Some(i), false) = (picked, auto) { return Some(i); }
    // Your own Mii: the one whose name matches who made it.
    let made_by_self = loaded.iter().position(|e| {
        let m: &Model = e.model.as_ref().unwrap();
        m.creator.as_deref().map_or(false, |c| !c.is_empty() && c.eq_ignore_ascii_case(&m.name))
    });
    if made_by_self.is_some() { return made_by_self; }
    if picked.is_some() { return picked; }
    // Only one Mii? Then that's you (remembered, so a new arrival doesn't change it).
    if loaded.len() == 1 { let _ = std::fs::write(me_file(), format!("auto:{}", loaded[0].file)); return Some(0); }
    None
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

// ---- Adding Miis from this computer ----
fn home() -> PathBuf { PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())) }
/// Folders where Mii files usually end up (the web browser saves to Downloads).
fn places() -> Vec<(&'static str, PathBuf)> {
    vec![("Downloads", home().join("Downloads")), ("Desktop", home().join("Desktop")), ("Documents", home().join("Documents")), ("Home", home())]
}
fn glb_files(d: &std::path::Path) -> Vec<(String, PathBuf, u64)> {
    let mut v: Vec<_> = std::fs::read_dir(d).into_iter().flatten().flatten().filter_map(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.to_ascii_lowercase().ends_with(".glb") { return None; }
        let md = e.metadata().ok()?;
        if !md.is_file() || md.len() == 0 { return None; } // the browser makes an empty file before the download finishes
        Some((name, e.path(), md.len()))
    }).collect();
    v.sort();
    v
}
fn seen_file() -> PathBuf { dir().join(".seen") }
fn seen_key(name: &str, len: u64) -> String { format!("{}\t{}", name, len) }
fn mark_seen(key: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(seen_file()) { let _ = writeln!(f, "{}", key); }
}
/// Copy a Mii file into ~/MiiOS/miis (with a new name if one is already there).
fn copy_in(path: &std::path::Path) -> std::io::Result<PathBuf> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Mii.glb".into());
    let stem = name[..name.len() - 4].to_string();
    let mut dst = dir().join(&name);
    let mut k = 2;
    while dst.exists() { dst = dir().join(format!("{} ({}).glb", stem, k)); k += 1; }
    std::fs::copy(path, &dst)?;
    Ok(dst)
}

/// A Mii you download in the web browser (or save to Downloads / Desktop)
/// moves in by itself: each new file is copied in once.
fn auto_import() {
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    {
        let mut l = LAST.lock().unwrap();
        if l.map_or(false, |t| t.elapsed().as_millis() < 1500) { return; }
        *l = Some(std::time::Instant::now());
    }
    let seen = std::fs::read_to_string(seen_file()).unwrap_or_default();
    for (_, d) in places().into_iter().take(2) {
        for (name, path, len) in glb_files(&d) {
            let key = seen_key(&name, len);
            if seen.lines().any(|l| l == key) { continue; }
            // Still being written? Wait until it hasn't changed for a moment.
            let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok()
                .and_then(|t| t.elapsed().ok()).map_or(true, |e| e.as_secs() < 2);
            if fresh { continue; }
            mark_seen(&key);
            if std::fs::metadata(dir().join(&name)).map_or(false, |m| m.len() == len) { continue; } // already here
            let ok = std::fs::read(&path).ok().map_or(false, |b| Model::load(&b).is_ok());
            if ok && copy_in(&path).is_ok() { crate::log!("a new Mii arrived: {}", name); }
        }
    }
}

/// .glb files on this computer you could add: ("Downloads: Lizzi.glb", full path).
pub fn files() -> Vec<(String, String)> {
    let mut v = Vec::new();
    for (place, d) in places() {
        for (name, path, _) in glb_files(&d) { v.push((format!("{}: {}", place, name), path.to_string_lossy().into_owned())); }
    }
    v
}

/// Add one Mii file. Ok("Welcome, Lizzi!") or Err(why not).
pub fn add_file(path: &str) -> Result<String, String> {
    let p = std::path::Path::new(path);
    let bytes = std::fs::read(p).map_err(|e| format!("Couldn't read the file ({})", e))?;
    let m = Model::load(&bytes).map_err(|e| format!("That file isn't a Mii we can show ({})", e))?;
    let len = bytes.len() as u64;
    if let Some(n) = p.file_name() { mark_seen(&seen_key(&n.to_string_lossy(), len)); }
    copy_in(p).map_err(|e| format!("Couldn't copy it ({})", e))?;
    rescan();
    let mut name = m.name.clone();
    if name.chars().count() == 1 { name = format!("Guest {}", name); }
    Ok(format!("Welcome, {}! Say hi on the home screen.", name))
}

// ---- Importing new Miis (USB sticks) ----
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
