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

pub fn wifi_scan_start() {
    std::thread::spawn(|| {
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

/// How a Mii is posed. Angles/phases are in 1/256 of a turn/cycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MiiPose { pub yaw: i32, pub walk: Option<i32>, pub wave: Option<i32> }

pub fn miis() -> Vec<String> {
    static FIRST: std::sync::Once = std::sync::Once::new();
    FIRST.call_once(|| { super::miis::rescan(); });
    super::miis::names()
}
pub fn miis_rescan() -> bool { super::miis::rescan() }
pub fn mii_draw(c: &mut crate::gfx::Canvas, i: usize, pose: MiiPose, dst: crate::gfx::Rect) { super::miis::draw(c, i, pose, dst) }
pub fn mii_me() -> Option<usize> { super::miis::me() }
pub fn mii_set_me(i: usize) { super::miis::set_me(i) }
pub fn mii_remove(i: usize) { super::miis::remove(i) }
pub fn mii_import_start() { super::miis::import_start() }
pub fn mii_import_result() -> Option<String> { super::miis::import_result() }

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
