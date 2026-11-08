//! Animation helpers.
//!
//! A *spring* pulls a value toward a target, like a weight on a rubber band.
//! With a little less damping than needed to stop dead, it overshoots
//! slightly and settles: the bouncy, playful motion we want. Everything is
//! fixed-point and steps at exactly 60 Hz, so it looks the same on any
//! machine.

use crate::gfx::math::Fx;

pub const STEP_HZ: i64 = 60;

#[derive(Clone, Copy, Debug)]
pub struct Spring { pub value: Fx, pub target: Fx, vel: Fx, k: i64, d: i64 }

impl Spring {
    /// A snappy spring with a little bounce.
    pub const fn new(v: Fx) -> Spring { Spring { value: v, target: v, vel: 0, k: 320, d: 24 } }
    /// A softer, slower spring (for big movements).
    pub const fn soft(v: Fx) -> Spring { Spring { value: v, target: v, vel: 0, k: 140, d: 20 } }
    pub fn set(&mut self, t: Fx) { self.target = t; }
    pub fn snap(&mut self, v: Fx) { self.value = v; self.target = v; self.vel = 0; }
    pub fn moving(&self) -> bool { self.value != self.target || self.vel != 0 }

    /// Advance 1/60 s. Returns true while still moving.
    pub fn step(&mut self) -> bool {
        if !self.moving() { return false; }
        let x = (self.target - self.value) as i64;
        let v = self.vel as i64;
        let a = self.k * x - self.d * v;      // acceleration
        let nv = v + a / STEP_HZ;
        self.vel = nv as Fx;
        self.value += (nv / STEP_HZ) as Fx;
        // Close enough and slow enough: stop exactly on target.
        if (self.target - self.value).abs() < 26 && self.vel.abs() < 128 {
            self.value = self.target;
            self.vel = 0;
            return false;
        }
        true
    }
}

/// A simple 0 -> 256 progress over a fixed number of 60 Hz steps.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tween { pub t: i32, pub steps: i32, pub running: bool }
impl Tween {
    pub fn start(&mut self, steps: i32) { self.t = 0; self.steps = steps.max(1); self.running = true; }
    pub fn step(&mut self) -> bool {
        if !self.running { return false; }
        self.t += 1;
        if self.t >= self.steps { self.t = self.steps; self.running = false; }
        true
    }
    /// Progress 0..=256.
    pub fn p(&self) -> i32 { self.t * 256 / self.steps.max(1) }
}
