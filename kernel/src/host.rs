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
pub fn wifi_problem() -> Option<String> { None }

pub fn live_session() -> bool { false }
pub fn install_targets() -> Vec<(String, String)> { Vec::new() }
pub fn install_start(_dev: &str) {}
pub fn install_progress() -> (String, bool, bool) { (String::new(), false, false) }

pub fn about_lines() -> Vec<(&'static str, String)> { Vec::new() }

/// Pictures for app tiles (ARGB pixels), when available.
pub struct Image { pub w: u32, pub h: u32, pub px: &'static [u32] }
pub fn app_icon(_name: &str) -> Option<&'static Image> { None }

// ---- Your 3D Miis (.glb files). Only the Linux edition can draw these. ----

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

/// Names of your Miis (in a stable order).
pub fn miis() -> Vec<String> { Vec::new() }
/// True while some Mii pictures are still being drawn in the background.
pub fn miis_rendering() -> bool { false }
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
/// .glb Mii files on this computer that could be added: (label to show, full path).
pub fn mii_files() -> Vec<(String, String)> { Vec::new() }
/// Add the Mii file at `path`. Ok(message) or Err(what went wrong).
pub fn mii_add_file(_path: &str) -> Result<String, String> { Err(String::from("not available")) }

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

/// Little sound effects (made up from tones by the Linux edition; silent on bare metal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sfx { Hover, Type, Open, Back, Jump, Land, Chirp, Pop, Sparkle, Click, Whoosh, Cheer, Step, Snore }
/// Play a sound; `pitch` 256 = normal, 512 = an octave higher.
pub fn sound(_s: Sfx, _pitch: i32) {}
pub fn music_on() -> bool { false }
pub fn set_music(_on: bool) {}
pub fn sounds_on() -> bool { false }
pub fn set_sounds(_on: bool) {}
/// System volume 0..=100 (None = no sound system).
pub fn volume() -> Option<u8> { None }
pub fn change_volume(_delta: i32) {}

/// The screen code copied these rectangles to the display buffer (Linux edition: forward to Wayland).
pub fn presented(_rects: &[crate::gfx::Rect]) {}
