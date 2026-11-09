//! The Mii move library: 80 animations made from joint "dials".
//!
//! A *move* is a little recipe: given the time since it started, it sets the
//! joint dials (see `host::j`) using sine waves, plus whole-body extras:
//! lifting off the ground, flipping, rolling, squash & stretch. Everything is
//! integer maths (it also builds into the bare-metal kernel), angles are given
//! in degrees with `d()` for readability.
//!
//! Timing is in frames at 60 per second. Slow, smooth sines keep things calm;
//! a little squash on landings and anticipations make them cartoony.

use crate::gfx::math::sin;
use crate::host::{j::*, MiiPose, NJ};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mv {
    // ---- standing and idle (1-15)
    Stand, LookAround, Stretch, Yawn, ScratchHead, TapFoot, Shrug, Nod, HeadTilt, Sigh,
    Shiver, Sneeze, CheckWatch, Sway, BounceIdle,
    // ---- feelings (16-33)
    Wave, BigWave, Cheer, Clap, Laugh, Bow, Salute, ThumbsUp, Facepalm, HeartHands,
    Stomp, Point, Think, Cry, Surprised, Scared, Shy, Proud,
    // ---- dancing (34-43)
    DanceBounce, Disco, Robot, Twist, ChickenDance, Spin, Floss, JumpingJacks, Conga, SwayDance,
    // ---- acrobatics (44-49)
    Cartwheel, Backflip, Frontflip, Handstand, TwirlJump, Somersault,
    // ---- getting around (50-65)
    Walk, Run, Tiptoe, Skip, Hop, March, Moonwalk, Climb, Hang, SitEdge, Nap,
    Fall, Land, Crouch, Jump, Carried,
    // ---- playing and doing things (66-80)
    Dizzy, Push, Kick, Throw, Catch, Juggle, HoldBalloon, Water, Plant, Doze,
    Hug, HighFive, Whisper, Sing, Squats,
}

impl Mv {
    /// All moves, for the README and for picking at random.
    pub const ALL: [Mv; 80] = [
        Mv::Stand, Mv::LookAround, Mv::Stretch, Mv::Yawn, Mv::ScratchHead, Mv::TapFoot, Mv::Shrug, Mv::Nod, Mv::HeadTilt, Mv::Sigh,
        Mv::Shiver, Mv::Sneeze, Mv::CheckWatch, Mv::Sway, Mv::BounceIdle,
        Mv::Wave, Mv::BigWave, Mv::Cheer, Mv::Clap, Mv::Laugh, Mv::Bow, Mv::Salute, Mv::ThumbsUp, Mv::Facepalm, Mv::HeartHands,
        Mv::Stomp, Mv::Point, Mv::Think, Mv::Cry, Mv::Surprised, Mv::Scared, Mv::Shy, Mv::Proud,
        Mv::DanceBounce, Mv::Disco, Mv::Robot, Mv::Twist, Mv::ChickenDance, Mv::Spin, Mv::Floss, Mv::JumpingJacks, Mv::Conga, Mv::SwayDance,
        Mv::Cartwheel, Mv::Backflip, Mv::Frontflip, Mv::Handstand, Mv::TwirlJump, Mv::Somersault,
        Mv::Walk, Mv::Run, Mv::Tiptoe, Mv::Skip, Mv::Hop, Mv::March, Mv::Moonwalk, Mv::Climb, Mv::Hang, Mv::SitEdge, Mv::Nap,
        Mv::Fall, Mv::Land, Mv::Crouch, Mv::Jump, Mv::Carried,
        Mv::Dizzy, Mv::Push, Mv::Kick, Mv::Throw, Mv::Catch, Mv::Juggle, Mv::HoldBalloon, Mv::Water, Mv::Plant, Mv::Doze,
        Mv::Hug, Mv::HighFive, Mv::Whisper, Mv::Sing, Mv::Squats,
    ];

    /// How long the move lasts by default (frames), and whether it repeats.
    pub fn length(self) -> i32 {
        match self {
            Mv::Stand | Mv::Walk | Mv::Run | Mv::Tiptoe | Mv::Skip | Mv::Hop | Mv::March | Mv::Moonwalk | Mv::Climb
            | Mv::Hang | Mv::SitEdge | Mv::Nap | Mv::Fall | Mv::Jump | Mv::Carried | Mv::HoldBalloon | Mv::Doze => 120,
            Mv::Land | Mv::Crouch => 14,
            Mv::Nod | Mv::Shrug | Mv::Surprised | Mv::ThumbsUp | Mv::Point => 60,
            Mv::Sneeze | Mv::Bow | Mv::Salute | Mv::HighFive | Mv::Kick | Mv::Throw | Mv::Catch => 75,
            Mv::Backflip | Mv::Frontflip | Mv::TwirlJump => 70,
            Mv::Cartwheel | Mv::Somersault => 80,
            Mv::Wave => 150, Mv::BigWave => 130,
            _ => 160,
        }
    }

    /// Readable name (for the README / About page).
    pub fn name(self) -> &'static str {
        match self {
            Mv::Stand => "stand and breathe", Mv::LookAround => "look around", Mv::Stretch => "big stretch", Mv::Yawn => "yawn",
            Mv::ScratchHead => "scratch head", Mv::TapFoot => "tap foot", Mv::Shrug => "shrug", Mv::Nod => "nod", Mv::HeadTilt => "curious head tilt",
            Mv::Sigh => "sigh", Mv::Shiver => "shiver", Mv::Sneeze => "sneeze", Mv::CheckWatch => "check the time", Mv::Sway => "sway",
            Mv::BounceIdle => "bouncy idle", Mv::Wave => "wave", Mv::BigWave => "two-arm wave", Mv::Cheer => "cheer", Mv::Clap => "clap",
            Mv::Laugh => "laugh", Mv::Bow => "bow", Mv::Salute => "salute", Mv::ThumbsUp => "thumbs up", Mv::Facepalm => "facepalm",
            Mv::HeartHands => "heart hands", Mv::Stomp => "angry stomp", Mv::Point => "point", Mv::Think => "think", Mv::Cry => "cry",
            Mv::Surprised => "surprised jump", Mv::Scared => "scared crouch", Mv::Shy => "shy", Mv::Proud => "proud hands on hips",
            Mv::DanceBounce => "bounce dance", Mv::Disco => "disco point", Mv::Robot => "robot", Mv::Twist => "the twist",
            Mv::ChickenDance => "chicken dance", Mv::Spin => "spin", Mv::Floss => "arm swing dance", Mv::JumpingJacks => "jumping jacks",
            Mv::Conga => "conga", Mv::SwayDance => "slow dance", Mv::Cartwheel => "cartwheel", Mv::Backflip => "backflip",
            Mv::Frontflip => "front flip", Mv::Handstand => "handstand", Mv::TwirlJump => "twirl jump", Mv::Somersault => "somersault",
            Mv::Walk => "walk", Mv::Run => "run", Mv::Tiptoe => "tiptoe", Mv::Skip => "skip", Mv::Hop => "bunny hop", Mv::March => "march",
            Mv::Moonwalk => "moonwalk", Mv::Climb => "climb", Mv::Hang => "hang from a ledge", Mv::SitEdge => "sit and swing legs",
            Mv::Nap => "nap", Mv::Fall => "fall", Mv::Land => "land", Mv::Crouch => "crouch", Mv::Jump => "jump", Mv::Carried => "dangle (carried)",
            Mv::Dizzy => "dizzy", Mv::Push => "push", Mv::Kick => "kick", Mv::Throw => "throw", Mv::Catch => "catch", Mv::Juggle => "juggle",
            Mv::HoldBalloon => "hold a balloon", Mv::Water => "water a flower", Mv::Plant => "plant something", Mv::Doze => "doze off",
            Mv::Hug => "hug", Mv::HighFive => "high five", Mv::Whisper => "whisper", Mv::Sing => "sing", Mv::Squats => "squats",
        }
    }

    /// Moves that travel (the plaza moves the Mii while they play).
    pub fn travels(self) -> bool { matches!(self, Mv::Walk | Mv::Run | Mv::Tiptoe | Mv::Skip | Mv::Hop | Mv::March | Mv::Moonwalk | Mv::Conga | Mv::Cartwheel) }
}

/// One frame of a move.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub pose: MiiPose,
    /// Lift off the ground, in 1/256 of the Mii's height (jumps, bounces).
    pub lift: i32,
    /// Extra spin added to the Mii's facing (1/256 turn).
    pub spin: i32,
}

/// Degrees -> dial units (1/256 of a turn).
const fn d(deg: i32) -> i32 { deg * 256 / 360 }
/// A sine wave: `period` frames per cycle, `offset` in 1/256 cycle, result -256..256.
fn wave(t: i32, period: i32, offset: i32) -> i32 { sin(t * 256 / period.max(1) + offset) }
/// amp * wave / 256
fn sw(amp: i32, t: i32, period: i32, offset: i32) -> i32 { amp * wave(t, period, offset) / 256 }
/// 0..256 progress through the move.
fn prog(t: i32, len: i32) -> i32 { (t * 256 / len.max(1)).clamp(0, 256) }
/// A smooth bump 0 -> 256 -> 0 over the move.
fn bump(t: i32, len: i32) -> i32 { sin(prog(t, len) / 2) }
/// Ease in at the start and out at the end (frames), so moves blend nicely.
fn ease(t: i32, len: i32, edge: i32) -> i32 { (t.min(len - t).max(0) * 256 / edge.max(1)).min(256) }

/// Evaluate move `m` at frame `t` (of `len`).
pub fn eval(m: Mv, t: i32, len: i32) -> Frame {
    let mut j = [0i32; NJ];
    let (mut lift, mut spin, mut pitch, mut roll, mut squash) = (0, 0, 0, 0, 256);
    // arms hang a little away from the body, elbows soft: the "rest" pose
    j[ARM_L_RAISE] = d(6); j[ARM_R_RAISE] = d(6); j[ELBOW_L] = d(10); j[ELBOW_R] = d(10);
    let e = ease(t, len, 12); // 0..256 fade at the edges of non-looping moves
    let fe = |v: i32| v * e / 256;

    match m {
        Mv::Stand => { j[SPINE_BEND] = sw(d(2), t, 180, 0); squash = 256 + sw(3, t, 180, 0); }
        Mv::LookAround => { j[HEAD_TURN] = fe(sw(d(40), t, 120, 0)); j[SPINE_TWIST] = fe(sw(d(10), t, 120, 0)); }
        Mv::Stretch => {
            let b = bump(t, len);
            j[ARM_L_SWING] = d(160) * b / 256; j[ARM_R_SWING] = d(160) * b / 256; j[SPINE_BEND] = -d(12) * b / 256;
            j[HEAD_NOD] = -d(15) * b / 256; squash = 256 + 18 * b / 256;
        }
        Mv::Yawn => {
            let b = bump(t, len);
            j[ARM_R_SWING] = d(95) * b / 256; j[ELBOW_R] = d(130) * b / 256; j[ARM_R_RAISE] = -d(20) * b / 256;
            j[HEAD_NOD] = -d(25) * b / 256; j[SPINE_BEND] = -d(6) * b / 256;
        }
        Mv::ScratchHead => {
            j[ARM_R_SWING] = fe(d(150)); j[ELBOW_R] = fe(d(100) + sw(d(15), t, 14, 0)); j[HEAD_TILT] = fe(d(12));
        }
        Mv::TapFoot => { j[LEG_R_SWING] = d(8); j[KNEE_R] = d(10) + sw(d(8), t, 20, 0).max(0); j[HEAD_TILT] = fe(d(6)); }
        Mv::Shrug => {
            let b = bump(t, len);
            j[ARM_L_RAISE] = d(30) * b / 256; j[ARM_R_RAISE] = d(30) * b / 256; j[ELBOW_L] = d(90) * b / 256; j[ELBOW_R] = d(90) * b / 256;
            j[ARM_L_SWING] = d(20) * b / 256; j[ARM_R_SWING] = d(20) * b / 256; j[HEAD_TILT] = d(12) * b / 256; squash = 256 - 10 * b / 256;
        }
        Mv::Nod => { j[HEAD_NOD] = fe(sw(d(14), t, 30, 0).max(0)); }
        Mv::HeadTilt => { j[HEAD_TILT] = fe(d(22)); j[HEAD_TURN] = fe(d(10)); }
        Mv::Sigh => { let b = bump(t, len); j[SPINE_BEND] = d(14) * b / 256; j[HEAD_NOD] = d(18) * b / 256; squash = 256 - 14 * b / 256; }
        Mv::Shiver => { j[SPINE_TWIST] = sw(d(4), t, 4, 0); j[ARM_L_RAISE] = -d(10); j[ARM_R_RAISE] = -d(10); j[ELBOW_L] = d(70); j[ELBOW_R] = d(70); j[ARM_L_SWING] = d(30); j[ARM_R_SWING] = d(30); }
        Mv::Sneeze => {
            let p = prog(t, len);
            let back = if p < 160 { d(25) * p / 160 } else { 0 };
            let fwd = if p >= 160 { d(30) * (256 - p) / 96 } else { 0 };
            j[HEAD_NOD] = fwd - back; j[SPINE_BEND] = fwd / 2 - back / 3;
            j[ARM_R_SWING] = d(80) * bump(t, len) / 256; j[ELBOW_R] = d(120) * bump(t, len) / 256;
        }
        Mv::CheckWatch => { j[ARM_L_SWING] = fe(d(70)); j[ELBOW_L] = fe(d(95)); j[ARM_L_RAISE] = fe(-d(15)); j[HEAD_NOD] = fe(d(22)); j[HEAD_TURN] = fe(d(15)); }
        Mv::Sway => { j[SPINE_TILT] = sw(d(8), t, 120, 0); j[HEAD_TILT] = sw(d(6), t, 120, 64); }
        Mv::BounceIdle => { let b = wave(t, 40, 0).abs(); j[KNEE_L] = d(15) * b / 256; j[KNEE_R] = d(15) * b / 256; squash = 256 - 10 * b / 256; }

        Mv::Wave => {
            // A gentle wave: lift the arm, then sway the forearm slowly (about once a second).
            // Arm up and out beside the head, swaying gently side to side.
            j[ARM_R_RAISE] = fe(d(100) + sw(d(16), t, 64, 0)); j[ARM_R_SWING] = fe(d(12));
            j[ELBOW_R] = fe(d(20)); j[HEAD_TILT] = fe(d(8)); j[SPINE_TILT] = fe(-d(3));
        }
        Mv::BigWave => {
            j[ARM_L_SWING] = fe(d(160)); j[ARM_R_SWING] = fe(d(160));
            j[ARM_L_RAISE] = fe(d(20) + sw(d(18), t, 44, 0)); j[ARM_R_RAISE] = fe(d(20) - sw(d(18), t, 44, 0));
            lift = fe(sw(8, t, 44, 0).max(0));
        }
        Mv::Cheer => {
            let hop = wave(t, 30, 0).max(0);
            j[ARM_L_SWING] = fe(d(165)); j[ARM_R_SWING] = fe(d(165)); j[ARM_L_RAISE] = fe(d(25)); j[ARM_R_RAISE] = fe(d(25));
            lift = fe(30 * hop / 256); j[KNEE_L] = d(20) * (256 - hop) / 256; j[KNEE_R] = j[KNEE_L]; squash = 256 + 16 * hop / 256;
        }
        Mv::Clap => {
            let c = wave(t, 18, 0);
            j[ARM_L_SWING] = fe(d(55)); j[ARM_R_SWING] = fe(d(55));
            j[ARM_L_RAISE] = fe(-d(12) + d(16) * c / 256); j[ARM_R_RAISE] = fe(-d(12) + d(16) * c / 256);
            j[ELBOW_L] = fe(d(55)); j[ELBOW_R] = fe(d(55));
        }
        Mv::Laugh => {
            j[SPINE_BEND] = fe(-d(10) + sw(d(6), t, 10, 0)); j[HEAD_NOD] = fe(-d(18));
            j[ARM_L_SWING] = fe(d(35)); j[ARM_R_SWING] = fe(d(35)); j[ELBOW_L] = fe(d(80)); j[ELBOW_R] = fe(d(80)); j[ARM_L_RAISE] = fe(-d(15)); j[ARM_R_RAISE] = fe(-d(15));
            squash = 256 + sw(8, t, 10, 0);
        }
        Mv::Bow => { let b = bump(t, len); j[SPINE_BEND] = d(50) * b / 256; j[HEAD_NOD] = d(20) * b / 256; j[ARM_L_SWING] = -d(10) * b / 256; j[ARM_R_SWING] = -d(10) * b / 256; }
        Mv::Salute => { j[ARM_R_SWING] = fe(d(120)); j[ARM_R_RAISE] = fe(d(45)); j[ELBOW_R] = fe(d(125)); j[SPINE_BEND] = fe(-d(4)); }
        Mv::ThumbsUp => { j[ARM_R_SWING] = fe(d(70)); j[ELBOW_R] = fe(d(45)); j[HEAD_TILT] = fe(d(10)); lift = fe(6 * bump(t, len) / 256); }
        Mv::Facepalm => { j[ARM_R_SWING] = fe(d(105)); j[ELBOW_R] = fe(d(135)); j[ARM_R_RAISE] = fe(-d(15)); j[HEAD_NOD] = fe(d(22)); j[SPINE_BEND] = fe(d(8)); }
        Mv::HeartHands => { j[ARM_L_SWING] = fe(d(150)); j[ARM_R_SWING] = fe(d(150)); j[ELBOW_L] = fe(d(70)); j[ELBOW_R] = fe(d(70)); j[ARM_L_RAISE] = fe(-d(10)); j[ARM_R_RAISE] = fe(-d(10)); j[HEAD_TILT] = fe(sw(d(10), t, 60, 0)); }
        Mv::Stomp => {
            let s = wave(t, 16, 0);
            j[LEG_L_SWING] = d(20) * s.max(0) / 256; j[KNEE_L] = d(40) * s.max(0) / 256;
            j[LEG_R_SWING] = d(20) * (-s).max(0) / 256; j[KNEE_R] = d(40) * (-s).max(0) / 256;
            j[ARM_L_RAISE] = fe(d(25)); j[ARM_R_RAISE] = fe(d(25)); j[ARM_L_SWING] = fe(-d(15)); j[ARM_R_SWING] = fe(-d(15)); j[SPINE_BEND] = fe(d(10));
        }
        Mv::Point => { j[ARM_R_SWING] = fe(d(90)); j[ARM_R_RAISE] = fe(d(10)); j[ELBOW_R] = 0; j[HEAD_TURN] = fe(-d(10)); }
        Mv::Think => { j[ARM_R_SWING] = fe(d(75)); j[ELBOW_R] = fe(d(140)); j[ARM_R_RAISE] = fe(-d(10)); j[ARM_L_SWING] = fe(d(30)); j[ELBOW_L] = fe(d(90)); j[HEAD_TILT] = fe(d(14)); j[HEAD_NOD] = fe(-d(8)); }
        Mv::Cry => {
            j[ARM_L_SWING] = fe(d(110)); j[ARM_R_SWING] = fe(d(110)); j[ELBOW_L] = fe(d(140)); j[ELBOW_R] = fe(d(140));
            j[HEAD_NOD] = fe(d(25)); j[SPINE_BEND] = fe(d(12) + sw(d(3), t, 12, 0));
        }
        Mv::Surprised => {
            let p = prog(t, len);
            let up = if p < 80 { p * 256 / 80 } else { (256 - p) * 256 / 176 };
            j[ARM_L_RAISE] = d(70) * up / 256; j[ARM_R_RAISE] = d(70) * up / 256; j[ELBOW_L] = d(60) * up / 256; j[ELBOW_R] = d(60) * up / 256;
            lift = 26 * up / 256; squash = 256 + 30 * up / 256; j[HEAD_NOD] = -d(10) * up / 256;
        }
        Mv::Scared => {
            j[KNEE_L] = fe(d(45)); j[KNEE_R] = fe(d(45)); j[LEG_L_SWING] = fe(d(20)); j[LEG_R_SWING] = fe(d(20)); j[SPINE_BEND] = fe(d(18) + sw(d(3), t, 6, 0));
            j[ARM_L_SWING] = fe(d(80)); j[ARM_R_SWING] = fe(d(80)); j[ELBOW_L] = fe(d(110)); j[ELBOW_R] = fe(d(110)); squash = 256 - 20 * e / 256;
        }
        Mv::Shy => { j[ARM_L_SWING] = fe(-d(25)); j[ARM_R_SWING] = fe(-d(25)); j[ELBOW_L] = fe(d(40)); j[ELBOW_R] = fe(d(40)); j[HEAD_NOD] = fe(d(18)); j[HEAD_TILT] = fe(d(12)); j[LEG_R_SWING] = fe(sw(d(8), t, 50, 0)); }
        Mv::Proud => { j[ARM_L_RAISE] = fe(d(35)); j[ARM_R_RAISE] = fe(d(35)); j[ELBOW_L] = fe(d(100)); j[ELBOW_R] = fe(d(100)); j[ARM_L_SWING] = fe(-d(15)); j[ARM_R_SWING] = fe(-d(15)); j[SPINE_BEND] = fe(-d(8)); j[HEAD_NOD] = fe(-d(10)); }

        Mv::DanceBounce => {
            let b = wave(t, 30, 0);
            j[KNEE_L] = d(25) * b.abs() / 256; j[KNEE_R] = j[KNEE_L]; squash = 256 - 12 * b.abs() / 256;
            j[ARM_L_SWING] = d(70) + sw(d(40), t, 30, 0); j[ARM_R_SWING] = d(70) - sw(d(40), t, 30, 0); j[ELBOW_L] = d(80); j[ELBOW_R] = d(80);
            j[SPINE_TILT] = sw(d(8), t, 60, 0); j[HEAD_TILT] = sw(d(10), t, 60, 0);
        }
        Mv::Disco => {
            let up = wave(t, 50, 0) > 0;
            j[ARM_R_SWING] = if up { d(160) } else { d(30) }; j[ARM_R_RAISE] = if up { d(40) } else { -d(20) };
            j[ARM_L_RAISE] = d(30); j[ELBOW_L] = d(90); j[ARM_L_SWING] = -d(10);
            j[SPINE_TILT] = if up { d(10) } else { -d(10) }; j[KNEE_L] = d(15);
        }
        Mv::Robot => {
            let step = (t / 20) % 4;
            let a = |k: i32| if step == k { d(90) } else { 0 };
            j[ARM_L_SWING] = d(90); j[ARM_R_SWING] = d(90); j[ELBOW_L] = d(90) - a(1); j[ELBOW_R] = d(90) - a(3);
            j[HEAD_TURN] = if step < 2 { d(30) } else { -d(30) }; j[SPINE_TWIST] = if step % 2 == 0 { d(10) } else { -d(10) };
        }
        Mv::Twist => {
            j[SPINE_TWIST] = sw(d(25), t, 30, 0); j[KNEE_L] = d(25) + sw(d(10), t, 30, 0); j[KNEE_R] = d(25) - sw(d(10), t, 30, 0);
            j[ARM_L_SWING] = d(40) + sw(d(30), t, 30, 64); j[ARM_R_SWING] = d(40) - sw(d(30), t, 30, 64); j[ELBOW_L] = d(80); j[ELBOW_R] = d(80);
            squash = 256 - 12;
        }
        Mv::ChickenDance => {
            j[ARM_L_RAISE] = d(20) + sw(d(40), t, 14, 0).abs(); j[ARM_R_RAISE] = j[ARM_L_RAISE];
            j[ELBOW_L] = d(140); j[ELBOW_R] = d(140); j[ARM_L_SWING] = d(20); j[ARM_R_SWING] = d(20);
            j[KNEE_L] = d(20); j[KNEE_R] = d(20); j[HEAD_NOD] = sw(d(15), t, 14, 0); j[SPINE_BEND] = d(10);
        }
        Mv::Spin => { spin = prog(t, len) * 2; j[ARM_L_RAISE] = fe(d(70)); j[ARM_R_RAISE] = fe(d(70)); lift = fe(4); }
        Mv::Floss => {
            let s = wave(t, 24, 0);
            j[ARM_L_SWING] = d(30) * s / 256; j[ARM_R_SWING] = d(30) * s / 256; j[ARM_L_RAISE] = d(25) * s / 256 + d(5); j[ARM_R_RAISE] = -d(25) * s / 256 + d(5);
            j[SPINE_TILT] = -d(10) * s / 256; j[SPINE_TWIST] = d(8) * s / 256;
        }
        Mv::JumpingJacks => {
            let o = wave(t, 36, 0).max(0);
            j[ARM_L_RAISE] = d(6) + d(140) * o / 256; j[ARM_R_RAISE] = j[ARM_L_RAISE];
            j[LEG_L_OUT] = d(18) * o / 256; j[LEG_R_OUT] = j[LEG_L_OUT]; lift = 14 * wave(t, 18, 0).max(0) / 256;
        }
        Mv::Conga => {
            j[ARM_L_SWING] = d(80); j[ARM_R_SWING] = d(80); j[ELBOW_L] = d(30); j[ELBOW_R] = d(30);
            j[LEG_L_SWING] = sw(d(15), t, 40, 0); j[LEG_R_SWING] = -sw(d(15), t, 40, 0); j[SPINE_TILT] = sw(d(8), t, 40, 0);
            lift = 6 * wave(t, 20, 0).max(0) / 256;
        }
        Mv::SwayDance => { j[SPINE_TILT] = sw(d(12), t, 90, 0); j[ARM_L_SWING] = d(150); j[ARM_R_SWING] = d(150); j[ARM_L_RAISE] = d(15) + sw(d(15), t, 90, 0); j[ARM_R_RAISE] = d(15) - sw(d(15), t, 90, 0); }

        Mv::Cartwheel => {
            roll = prog(t, len); // one full turn sideways
            j[ARM_L_RAISE] = d(150); j[ARM_R_RAISE] = d(150); j[LEG_L_OUT] = d(30); j[LEG_R_OUT] = d(30);
            lift = 40 * bump(t, len) / 256;
        }
        Mv::Backflip | Mv::Frontflip | Mv::Somersault => {
            let p = prog(t, len);
            let tuck = if (40..216).contains(&p) { 256 } else { 0 };
            pitch = if m == Mv::Backflip { -p } else { p };
            j[KNEE_L] = d(100) * tuck / 256; j[KNEE_R] = j[KNEE_L]; j[LEG_L_SWING] = d(70) * tuck / 256; j[LEG_R_SWING] = j[LEG_L_SWING];
            j[ARM_L_SWING] = d(90); j[ARM_R_SWING] = d(90);
            lift = if m == Mv::Somersault { 10 * bump(t, len) / 256 } else { 90 * bump(t, len) / 256 };
            squash = if p < 20 { 220 } else { 256 };
        }
        Mv::Handstand => {
            pitch = 128 * e / 256; j[ARM_L_SWING] = d(170); j[ARM_R_SWING] = d(170);
            j[LEG_L_OUT] = sw(d(15), t, 50, 0).abs(); j[LEG_R_OUT] = j[LEG_L_OUT]; lift = 30 * e / 256;
        }
        Mv::TwirlJump => { spin = prog(t, len); lift = 70 * bump(t, len) / 256; j[ARM_L_SWING] = d(160); j[ARM_R_SWING] = d(160); squash = 256 + 20 * bump(t, len) / 256; }

        Mv::Walk | Mv::Moonwalk | Mv::Tiptoe | Mv::March | Mv::Run => {
            // One stride = 36 frames (two steps). Legs swing opposite each other; a knee
            // bends only while its leg swings forward; arms swing opposite the legs.
            let (period, legs, knees, arms) = match m {
                Mv::Run => (24, d(34), d(65), d(40)),
                Mv::Tiptoe => (48, d(14), d(30), d(10)),
                Mv::March => (40, d(30), d(80), d(45)),
                _ => (36, d(24), d(38), d(26)),
            };
            let dir = if m == Mv::Moonwalk { -1 } else { 1 };
            let s = wave(t, period, 0) * dir;
            let c = wave(t, period, 64) * dir;
            j[LEG_L_SWING] = legs * s / 256; j[LEG_R_SWING] = -legs * s / 256;
            j[KNEE_L] = knees * c.max(0) / 256 + d(5); j[KNEE_R] = knees * (-c).max(0) / 256 + d(5);
            j[ARM_L_SWING] = -arms * s / 256; j[ARM_R_SWING] = arms * s / 256;
            j[SPINE_TWIST] = d(4) * s / 256;
            lift = 4 * wave(t, period / 2, 64).abs() / 256;
            match m {
                Mv::Run => { j[SPINE_BEND] = d(10); j[ELBOW_L] = d(80); j[ELBOW_R] = d(80); lift *= 2; }
                Mv::Tiptoe => { j[ARM_L_SWING] = d(50); j[ARM_R_SWING] = d(50); j[ELBOW_L] = d(100); j[ELBOW_R] = d(100); j[SPINE_BEND] = d(14); j[HEAD_TURN] = sw(d(20), t, 96, 0); }
                Mv::March => { j[SPINE_BEND] = -d(5); }
                _ => {}
            }
        }
        Mv::Skip => {
            let s = wave(t, 30, 0);
            j[LEG_L_SWING] = d(30) * s.max(0) / 256; j[KNEE_L] = d(70) * s.max(0) / 256;
            j[LEG_R_SWING] = d(30) * (-s).max(0) / 256; j[KNEE_R] = d(70) * (-s).max(0) / 256;
            j[ARM_L_SWING] = -d(35) * s / 256; j[ARM_R_SWING] = d(35) * s / 256; lift = 16 * wave(t, 15, 0).abs() / 256;
        }
        Mv::Hop => {
            let h = wave(t, 26, 0).max(0);
            lift = 30 * h / 256; j[KNEE_L] = d(30) * (256 - h) / 256; j[KNEE_R] = j[KNEE_L];
            j[ARM_L_SWING] = d(40); j[ARM_R_SWING] = d(40); j[ELBOW_L] = d(90); j[ELBOW_R] = d(90); squash = 256 + 20 * h / 256 - 14 * (256 - h) / 256;
        }
        Mv::Climb => {
            // Hands take turns reaching up; knees take turns stepping up.
            let s = wave(t, 40, 0);
            j[ARM_L_SWING] = d(150) + d(25) * s / 256; j[ARM_R_SWING] = d(150) - d(25) * s / 256;
            j[ELBOW_L] = d(30) + d(30) * (-s).max(0) / 256; j[ELBOW_R] = d(30) + d(30) * s.max(0) / 256;
            j[LEG_L_SWING] = d(40) * s.max(0) / 256; j[KNEE_L] = d(70) * s.max(0) / 256;
            j[LEG_R_SWING] = d(40) * (-s).max(0) / 256; j[KNEE_R] = d(70) * (-s).max(0) / 256;
        }
        Mv::Hang => { j[ARM_L_SWING] = d(170); j[ARM_R_SWING] = d(170); j[LEG_L_SWING] = sw(d(18), t, 70, 0); j[LEG_R_SWING] = sw(d(18), t, 70, 20); j[KNEE_L] = d(10); j[KNEE_R] = d(10); }
        Mv::SitEdge => {
            // Sitting on a ledge: thighs forward, lower legs hanging and swinging.
            j[LEG_L_SWING] = d(85); j[LEG_R_SWING] = d(85); j[KNEE_L] = d(85) + sw(d(18), t, 60, 0); j[KNEE_R] = d(85) - sw(d(18), t, 60, 0);
            j[ARM_L_SWING] = -d(20); j[ARM_R_SWING] = -d(20); j[ARM_L_RAISE] = d(18); j[ARM_R_RAISE] = d(18); j[SPINE_BEND] = d(4);
            j[HEAD_TILT] = sw(d(6), t, 120, 0); lift = -60;
        }
        Mv::Nap => {
            // Curled up asleep on its side, breathing slowly.
            roll = 64; j[KNEE_L] = d(70); j[KNEE_R] = d(70); j[LEG_L_SWING] = d(50); j[LEG_R_SWING] = d(50);
            j[ARM_L_SWING] = d(70); j[ARM_R_SWING] = d(70); j[ELBOW_L] = d(110); j[ELBOW_R] = d(110);
            j[SPINE_BEND] = d(15) + sw(d(3), t, 150, 0); lift = -92;
        }
        Mv::Fall => {
            j[ARM_L_SWING] = d(150) + sw(d(25), t, 12, 0); j[ARM_R_SWING] = d(150) - sw(d(25), t, 12, 0); j[ARM_L_RAISE] = d(40); j[ARM_R_RAISE] = d(40);
            j[LEG_L_SWING] = sw(d(25), t, 14, 0); j[LEG_R_SWING] = -sw(d(25), t, 14, 0); j[KNEE_L] = d(30); j[KNEE_R] = d(30); squash = 270;
        }
        Mv::Land | Mv::Crouch => {
            let b = bump(t, len);
            j[KNEE_L] = d(50) * b / 256; j[KNEE_R] = j[KNEE_L]; j[LEG_L_SWING] = d(25) * b / 256; j[LEG_R_SWING] = j[LEG_L_SWING];
            j[SPINE_BEND] = d(15) * b / 256; j[ARM_L_RAISE] = d(30) * b / 256; j[ARM_R_RAISE] = j[ARM_L_RAISE]; squash = 256 - 50 * b / 256;
        }
        Mv::Jump => { j[ARM_L_SWING] = d(150); j[ARM_R_SWING] = d(150); j[KNEE_L] = d(50); j[KNEE_R] = d(20); j[LEG_L_SWING] = d(30); squash = 285; }
        Mv::Carried => {
            j[ARM_L_SWING] = d(160) + sw(d(15), t, 30, 0); j[ARM_R_SWING] = d(160) - sw(d(15), t, 30, 0);
            j[LEG_L_SWING] = sw(d(30), t, 22, 0); j[LEG_R_SWING] = sw(d(30), t, 22, 128); j[KNEE_L] = d(25); j[KNEE_R] = d(25); squash = 280;
        }

        Mv::Dizzy => {
            j[HEAD_TILT] = sw(d(18), t, 40, 0); j[HEAD_TURN] = sw(d(18), t, 40, 64); j[SPINE_TILT] = sw(d(10), t, 40, 0);
            j[ARM_L_RAISE] = d(30); j[ARM_R_RAISE] = d(30); j[KNEE_L] = d(15) + sw(d(10), t, 40, 0); j[KNEE_R] = d(15) - sw(d(10), t, 40, 0);
        }
        Mv::Push => { j[ARM_L_SWING] = fe(d(90)); j[ARM_R_SWING] = fe(d(90)); j[SPINE_BEND] = fe(d(25)); j[LEG_R_SWING] = fe(-d(25)); j[LEG_L_SWING] = fe(d(10)); j[KNEE_L] = fe(d(25)); }
        Mv::Kick => {
            let p = prog(t, len);
            let k = if p < 100 { -d(20) * p / 100 } else if p < 150 { -d(20) + d(90) * (p - 100) / 50 } else { d(70) * (256 - p) / 106 };
            j[LEG_R_SWING] = k; j[KNEE_R] = if p < 100 { d(40) * p / 100 } else { d(40) * (256 - p) / 156 }; j[ARM_L_SWING] = k / 2; j[ARM_R_SWING] = -k / 2;
        }
        Mv::Throw => {
            let p = prog(t, len);
            j[ARM_R_SWING] = if p < 120 { -d(40) * p / 120 } else { -d(40) + d(190) * (p - 120).min(60) / 60 };
            j[ELBOW_R] = d(60); j[SPINE_TWIST] = if p < 120 { -d(15) } else { d(15) }; j[LEG_L_SWING] = d(15);
        }
        Mv::Catch => { j[ARM_L_SWING] = fe(d(85)); j[ARM_R_SWING] = fe(d(85)); j[ARM_L_RAISE] = fe(-d(10)); j[ARM_R_RAISE] = fe(-d(10)); j[ELBOW_L] = fe(d(40)); j[ELBOW_R] = fe(d(40)); j[KNEE_L] = fe(d(15)); j[KNEE_R] = fe(d(15)); }
        Mv::Juggle => {
            j[ARM_L_SWING] = d(70) + sw(d(25), t, 20, 0); j[ARM_R_SWING] = d(70) - sw(d(25), t, 20, 0);
            j[ELBOW_L] = d(70); j[ELBOW_R] = d(70); j[HEAD_NOD] = -d(20) + sw(d(8), t, 20, 64);
        }
        Mv::HoldBalloon => { j[ARM_R_SWING] = d(165); j[ARM_R_RAISE] = d(10); j[ELBOW_R] = d(5); j[LEG_L_SWING] = sw(d(12), t, 80, 0); j[LEG_R_SWING] = sw(d(12), t, 80, 40); j[HEAD_NOD] = -d(15); }
        Mv::Water => { j[ARM_R_SWING] = fe(d(70)); j[ELBOW_R] = fe(d(30)); j[ARM_R_RAISE] = fe(-d(10) + sw(d(8), t, 40, 0)); j[SPINE_BEND] = fe(d(15)); j[HEAD_NOD] = fe(d(25)); }
        Mv::Plant => { let b = bump(t, len); j[KNEE_L] = d(80) * b / 256; j[KNEE_R] = j[KNEE_L]; j[LEG_L_SWING] = d(50) * b / 256; j[LEG_R_SWING] = j[LEG_L_SWING]; j[SPINE_BEND] = d(35) * b / 256; j[ARM_L_SWING] = d(60) * b / 256; j[ARM_R_SWING] = d(60) * b / 256 + sw(d(10), t, 16, 0) * b / 256; squash = 256 - 40 * b / 256; }
        Mv::Doze => { j[HEAD_NOD] = d(20) + sw(d(10), t, 100, 0); j[SPINE_BEND] = d(6); j[ARM_L_RAISE] = d(2); j[ARM_R_RAISE] = d(2); }
        Mv::Hug => { j[ARM_L_SWING] = fe(d(75)); j[ARM_R_SWING] = fe(d(75)); j[ARM_L_RAISE] = fe(-d(25)); j[ARM_R_RAISE] = fe(-d(25)); j[ELBOW_L] = fe(d(70)); j[ELBOW_R] = fe(d(70)); j[SPINE_BEND] = fe(d(8)); j[HEAD_TILT] = fe(d(10)); }
        Mv::HighFive => { let b = bump(t, len); j[ARM_R_SWING] = d(140) * b / 256; j[ARM_R_RAISE] = d(15) * b / 256; lift = 18 * b / 256; }
        Mv::Whisper => { j[SPINE_TILT] = fe(d(15)); j[ARM_L_SWING] = fe(d(90)); j[ELBOW_L] = fe(d(120)); j[ARM_L_RAISE] = fe(d(10)); j[HEAD_TILT] = fe(d(12)); }
        Mv::Sing => { j[ARM_L_RAISE] = fe(d(50)); j[ARM_R_RAISE] = fe(d(50)); j[ARM_L_SWING] = fe(d(40)); j[ARM_R_SWING] = fe(d(40)); j[HEAD_NOD] = fe(-d(18)); j[SPINE_TILT] = sw(d(8), t, 70, 0); }
        Mv::Squats => {
            let b = wave(t, 50, 0).max(0);
            j[KNEE_L] = d(90) * b / 256; j[KNEE_R] = j[KNEE_L]; j[LEG_L_SWING] = d(60) * b / 256; j[LEG_R_SWING] = j[LEG_L_SWING];
            j[ARM_L_SWING] = d(90) * b / 256; j[ARM_R_SWING] = j[ARM_L_SWING]; j[SPINE_BEND] = d(20) * b / 256; squash = 256 - 45 * b / 256;
        }
    }

    // Mii heads are big: an arm raised straight up in front would cover the face.
    // So "arms up" becomes a happy V shape (up and out to the side) instead.
    if pitch == 0 && roll == 0 {
        for (sw_, rz) in [(ARM_L_SWING, ARM_L_RAISE), (ARM_R_SWING, ARM_R_RAISE)] {
            let f = ((j[sw_] - d(100)) * 256 / d(50)).clamp(0, 256); // 0 = arm forward .. 256 = arm straight up
            if f > 0 {
                j[sw_] = (j[sw_] * (256 - f) + d(15) * f) / 256;
                j[rz] = (j[rz] * (256 - f) + d(122) * f) / 256;
            }
            j[rz] = j[rz].min(d(125)); // any higher and the arm hides behind the head
        }
    }
    let mut pose = MiiPose { yaw: 0, pitch, roll, squash, j: [0; NJ] };
    for k in 0..NJ { pose.j[k] = j[k].clamp(-126, 126) as i8; }
    Frame { pose, lift, spin }
}
