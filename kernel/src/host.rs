//! Things only an OS with a full Linux underneath can do (web browser,
//! Wi-Fi, battery, installing to disk). On bare metal they're all "not
//! available"; the Linux edition provides real versions of this module.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// False here: we're the bare-metal kernel.
pub const HOSTED: bool = false;

pub fn launch(_app: &str) -> bool { false }
pub fn battery() -> Option<(u8, bool)> { None }

#[derive(Clone, Debug)]
pub struct Network { pub ssid: String, pub signal: u8, pub secure: bool, pub connected: bool }
pub fn wifi_scan_start() {}
pub fn wifi_networks() -> Option<Vec<Network>> { None }
pub fn wifi_connect_start(_ssid: &str, _password: &str) {}
/// None = still working, Some(Ok) = connected, Some(Err(msg)) = failed.
pub fn wifi_connect_result() -> Option<Result<(), String>> { Some(Err(String::from("no Wi-Fi driver"))) }
pub fn wifi_current() -> Option<String> { None }

pub fn live_session() -> bool { false }
pub fn install_targets() -> Vec<(String, String)> { Vec::new() }
pub fn install_start(_dev: &str) {}
pub fn install_progress() -> (String, bool, bool) { (String::new(), false, false) }

pub fn about_lines() -> Vec<(&'static str, String)> { Vec::new() }

/// Pictures for app tiles (ARGB pixels), when available.
pub struct Image { pub w: u32, pub h: u32, pub px: &'static [u32] }
pub fn app_icon(_name: &str) -> Option<&'static Image> { None }

// ---- Your 3D Miis (.glb files). Only the Linux edition can draw these. ----

/// How a Mii is posed. Angles/phases are in 1/256 of a turn/cycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MiiPose { pub yaw: i32, pub walk: Option<i32>, pub wave: Option<i32> }

/// Names of your Miis (in a stable order).
pub fn miis() -> Vec<String> { Vec::new() }
/// Re-read the Miis folder; true if something changed.
pub fn miis_rescan() -> bool { false }
/// Draw Mii `i` into `dst` (feet at the bottom edge).
pub fn mii_draw(_c: &mut crate::gfx::Canvas, _i: usize, _pose: MiiPose, _dst: crate::gfx::Rect) {}
/// Which Mii is "you" (for greetings).
pub fn mii_me() -> Option<usize> { None }
pub fn mii_set_me(_i: usize) {}
pub fn mii_remove(_i: usize) {}
/// Look for new Miis (Downloads folder and USB sticks), in the background.
pub fn mii_import_start() {}
/// None = still looking, Some(message) = finished.
pub fn mii_import_result() -> Option<String> { None }

// ---- Updates (Linux edition) ----

#[derive(Clone, Debug, Default)]
pub struct UpdateInfo {
    /// Linux/system packages waiting to be installed.
    pub system: u32,
    /// The MiiOS build running now ("dev" for home-made builds).
    pub miios_have: String,
    /// A newer MiiOS build, if there is one.
    pub miios_new: Option<String>,
    /// False if MiiOS's own updates couldn't be checked (offline, or a private repository).
    pub miios_reached: bool,
}
pub fn updates_check_start() {}
pub fn updates_install_start() {}
pub fn updates_busy() -> bool { false }
/// The updater's latest progress message.
pub fn updates_line() -> String { String::new() }
/// Result of the last check (None = never checked).
pub fn updates_info() -> Option<UpdateInfo> { None }
pub fn restart_needed() -> bool { false }
pub fn auto_updates() -> bool { false }
pub fn set_auto_updates(_on: bool) {}
