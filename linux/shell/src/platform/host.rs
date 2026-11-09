//! What the full Linux underneath gives MiiOS: launching the web browser
//! and other programs, Wi-Fi (through NetworkManager's `nmcli` tool),
//! battery level, and installing MiiOS onto the laptop's drive.
//!
//! Slow things (scanning Wi-Fi, connecting, installing) run in background
//! threads; the UI asks for results once a second, so it never freezes.

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

pub const HOSTED: bool = true;

/// Start (or bring to the front) an outside program, via the
/// `miios-launch` script installed with MiiOS.
pub fn launch(app: &str) -> bool {
    Command::new("miios-launch").arg(app).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok()
}

/// (percent, charging) of the laptop battery, if there is one.
pub fn battery() -> Option<(u8, bool)> {
    for b in ["BAT0", "BAT1"] {
        let base = format!("/sys/class/power_supply/{b}");
        if let Ok(c) = std::fs::read_to_string(format!("{base}/capacity")) {
            let status = std::fs::read_to_string(format!("{base}/status")).unwrap_or_default();
            return Some((c.trim().parse().unwrap_or(0), status.trim() == "Charging" || status.trim() == "Full"));
        }
    }
    None
}

// ---------------------------------------------------------------- Wi-Fi

#[derive(Clone, Debug)]
pub struct Network { pub ssid: String, pub signal: u8, pub secure: bool, pub connected: bool }

static SCAN: Mutex<Option<Vec<Network>>> = Mutex::new(None);
static CONNECT: Mutex<Option<Result<(), String>>> = Mutex::new(None);
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

/// nmcli's "terse" output separates fields with ':' and escapes ':' inside names as '\:'.
fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut esc = false;
    for c in line.chars() {
        if esc { out.last_mut().unwrap().push(c); esc = false; }
        else if c == '\\' { esc = true; }
        else if c == ':' { out.push(String::new()); }
        else { out.last_mut().unwrap().push(c); }
    }
    out
}

fn nmcli(args: &[&str]) -> Result<String, String> {
    let o = Command::new("nmcli").args(args).output().map_err(|_| String::from("NetworkManager isn't installed"))?;
    if o.status.success() { Ok(String::from_utf8_lossy(&o.stdout).into_owned()) }
    else {
        let e = String::from_utf8_lossy(&o.stderr).trim().replace("Error: ", "");
        Err(if e.contains("Secrets were required") || e.contains("802-11-wireless-security") { String::from("wrong password") } else { e })
    }
}

fn refresh_current() {
    let cur = nmcli(&["-t", "-f", "ACTIVE,SSID", "dev", "wifi"]).ok()
        .and_then(|s| s.lines().map(split_terse).find(|f| f.first().map(|a| a == "yes").unwrap_or(false)).and_then(|f| f.get(1).cloned()));
    *CURRENT.lock().unwrap() = cur.filter(|s| !s.is_empty());
}

static WIFI_PROBLEM: Mutex<Option<String>> = Mutex::new(None);

/// Why there might be no networks: no card, Wi-Fi switched off, ... (None = fine).
pub fn wifi_problem() -> Option<String> { WIFI_PROBLEM.lock().unwrap().clone() }

/// Check the Wi-Fi card is there and switched on (turning it on if needed).
fn wifi_ready() -> Result<(), String> {
    let devs = nmcli(&["-t", "-f", "TYPE,STATE", "dev"]).map_err(|e| format!("NetworkManager problem: {e}"))?;
    if !devs.lines().any(|l| l.starts_with("wifi:")) {
        let rf = Command::new("rfkill").arg("list").arg("wifi").output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        if rf.contains("Hard blocked: yes") { return Err("Wi-Fi is switched off by the laptop's hardware switch or Fn key.".into()); }
        return Err("No Wi-Fi card found (the driver may be missing). Try a cable for now.".into());
    }
    if nmcli(&["radio", "wifi"]).map(|s| s.trim() == "disabled").unwrap_or(false) {
        nmcli(&["radio", "wifi", "on"]).map_err(|e| format!("Couldn't turn Wi-Fi on: {e}"))?;
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    if devs.lines().any(|l| l == "wifi:unavailable") {
        return Err("Wi-Fi isn't ready (the Wi-Fi helper may be missing; Settings > Updates installs it).".into());
    }
    Ok(())
}

pub fn wifi_scan_start() {
    std::thread::spawn(|| {
        let ready = wifi_ready();
        *WIFI_PROBLEM.lock().unwrap() = ready.err();
        let out = nmcli(&["-t", "-f", "IN-USE,SSID,SIGNAL,SECURITY", "dev", "wifi", "list", "--rescan", "yes"]).unwrap_or_default();
        let mut nets: Vec<Network> = Vec::new();
        for f in out.lines().map(split_terse) {
            if f.len() < 4 || f[1].is_empty() { continue; } // hidden networks have no name
            let n = Network { ssid: f[1].clone(), signal: f[2].parse().unwrap_or(0), secure: !f[3].is_empty() && f[3] != "--", connected: f[0] == "*" };
            // The same network can appear once per access point: keep the strongest.
            match nets.iter_mut().find(|x| x.ssid == n.ssid) {
                Some(x) => { x.signal = x.signal.max(n.signal); x.connected |= n.connected; }
                None => nets.push(n),
            }
        }
        nets.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
        refresh_current();
        *SCAN.lock().unwrap() = Some(nets);
    });
}

/// The finished scan, once (None while still scanning).
pub fn wifi_networks() -> Option<Vec<Network>> { SCAN.lock().unwrap().take() }

pub fn wifi_connect_start(ssid: &str, password: &str) {
    *CONNECT.lock().unwrap() = None;
    let (ssid, password) = (ssid.to_string(), password.to_string());
    std::thread::spawn(move || {
        let mut args = vec!["dev", "wifi", "connect", ssid.as_str()];
        if !password.is_empty() { args.push("password"); args.push(password.as_str()); }
        let r = nmcli(&args).map(|_| ());
        refresh_current();
        *CONNECT.lock().unwrap() = Some(r);
    });
}

pub fn wifi_connect_result() -> Option<Result<(), String>> { CONNECT.lock().unwrap().take() }

pub fn wifi_current() -> Option<String> {
    // Refresh in the background now and then; answer from the last known value.
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap();
    if last.map_or(true, |t| t.elapsed().as_secs() >= 5) {
        *last = Some(std::time::Instant::now());
        std::thread::spawn(refresh_current);
    }
    CURRENT.lock().unwrap().clone()
}

// ---------------------------------------------------------------- Installer

/// Running from the USB stick? (The image marks itself with this file.)
pub fn live_session() -> bool { std::path::Path::new("/etc/miios/live").exists() }

/// Drives MiiOS could be installed on: (device path, description). Never the
/// drive we're running from, never USB drives.
pub fn install_targets() -> Vec<(String, String)> {
    let root_disk = Command::new("sh").args(["-c", "lsblk -no PKNAME \"$(findmnt -no SOURCE /)\" | head -n1"]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let out = Command::new("lsblk").args(["-dnpo", "NAME,TYPE,TRAN,RM,SIZE,MODEL"]).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut v = Vec::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 || f[1] != "disk" { continue; }
        let (dev, tran, rm, size) = (f[0], f[2], f[3], f[4]);
        if tran == "usb" || rm == "1" || dev.ends_with(&root_disk) && !root_disk.is_empty() { continue; }
        if dev.contains("/zram") || dev.contains("/loop") { continue; }
        let model = f[5..].join(" ");
        v.push((dev.to_string(), format!("{} {} ({})", tran.to_uppercase(), if model.is_empty() { dev } else { &model }, size)));
    }
    v
}

static INSTALL: Mutex<Option<Child>> = Mutex::new(None);
const INSTALL_LOG: &str = "/tmp/miios-install.log";

pub fn install_start(dev: &str) {
    let log = std::fs::File::create(INSTALL_LOG).ok();
    let mut cmd = Command::new("sudo");
    cmd.args(["-n", "/usr/local/sbin/miios-install", dev]).stdin(Stdio::null());
    if let Some(l) = log { cmd.stdout(l.try_clone().map(Stdio::from).unwrap_or(Stdio::null())).stderr(Stdio::from(l)); }
    *INSTALL.lock().unwrap() = cmd.spawn().ok();
}

/// (last progress line, finished?, succeeded?)
pub fn install_progress() -> (String, bool, bool) {
    let line = std::fs::read_to_string(INSTALL_LOG).unwrap_or_default().lines().rev()
        .find(|l| !l.trim().is_empty()).unwrap_or("Starting").trim().to_string();
    let mut g = INSTALL.lock().unwrap();
    match g.as_mut().map(|c| c.try_wait()) {
        Some(Ok(Some(status))) => (line, true, status.success()),
        Some(Ok(None)) => (line, false, false),
        _ => (if line == "Starting" { String::from("Could not start the installer") } else { line }, true, false),
    }
}

// ---------------------------------------------------------------- About

pub fn about_lines() -> Vec<(&'static str, String)> {
    let cpu = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default().lines()
        .find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).unwrap_or("?").trim().to_string();
    let (total, used) = super::mem::frame::stats();
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim().to_string();
    let up = std::fs::read_to_string("/proc/uptime").unwrap_or_default().split_whitespace().next().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0) as u64;
    let bat = match battery() { Some((p, ch)) => format!("{}%{}", p, if ch { ", charging" } else { "" }), None => String::from("no battery") };
    vec![
        ("Processor", cpu),
        ("Memory", format!("{} MiB, {} MiB in use", total >> 20, used >> 20)),
        ("Battery", bat),
        ("Wi-Fi", wifi_current().unwrap_or_else(|| String::from("not connected"))),
        ("Running", String::from(if live_session() { "from the USB stick" } else { "from this laptop's drive" })),
        ("Uptime", format!("{} h {} min", up / 3600, up / 60 % 60)),
        ("Your files", String::from("~/MiiOS")),
        ("Underneath", format!("Debian Linux {}", kernel)),
        ("Built with", String::from("Rust, Sway, Firefox, NetworkManager")),
    ]
}

// ---------------------------------------------------------------- Icons

pub struct Image { pub w: u32, pub h: u32, pub px: &'static [u32] }

fn load_png(path: &str) -> Option<Image> {
    let dec = png::Decoder::new(std::fs::File::open(path).ok()?);
    let mut dec = dec;
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()];
    let info = r.next_frame(&mut buf).ok()?;
    let ch = match info.color_type { png::ColorType::Rgba => 4, png::ColorType::Rgb => 3, png::ColorType::GrayscaleAlpha => 2, _ => 1 };
    let px: Vec<u32> = buf[..info.buffer_size()].chunks(ch).map(|p| {
        let (r, g, b, a) = match ch { 4 => (p[0], p[1], p[2], p[3]), 3 => (p[0], p[1], p[2], 255), 2 => (p[0], p[0], p[0], p[1]), _ => (p[0], p[0], p[0], 255) };
        (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
    }).collect();
    Some(Image { w: info.width, h: info.height, px: Box::leak(px.into_boxed_slice()) })
}

/// The icon of an installed app (from /opt/miios/apps/NAME/icon.png), loaded once.
pub fn app_icon(name: &str) -> Option<&'static Image> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<&'static Image>>>> = OnceLock::new();
    let mut c = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap();
    let updated = format!("{}/.local/share/miios/apps/{name}/icon.png", std::env::var("HOME").unwrap_or_default());
    *c.entry(name.to_string()).or_insert_with(|| load_png(&updated).or_else(|| load_png(&format!("/opt/miios/apps/{name}/icon.png"))).map(|i| &*Box::leak(Box::new(i))))
}

// ---------------------------------------------------------------- Your Miis

/// How many joint "dials" a Mii pose has (see `j`).
pub const NJ: usize = 18;

/// Names for the joint dials. Values are angles in 1/256 of a full turn
/// (64 = a right angle), positive = the natural direction named here.
pub mod j {
    pub const SPINE_BEND: usize = 0;   // lean forward
    pub const SPINE_TWIST: usize = 1;  // turn the upper body to its left
    pub const SPINE_TILT: usize = 2;   // lean to its right
    pub const HEAD_NOD: usize = 3;     // look down
    pub const HEAD_TURN: usize = 4;    // look to its left
    pub const HEAD_TILT: usize = 5;    // tilt to its right
    pub const ARM_L_SWING: usize = 6;  // left arm forward (and up, at large angles)
    pub const ARM_L_RAISE: usize = 7;  // left arm out to the side
    pub const ELBOW_L: usize = 8;
    pub const ARM_R_SWING: usize = 9;
    pub const ARM_R_RAISE: usize = 10;
    pub const ELBOW_R: usize = 11;
    pub const LEG_L_SWING: usize = 12; // left leg forward
    pub const KNEE_L: usize = 13;      // bend the left knee
    pub const LEG_R_SWING: usize = 14;
    pub const KNEE_R: usize = 15;
    pub const LEG_L_OUT: usize = 16;   // left leg out to the side
    pub const LEG_R_OUT: usize = 17;
}

/// How a Mii is posed. Angles are in 1/256 of a full turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MiiPose {
    /// Facing: 0 = towards you, 64 = to the right, 128 = away, 192 = to the left.
    pub yaw: i32,
    /// Whole-body flip forwards (for somersaults) and roll sideways (cartwheels).
    pub pitch: i32,
    pub roll: i32,
    /// Squash and stretch: 256 = normal, 300 = tall and thin, 200 = short and wide.
    pub squash: i32,
    pub j: [i8; NJ],
}

impl Default for MiiPose {
    fn default() -> MiiPose { MiiPose { yaw: 0, pitch: 0, roll: 0, squash: 256, j: [0; NJ] } }
}

pub fn miis() -> Vec<String> {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| { super::miis::rescan(); });
    super::miis::names()
}
pub fn miis_rescan() -> bool { super::miis::rescan() }
pub fn miis_rendering() -> bool { super::miis::rendering() }
pub fn mii_draw(c: &mut crate::gfx::Canvas, i: usize, pose: MiiPose, dst: crate::gfx::Rect) { super::miis::draw(c, i, pose, dst) }
pub fn mii_me() -> Option<usize> { super::miis::me() }
pub fn mii_set_me(i: usize) { super::miis::set_me(i) }
pub fn mii_remove(i: usize) { super::miis::remove(i) }
pub fn mii_import_start() { super::miis::import_start() }
pub fn mii_import_result() -> Option<String> { super::miis::import_result() }
pub fn mii_files() -> Vec<(String, String)> { super::miis::files() }
pub fn mii_add_file(path: &str) -> Result<String, String> { super::miis::add_file(path) }

// ---------------------------------------------------------------- Updates

#[derive(Clone, Debug, Default)]
pub struct UpdateInfo { pub system: u32, pub miios_have: String, pub miios_new: Option<String>, pub miios_reached: bool }

static UPDATER: Mutex<Option<Child>> = Mutex::new(None);
const UPDATE_LOG: &str = "/tmp/miios-update-ui.log";

/// Run the updater (as root, via the sudo rule) in the background.
fn run_updater(arg: &str) {
    let mut g = UPDATER.lock().unwrap();
    if let Some(c) = g.as_mut() { if matches!(c.try_wait(), Ok(None)) { return; } } // already running
    let Ok(log) = std::fs::File::create(UPDATE_LOG) else { return };
    let mut cmd = Command::new("sudo");
    cmd.args(["-n", "/usr/local/sbin/miios-update", arg]).stdin(Stdio::null());
    if let Ok(l2) = log.try_clone() { cmd.stdout(Stdio::from(l2)); }
    cmd.stderr(Stdio::from(log));
    *g = cmd.spawn().ok();
}

pub fn updates_check_start() { run_updater("--check") }
pub fn updates_install_start() { run_updater("--install") }
pub fn updates_busy() -> bool {
    UPDATER.lock().unwrap().as_mut().map_or(false, |c| matches!(c.try_wait(), Ok(None)))
}
pub fn updates_line() -> String {
    std::fs::read_to_string(UPDATE_LOG).unwrap_or_default().lines().rev()
        .find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}
pub fn updates_info() -> Option<UpdateInfo> {
    let s = std::fs::read_to_string("/var/lib/miios/update/status").ok()?;
    let get = |k: &str| s.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or("").to_string();
    if get("system").is_empty() { return None; }
    let new = get("miios_new");
    Some(UpdateInfo {
        system: get("system").parse().unwrap_or(0),
        miios_have: std::fs::read_to_string("/etc/miios/version").map(|v| v.trim().to_string()).unwrap_or_else(|_| get("miios_have")),
        miios_new: if new.is_empty() { None } else { Some(new) },
        miios_reached: get("miios_reached") != "0",
    })
}
pub fn restart_needed() -> bool { std::path::Path::new("/run/miios-restart-needed").exists() || std::path::Path::new("/var/run/reboot-required").exists() }
pub fn auto_updates() -> bool { std::fs::read_to_string("/etc/miios/auto-updates").map(|s| s.trim() != "off").unwrap_or(true) }
pub fn set_auto_updates(on: bool) {
    let _ = Command::new("sudo").args(["-n", "/usr/local/sbin/miios-update", "--set-auto", if on { "on" } else { "off" }]).status();
}

/// Little sound effects (made up from tones by the Linux edition; silent on bare metal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx { Hover, Type, Open, Back, Jump, Land, Chirp, Pop, Sparkle, Click, Whoosh, Cheer, Step, Snore }
pub fn sound(s: Sfx, pitch: i32) { super::audio::play(s, pitch as f32 / 256.0) }
pub fn music_on() -> bool { super::audio::music_on() }
pub fn set_music(on: bool) { super::audio::set_music(on) }
pub fn sounds_on() -> bool { super::audio::sounds_on() }
pub fn set_sounds(on: bool) { super::audio::set_sounds(on) }

static VOLUME: Mutex<(Option<u8>, Option<std::time::Instant>)> = Mutex::new((None, None));
/// System volume (asks the `miios-volume` helper, at most every 2 seconds).
pub fn volume() -> Option<u8> {
    let mut g = VOLUME.lock().unwrap();
    if g.1.map_or(true, |t| t.elapsed().as_secs() >= 2) {
        g.0 = Command::new("miios-volume").arg("get").output().ok()
            .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok());
        g.1 = Some(std::time::Instant::now());
    }
    g.0
}
pub fn change_volume(delta: i32) {
    let arg = if delta >= 0 { "up" } else { "down" };
    let _ = Command::new("miios-volume").arg(arg).status();
    VOLUME.lock().unwrap().1 = None;
}

pub fn presented(rects: &[crate::gfx::Rect]) { super::window::damaged(rects) }
