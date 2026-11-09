//! Sound: Mii Hub's music softly in the background, plus little sound effects.
//!
//! One background thread is the *mixer*: every 10 ms it adds up the music
//! and any sound effects that are playing into one buffer of samples, and
//! hands it to `aplay` (a standard Linux tool) through a pipe. Writing to the
//! pipe waits until the sound card has room, which also keeps the timing.
//!
//! The music is Mii Hub's own song, decoded from Ogg Vorbis and looped at the
//! same seamless loop points Mii Hub uses. The sound effects aren't files at
//! all: they're made from sine waves and noise when MiiOS starts ("synthesis").

use crate::host::Sfx;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

const RATE: f32 = 48_000.0;

static FOCUSED: AtomicBool = AtomicBool::new(true);
static MUSIC_ON: AtomicBool = AtomicBool::new(true);
static SOUNDS_ON: AtomicBool = AtomicBool::new(true);
static SEED: AtomicU32 = AtomicU32::new(12345);

struct Voice { sfx: usize, pos: f32, step: f32, gain: f32 }
static VOICES: Mutex<Vec<Voice>> = Mutex::new(Vec::new());
static BANK: OnceLock<Vec<Vec<f32>>> = OnceLock::new();
static MUSIC: OnceLock<(Vec<i16>, usize, usize)> = OnceLock::new(); // stereo samples, loop start/end (frames)

fn rnd() -> f32 {
    let mut x = SEED.load(Ordering::Relaxed);
    x ^= x << 13; x ^= x >> 17; x ^= x << 5;
    SEED.store(x, Ordering::Relaxed);
    (x % 10_000) as f32 / 10_000.0
}

fn settings_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("MiiOS/.sound")
}
fn save() {
    let _ = std::fs::write(settings_path(), format!("music={}\nsounds={}\n",
        MUSIC_ON.load(Ordering::Relaxed) as u8, SOUNDS_ON.load(Ordering::Relaxed) as u8));
}

// ---------------------------------------------------------------- synthesis

/// A tone that slides from `f0` to `f1` Hz over `dur` seconds, with a quick
/// attack and an exponential fade ("envelope"). `square` mixes in harmonics.
fn tone(f0: f32, f1: f32, dur: f32, decay: f32, square: f32) -> Vec<f32> {
    let n = (dur * RATE) as usize;
    let mut phase = 0.0f32;
    (0..n).map(|i| {
        let t = i as f32 / n as f32;
        let f = f0 + (f1 - f0) * t;
        phase += f / RATE * std::f32::consts::TAU;
        let s = phase.sin();
        let wave = s * (1.0 - square) + s.signum() * square * 0.6;
        let env = (i as f32 / (0.004 * RATE)).min(1.0) * (-t * decay).exp();
        wave * env
    }).collect()
}
fn noise(dur: f32, decay: f32) -> Vec<f32> {
    let n = (dur * RATE) as usize;
    let mut last = 0.0f32;
    (0..n).map(|i| {
        let t = i as f32 / n as f32;
        last = last * 0.5 + (rnd() * 2.0 - 1.0) * 0.5; // a little low-pass: softer
        last * (-t * decay).exp()
    }).collect()
}
fn mix(a: Vec<f32>, b: Vec<f32>, gb: f32) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n).map(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0) * gb).collect()
}
fn then(mut a: Vec<f32>, gap: f32, b: Vec<f32>) -> Vec<f32> {
    a.extend(std::iter::repeat(0.0).take((gap * RATE) as usize));
    a.extend(b);
    a
}
fn scale(v: Vec<f32>, g: f32) -> Vec<f32> { v.into_iter().map(|s| s * g).collect() }

/// Every sound effect, in the same order as `Sfx`.
fn make_bank() -> Vec<Vec<f32>> {
    vec![
        /* Hover   */ scale(tone(1350.0, 1650.0, 0.045, 6.0, 0.0), 0.35),
        /* Type    */ scale(mix(noise(0.018, 9.0), tone(2600.0, 2200.0, 0.012, 8.0, 0.0), 0.4), 0.30),
        /* Open    */ scale(then(tone(660.0, 700.0, 0.07, 4.0, 0.1), 0.0, tone(990.0, 1050.0, 0.12, 4.0, 0.1)), 0.35),
        /* Back    */ scale(then(tone(880.0, 840.0, 0.06, 4.0, 0.1), 0.0, tone(587.0, 560.0, 0.11, 4.0, 0.1)), 0.32),
        /* Jump    */ scale(tone(220.0, 720.0, 0.16, 3.0, 0.15), 0.40),
        /* Land    */ scale(mix(tone(140.0, 90.0, 0.08, 5.0, 0.0), noise(0.05, 8.0), 0.3), 0.45),
        /* Chirp   */ scale(tone(420.0, 520.0, 0.07, 3.0, 0.35), 0.28),
        /* Pop     */ scale(tone(700.0, 1700.0, 0.06, 5.0, 0.0), 0.35),
        /* Sparkle */ scale(then(tone(2093.0, 2093.0, 0.08, 6.0, 0.0), 0.02, then(tone(2637.0, 2637.0, 0.08, 6.0, 0.0), 0.02, tone(3136.0, 3136.0, 0.14, 5.0, 0.0))), 0.22),
        /* Click   */ scale(mix(noise(0.012, 10.0), tone(1800.0, 1500.0, 0.01, 9.0, 0.0), 0.5), 0.40),
        /* Whoosh  */ scale(noise(0.22, 3.0), 0.25),
        /* Cheer   */ scale(then(tone(523.0, 523.0, 0.09, 3.0, 0.2), 0.0, then(tone(659.0, 659.0, 0.09, 3.0, 0.2), 0.0, tone(784.0, 800.0, 0.2, 3.0, 0.2))), 0.30),
        /* Step    */ scale(noise(0.03, 12.0), 0.12),
        /* Snore   */ scale(tone(110.0, 95.0, 0.6, 2.0, 0.5), 0.18),
    ]
}

/// Mii Hub's song: decode once, find the loop points (Mii Hub stores them in
/// samples at 44.1 kHz; this file is 48 kHz, so we go through seconds).
fn load_music() -> Option<(Vec<i16>, usize, usize)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = [format!("{home}/.local/share/miios/apps/mii-hub/music/crab-dance.ogg"), "/opt/miios/apps/mii-hub/music/crab-dance.ogg".into()]
        .into_iter().find(|p| std::path::Path::new(p).exists())?;
    let mut r = lewton::inside_ogg::OggStreamReader::new(std::fs::File::open(path).ok()?).ok()?;
    let (ch, rate) = (r.ident_hdr.audio_channels as usize, r.ident_hdr.audio_sample_rate as f32);
    let mut pcm = Vec::new();
    while let Ok(Some(p)) = r.read_dec_packet_itl() {
        if ch == 1 { for s in p { pcm.push(s); pcm.push(s); } } else { pcm.extend(p.chunks(ch).flat_map(|c| [c[0], c[1]])); }
    }
    let frames = pcm.len() / 2;
    let start = ((22050.0 / 44100.0) * rate) as usize;
    let end = (((2967173.0 / 44100.0) * rate) as usize).min(frames);
    if end <= start + rate as usize { return Some((pcm, 0, frames)); }
    Some((pcm, start, end))
}

// ---------------------------------------------------------------- mixer

pub fn start() {
    if let Ok(s) = std::fs::read_to_string(settings_path()) {
        MUSIC_ON.store(!s.contains("music=0"), Ordering::Relaxed);
        SOUNDS_ON.store(!s.contains("sounds=0"), Ordering::Relaxed);
    }
    SEED.store((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1)) | 1, Ordering::Relaxed);
    std::thread::spawn(|| {
        let _ = BANK.set(make_bank());
        std::thread::spawn(|| { if let Some(m) = load_music() { let _ = MUSIC.set(m); } });
        let Ok(mut child) = Command::new("aplay")
            .args(["-q", "-t", "raw", "-f", "S16_LE", "-r", "48000", "-c", "2", "--buffer-time=60000", "-"])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        else { crate::log!("sound: aplay not available"); return };
        let Some(mut out) = child.stdin.take() else { return };
        let bank = BANK.get().unwrap();
        let mut music_pos = 0usize; // frame
        let mut music_gain = 0.0f32;
        let block = 480; // 10 ms
        let mut buf = vec![0i16; block * 2];
        loop {
            let want = if MUSIC_ON.load(Ordering::Relaxed) && FOCUSED.load(Ordering::Relaxed) { 0.30 } else { 0.0 };
            let mut acc = vec![0f32; block * 2];
            if let Some((pcm, ls, le)) = MUSIC.get() {
                for i in 0..block {
                    music_gain += (want - music_gain) * 0.00005; // slow fade in/out (about half a second)
                    if music_gain > 0.0005 {
                        acc[i * 2] += pcm[music_pos * 2] as f32 / 32768.0 * music_gain;
                        acc[i * 2 + 1] += pcm[music_pos * 2 + 1] as f32 / 32768.0 * music_gain;
                        music_pos += 1;
                        if music_pos >= *le { music_pos = *ls; }
                    }
                }
            }
            {
                let mut v = VOICES.lock().unwrap();
                for voice in v.iter_mut() {
                    let snd = &bank[voice.sfx];
                    for i in 0..block {
                        let p = voice.pos as usize;
                        if p >= snd.len() { break; }
                        let s = snd[p] * voice.gain;
                        acc[i * 2] += s; acc[i * 2 + 1] += s;
                        voice.pos += voice.step;
                    }
                }
                v.retain(|voice| (voice.pos as usize) < bank[voice.sfx].len());
            }
            for (o, a) in buf.iter_mut().zip(&acc) { *o = (a.tanh() * 30000.0) as i16; } // tanh: gentle limiter, never clips harshly
            let bytes: Vec<u8> = buf.iter().flat_map(|s| s.to_le_bytes()).collect();
            if out.write_all(&bytes).is_err() { crate::log!("sound: output closed"); return; }
        }
    });
}

/// Play a sound effect (with a little random pitch so repeats sound natural).
pub fn play(s: Sfx, pitch: f32) {
    if !SOUNDS_ON.load(Ordering::Relaxed) || BANK.get().is_none() { return; }
    let mut v = VOICES.lock().unwrap();
    if v.len() >= 12 { return; }
    let wobble = 1.0 + (rnd() - 0.5) * 0.08;
    v.push(Voice { sfx: s as usize, pos: 0.0, step: pitch * wobble, gain: 1.0 });
}

pub fn set_focused(f: bool) { FOCUSED.store(f, Ordering::Relaxed); }
pub fn music_on() -> bool { MUSIC_ON.load(Ordering::Relaxed) }
pub fn sounds_on() -> bool { SOUNDS_ON.load(Ordering::Relaxed) }
pub fn set_music(on: bool) { MUSIC_ON.store(on, Ordering::Relaxed); save(); }
pub fn set_sounds(on: bool) { SOUNDS_ON.store(on, Ordering::Relaxed); save(); }
