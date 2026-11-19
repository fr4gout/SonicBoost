//! SonicBoost Mic: voice processing chain (dependency free, allocation free
//! in the hot path). Every stage can be switched off on its own.
//!
//!   mic → boost → [noise suppression: in mic.rs] → gate → voice EQ →
//!   compressor → de-esser → voice mode → soundboard mix → limiter → out

use crate::dsp::{Ambience, FilterKind, Limiter, ParametricEq, PeqBand, PitchShifter};
use std::f32::consts::PI;
use std::sync::Arc;

pub const MIC_FS: f32 = 48_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceMode {
    Natural,
    Broadcast,
    Deep,
    High,
    Chipmunk,
    Robot,
    Radio,
    Telephone,
    Megaphone,
    Monster,
    Alien,
    Cave,
    Custom,
}

impl VoiceMode {
    pub const ALL: [VoiceMode; 13] = [
        VoiceMode::Natural,
        VoiceMode::Broadcast,
        VoiceMode::Deep,
        VoiceMode::High,
        VoiceMode::Chipmunk,
        VoiceMode::Robot,
        VoiceMode::Radio,
        VoiceMode::Telephone,
        VoiceMode::Megaphone,
        VoiceMode::Monster,
        VoiceMode::Alien,
        VoiceMode::Cave,
        VoiceMode::Custom,
    ];
    pub fn from_code(c: &str) -> VoiceMode {
        match c.to_ascii_lowercase().as_str() {
            "broadcast" => VoiceMode::Broadcast,
            "deep" => VoiceMode::Deep,
            "high" => VoiceMode::High,
            "chipmunk" => VoiceMode::Chipmunk,
            "robot" => VoiceMode::Robot,
            "radio" => VoiceMode::Radio,
            "telephone" => VoiceMode::Telephone,
            "megaphone" => VoiceMode::Megaphone,
            "monster" => VoiceMode::Monster,
            "alien" => VoiceMode::Alien,
            "cave" => VoiceMode::Cave,
            "custom" => VoiceMode::Custom,
            _ => VoiceMode::Natural,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            VoiceMode::Natural => "natural",
            VoiceMode::Broadcast => "broadcast",
            VoiceMode::Deep => "deep",
            VoiceMode::High => "high",
            VoiceMode::Chipmunk => "chipmunk",
            VoiceMode::Robot => "robot",
            VoiceMode::Radio => "radio",
            VoiceMode::Telephone => "telephone",
            VoiceMode::Megaphone => "megaphone",
            VoiceMode::Monster => "monster",
            VoiceMode::Alien => "alien",
            VoiceMode::Cave => "cave",
            VoiceMode::Custom => "custom",
        }
    }
}

/// Plain copy of the mic settings, read by the audio thread when they change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MicParams {
    pub boost_db: f32, // 0..24
    pub gate_on: bool,
    pub gate_db: f32, // threshold, -80..-10
    pub eq_preset: u8, // 0 off, 1 clean, 2 broadcast, 3 clarity, 4 bass voice, 5 warm
    pub comp_on: bool,
    pub comp_amount: f32, // 0..1
    pub deess_on: bool,
    pub deess_amount: f32, // 0..1
    pub mode: VoiceMode,
    pub custom_pitch: f32, // semitones for Custom
    pub custom_room: f32,  // 0..1 room for Custom
    pub limiter_on: bool,
    pub out_db: f32, // -24..12
    pub muted: bool,
    pub sb_gain: f32, // soundboard level, linear 0..2
}

impl Default for MicParams {
    fn default() -> Self {
        Self {
            boost_db: 0.0,
            gate_on: true,
            gate_db: -48.0,
            eq_preset: 1,
            comp_on: true,
            comp_amount: 0.5,
            deess_on: false,
            deess_amount: 0.5,
            mode: VoiceMode::Natural,
            custom_pitch: 0.0,
            custom_room: 0.0,
            limiter_on: true,
            out_db: 0.0,
            muted: false,
            sb_gain: 0.8,
        }
    }
}

#[inline]
fn db2lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}
#[inline]
fn lin2db(x: f32) -> f32 {
    20.0 * (x.abs() + 1e-9).log10()
}
#[inline]
fn coef(ms: f32) -> f32 {
    1.0 - (-1.0 / (MIC_FS * ms / 1000.0)).exp()
}

// ------------------------------------------------------------------ gate --

/// Noise gate with hysteresis and hold: silent between words, no chattering.
pub struct Gate {
    env: f32,
    gain: f32,
    hold: u32,
    open: bool,
}

impl Gate {
    pub fn new() -> Self {
        Self { env: 0.0, gain: 0.0, hold: 0, open: false }
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    #[inline]
    pub fn process(&mut self, x: f32, thresh_db: f32) -> f32 {
        let a = x.abs();
        let k = if a > self.env { coef(0.5) } else { coef(40.0) };
        self.env += (a - self.env) * k;
        let lvl = lin2db(self.env);
        if lvl > thresh_db {
            self.open = true;
            self.hold = (MIC_FS * 0.18) as u32;
        } else if lvl < thresh_db - 6.0 {
            if self.hold > 0 {
                self.hold -= 1;
            } else {
                self.open = false;
            }
        }
        let target = if self.open { 1.0 } else { 0.0 };
        let k = if target > self.gain { coef(1.0) } else { coef(90.0) };
        self.gain += (target - self.gain) * k;
        x * self.gain
    }
}

// ------------------------------------------------------------ compressor --

/// Voice compressor: steady level whether you whisper or shout.
pub struct VoiceComp {
    env: f32,
    pub gr_db: f32,
}

impl VoiceComp {
    pub fn new() -> Self {
        Self { env: 0.0, gr_db: 0.0 }
    }
    #[inline]
    pub fn process(&mut self, x: f32, amount: f32) -> f32 {
        let a = x.abs();
        let k = if a > self.env { coef(6.0) } else { coef(160.0) };
        self.env += (a - self.env) * k;
        let amount = amount.clamp(0.0, 1.0);
        let thr = -12.0 - 18.0 * amount;
        let ratio = 2.0 + 4.0 * amount;
        let lvl = lin2db(self.env);
        let over = lvl - thr;
        let knee = 6.0;
        let gr = if over <= -knee / 2.0 {
            0.0
        } else if over >= knee / 2.0 {
            over * (1.0 - 1.0 / ratio)
        } else {
            (1.0 - 1.0 / ratio) * (over + knee / 2.0).powi(2) / (2.0 * knee)
        };
        self.gr_db = gr;
        let makeup = -thr * (1.0 - 1.0 / ratio) * 0.55;
        x * db2lin(makeup - gr)
    }
}

// ------------------------------------------------------------- de-esser --

/// Tames harsh "s" sounds: splits off the top end and turns only it down
/// while sibilance is loud. With no sibilance it's perfectly transparent.
pub struct DeEsser {
    lp: f32,
    env: f32,
    lp_k: f32,
    pub gr_db: f32,
}

impl DeEsser {
    pub fn new() -> Self {
        Self { lp: 0.0, env: 0.0, lp_k: 1.0 - (-2.0 * PI * 5500.0 / MIC_FS).exp(), gr_db: 0.0 }
    }
    #[inline]
    pub fn process(&mut self, x: f32, amount: f32) -> f32 {
        self.lp += (x - self.lp) * self.lp_k;
        let hp = x - self.lp;
        let a = hp.abs();
        let k = if a > self.env { coef(1.0) } else { coef(60.0) };
        self.env += (a - self.env) * k;
        let thr = -24.0 - 14.0 * amount.clamp(0.0, 1.0);
        let over = lin2db(self.env) - thr;
        let gr = if over > 0.0 { (over * 0.75).min(4.0 + 10.0 * amount) } else { 0.0 };
        self.gr_db = gr;
        self.lp + hp * db2lin(-gr)
    }
}

// ----------------------------------------------------------- voice EQ ---

fn band(kind: FilterKind, freq: f32, gain_db: f32, q: f32) -> PeqBand {
    PeqBand { enabled: true, kind, freq, gain_db, q }
}

/// Built-in voice EQ curves.
pub fn eq_preset(n: u8) -> Vec<PeqBand> {
    use FilterKind::*;
    match n {
        1 => vec![band(HighPass, 80.0, 0.0, 0.707)], // Clean
        2 => vec![
            // Broadcast
            band(HighPass, 70.0, 0.0, 0.707),
            band(LowShelf, 130.0, 3.0, 0.7),
            band(Peaking, 400.0, -2.0, 1.0),
            band(Peaking, 3500.0, 3.0, 0.8),
            band(HighShelf, 10_000.0, 2.0, 0.7),
        ],
        3 => vec![
            // Clarity
            band(HighPass, 100.0, 0.0, 0.707),
            band(Peaking, 250.0, -3.0, 1.0),
            band(Peaking, 4000.0, 4.0, 1.0),
            band(HighShelf, 9000.0, 3.0, 0.7),
        ],
        4 => vec![
            // Bass voice
            band(HighPass, 50.0, 0.0, 0.707),
            band(LowShelf, 150.0, 5.0, 0.7),
            band(Peaking, 3000.0, 1.5, 1.0),
        ],
        5 => vec![
            // Warm
            band(HighPass, 60.0, 0.0, 0.707),
            band(LowShelf, 200.0, 2.5, 0.7),
            band(HighShelf, 7000.0, -2.0, 0.7),
        ],
        _ => vec![],
    }
}

// ---------------------------------------------------------- voice modes --

fn mode_eq(m: VoiceMode) -> Vec<PeqBand> {
    use FilterKind::*;
    match m {
        VoiceMode::Broadcast => vec![band(LowShelf, 120.0, 4.0, 0.7), band(Peaking, 3500.0, 3.0, 0.8)],
        VoiceMode::Deep => vec![band(LowShelf, 150.0, 2.5, 0.7)],
        VoiceMode::Radio => vec![
            band(HighPass, 300.0, 0.0, 0.707),
            band(LowPass, 3000.0, 0.0, 0.707),
            band(Peaking, 1500.0, 4.0, 1.0),
        ],
        VoiceMode::Telephone => vec![band(HighPass, 400.0, 0.0, 0.9), band(LowPass, 3400.0, 0.0, 0.9)],
        VoiceMode::Megaphone => vec![
            band(HighPass, 500.0, 0.0, 0.707),
            band(LowPass, 4000.0, 0.0, 0.707),
            band(Peaking, 1800.0, 8.0, 1.0),
        ],
        VoiceMode::Monster => vec![band(LowShelf, 120.0, 5.0, 0.7), band(HighShelf, 6000.0, -4.0, 0.7)],
        VoiceMode::Alien => vec![band(Peaking, 2500.0, 3.0, 1.2)],
        _ => vec![],
    }
}

fn mode_pitch(m: VoiceMode, custom: f32) -> f32 {
    match m {
        VoiceMode::Deep => -4.0,
        VoiceMode::High => 5.0,
        VoiceMode::Chipmunk => 9.0,
        VoiceMode::Monster => -8.0,
        VoiceMode::Alien => 3.0,
        VoiceMode::Custom => custom.clamp(-12.0, 12.0),
        _ => 0.0,
    }
}

/// Short delay line used by the robot comb, flanger and megaphone slap.
struct Delay {
    buf: Vec<f32>,
    w: usize,
}
impl Delay {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.next_power_of_two()], w: 0 }
    }
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let n = self.buf.len();
        let pos = (self.w as f32 - d).rem_euclid(n as f32);
        let i = pos.floor() as usize % n;
        let f = pos - pos.floor();
        self.buf[i] * (1.0 - f) + self.buf[(i + 1) % n] * f
    }
    #[inline]
    fn write(&mut self, x: f32) {
        let n = self.buf.len();
        self.buf[self.w] = x;
        self.w = (self.w + 1) % n;
    }
    fn clear(&mut self) {
        self.buf.iter_mut().for_each(|v| *v = 0.0);
    }
}

// ---------------------------------------------------------- soundboard ---

pub const SB_VOICES: usize = 8;

/// Mixes soundboard clips into the mic stream (several at once).
pub struct Soundboard {
    voices: [Option<(Arc<Vec<f32>>, usize)>; SB_VOICES],
}

impl Soundboard {
    pub fn new() -> Self {
        Self { voices: Default::default() }
    }
    /// Starts a clip (moves the Arc in, no allocation).
    pub fn play(&mut self, clip: Arc<Vec<f32>>) {
        if let Some(slot) = self.voices.iter_mut().find(|v| v.is_none()) {
            *slot = Some((clip, 0));
        } else {
            self.voices[0] = Some((clip, 0));
        }
    }
    pub fn stop(&mut self) {
        self.voices.iter_mut().for_each(|v| *v = None);
    }
    pub fn active(&self) -> bool {
        self.voices.iter().any(|v| v.is_some())
    }
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut s = 0.0;
        for v in self.voices.iter_mut() {
            let done = match v {
                Some((clip, pos)) => {
                    if *pos < clip.len() {
                        s += clip[*pos];
                        *pos += 1;
                    }
                    *pos >= clip.len()
                }
                None => false,
            };
            if done {
                *v = None; // free the slot as soon as the clip ends
            }
        }
        s
    }
}

// ---------------------------------------------------------------- chain --

pub struct MicChain {
    p: MicParams,
    boost: f32,
    out: f32,
    mute: f32,
    pub gate: Gate,
    eq: ParametricEq,
    pub comp: VoiceComp,
    pub deess: DeEsser,
    mode: VoiceMode,
    meq: ParametricEq,
    pitch: PitchShifter,
    t: f32,
    comb: Delay,
    flange: Delay,
    slap: Delay,
    hold: f32,
    hold_n: u32,
    room: Ambience,
    pub soundboard: Soundboard,
    limiter: Limiter,
}

impl MicChain {
    pub fn new() -> Self {
        let mut c = Self {
            p: MicParams::default(),
            boost: 1.0,
            out: 1.0,
            mute: 1.0,
            gate: Gate::new(),
            eq: ParametricEq::new(MIC_FS),
            comp: VoiceComp::new(),
            deess: DeEsser::new(),
            mode: VoiceMode::Natural,
            meq: ParametricEq::new(MIC_FS),
            pitch: PitchShifter::new(MIC_FS),
            t: 0.0,
            comb: Delay::new(1024),
            flange: Delay::new(1024),
            slap: Delay::new(8192),
            hold: 0.0,
            hold_n: 0,
            room: Ambience::new(MIC_FS),
            soundboard: Soundboard::new(),
            limiter: Limiter::new(MIC_FS),
        };
        c.set(MicParams::default());
        c
    }

    /// Not allocation-free (rebuilds filter lists): call on parameter change.
    pub fn set(&mut self, p: MicParams) {
        if p.eq_preset != self.p.eq_preset || self.p == MicParams::default() {
            let b = eq_preset(p.eq_preset);
            self.eq.set(!b.is_empty(), 0.0, &b);
        }
        if p.mode != self.mode || p.custom_room != self.p.custom_room {
            let b = mode_eq(p.mode);
            self.meq.set(!b.is_empty(), 0.0, &b);
            self.comb.clear();
            self.flange.clear();
            self.slap.clear();
            let (on, mix, size) = match p.mode {
                VoiceMode::Cave => (true, 0.6, 1.8),
                VoiceMode::Custom if p.custom_room > 0.01 => (true, p.custom_room, 1.4),
                _ => (false, 0.0, 1.0),
            };
            self.room.set(on, mix, size);
            self.mode = p.mode;
        }
        self.p = p;
    }

    pub fn params(&self) -> MicParams {
        self.p
    }

    #[inline]
    fn sat(x: f32, drive: f32) -> f32 {
        (x * drive).tanh() / drive.tanh()
    }

    #[inline]
    fn voice(&mut self, x: f32) -> f32 {
        let m = self.mode;
        if m == VoiceMode::Natural {
            return x;
        }
        self.t += 1.0 / MIC_FS;
        if self.t > 1000.0 {
            self.t -= 1000.0;
        }
        let mut st = mode_pitch(m, self.p.custom_pitch);
        if m == VoiceMode::Alien {
            st += 0.4 * (2.0 * PI * 5.5 * self.t).sin(); // vibrato
        }
        let mut y = if st.abs() > 0.01 { self.pitch.process(x, 2f32.powf(st / 12.0)) } else { x };
        self.meq.tick();
        y = self.meq.process(0, y);
        match m {
            VoiceMode::Robot => {
                let ring = (2.0 * PI * 50.0 * self.t).sin();
                let r = y * ring * 0.85 + y * 0.15;
                let c = r + self.comb.read(MIC_FS * 0.004) * 0.6;
                self.comb.write(c);
                y = c * 0.7;
            }
            VoiceMode::Radio => y = Self::sat(y * 1.4, 3.0) * 0.8,
            VoiceMode::Telephone => {
                // 8 kHz-style lo-fi: hold each sample for 6 frames
                if self.hold_n == 0 {
                    self.hold = y;
                }
                self.hold_n = (self.hold_n + 1) % 6;
                y = Self::sat(self.hold, 1.6);
            }
            VoiceMode::Megaphone => {
                let s = Self::sat(y * 1.6, 4.0) * 0.7;
                let e = s + self.slap.read(MIC_FS * 0.07) * 0.3;
                self.slap.write(s);
                y = e;
            }
            VoiceMode::Monster => y = y * 0.6 + Self::sat(y, 2.0) * 0.5,
            VoiceMode::Alien => {
                let ring = (2.0 * PI * 35.0 * self.t).sin();
                y = y * 0.65 + y * ring * 0.35;
                let d = MIC_FS * (0.004 + 0.002 * (2.0 * PI * 0.3 * self.t).sin());
                let f = y + self.flange.read(d) * 0.5;
                self.flange.write(f);
                y = (y + f) * 0.5;
            }
            _ => {}
        }
        y
    }

    /// One mono sample in, one out. `denoised` has already been through RNNoise
    /// (or is the raw mic when noise suppression is off).
    #[inline]
    pub fn process(&mut self, denoised: f32) -> f32 {
        let p = self.p;
        let k = coef(25.0);
        self.boost += (db2lin(p.boost_db.clamp(0.0, 24.0)) - self.boost) * k;
        self.out += (db2lin(p.out_db.clamp(-24.0, 12.0)) - self.out) * k;
        self.mute += ((if p.muted { 0.0 } else { 1.0 }) - self.mute) * coef(8.0);

        let mut x = denoised * self.boost;
        if p.gate_on {
            x = self.gate.process(x, p.gate_db);
        } else {
            self.gate.open = true;
        }
        self.eq.tick();
        x = self.eq.process(0, x);
        if p.comp_on {
            x = self.comp.process(x, p.comp_amount);
        } else {
            self.comp.gr_db = 0.0;
        }
        if p.deess_on {
            x = self.deess.process(x, p.deess_amount);
        } else {
            self.deess.gr_db = 0.0;
        }
        x = self.voice(x);
        let (rl, _) = self.room.process(x, x);
        x = rl;
        x *= self.mute;
        x += self.soundboard.next() * p.sb_gain.clamp(0.0, 2.0);
        x *= self.out;
        if p.limiter_on {
            self.limiter.process(x, x).0
        } else {
            x.clamp(-1.0, 1.0)
        }
    }
}

// ----------------------------------------------------------------- WAV ---

/// Minimal WAV reader for soundboard clips: PCM 8/16/24/32-bit or float
/// 32/64, any channel count (mixed to mono), any rate (resampled to 48 kHz).
pub fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    let rd16 = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
    let rd32 = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Not a WAV file (only .wav clips are supported)".into());
    }
    let (mut fmt, mut data) = (None, None);
    let mut o = 12;
    while o + 8 <= bytes.len() {
        let id = &bytes[o..o + 4];
        let len = rd32(o + 4) as usize;
        let body = o + 8;
        if body + len > bytes.len() && id != b"data" {
            break;
        }
        match id {
            b"fmt " if len >= 16 => fmt = Some((rd16(body), rd16(body + 2), rd32(body + 4), rd16(body + 14))),
            b"data" => data = Some((body, len.min(bytes.len() - body))),
            _ => {}
        }
        o = body + len + (len & 1);
    }
    let (format, channels, rate, bits) = fmt.ok_or("WAV has no format chunk")?;
    let (start, len) = data.ok_or("WAV has no audio data")?;
    let ch = channels.max(1) as usize;
    let bps = (bits as usize / 8).max(1);
    let frames = len / (bps * ch);
    let is_float = format == 3;
    let sample = |i: usize| -> f32 {
        let p = start + i * bps;
        match (is_float, bits) {
            (true, 32) => f32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]),
            (true, 64) => f64::from_le_bytes(bytes[p..p + 8].try_into().unwrap()) as f32,
            (_, 8) => (bytes[p] as f32 - 128.0) / 128.0,
            (_, 16) => i16::from_le_bytes([bytes[p], bytes[p + 1]]) as f32 / 32768.0,
            (_, 24) => (((bytes[p] as i32) << 8 | (bytes[p + 1] as i32) << 16 | (bytes[p + 2] as i32) << 24) >> 8) as f32 / 8_388_608.0,
            (_, 32) => i32::from_le_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as f32 / 2_147_483_648.0,
            _ => 0.0,
        }
    };
    if !matches!(bits, 8 | 16 | 24 | 32 | 64) {
        return Err(format!("{bits}-bit WAV isn't supported"));
    }
    let mono: Vec<f32> = (0..frames).map(|f| (0..ch).map(|c| sample(f * ch + c)).sum::<f32>() / ch as f32).collect();
    if mono.len() > 48_000 * 60 {
        return Err("Clips can be at most 60 seconds long".into());
    }
    if rate == 48_000 || mono.is_empty() {
        return Ok(mono);
    }
    let step = rate as f64 / 48_000.0;
    let n = (mono.len() as f64 / step) as usize;
    Ok((0..n)
        .map(|i| {
            let pos = i as f64 * step;
            let a = pos.floor() as usize;
            let t = (pos - a as f64) as f32;
            let s0 = mono[a.min(mono.len() - 1)];
            let s1 = mono[(a + 1).min(mono.len() - 1)];
            s0 + (s1 - s0) * t
        })
        .collect())
}

// ----------------------------------------------------------------- tests --

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(f: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * f * i as f32 / MIC_FS).sin() * amp).collect()
    }
    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn zc(x: &[f32]) -> f32 {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count() as f32 * MIC_FS / x.len() as f32
    }

    #[test]
    fn gate_silences_noise_and_passes_voice() {
        let mut g = Gate::new();
        let noise: Vec<f32> = tone(200.0, 24_000, 0.002).iter().map(|&s| g.process(s, -40.0)).collect();
        assert!(rms(&noise[12_000..]) < 1e-4, "noise should be gated");
        let voice: Vec<f32> = tone(200.0, 24_000, 0.2).iter().map(|&s| g.process(s, -40.0)).collect();
        assert!(rms(&voice[4_800..]) > 0.13, "voice must pass");
        assert!(g.is_open());
    }

    #[test]
    fn compressor_evens_out_levels() {
        let level = |amp: f32| {
            let mut c = VoiceComp::new();
            let y: Vec<f32> = tone(300.0, 48_000, amp).iter().map(|&s| c.process(s, 0.7)).collect();
            rms(&y[24_000..])
        };
        let quiet = level(0.03);
        let loud = level(0.6);
        let in_ratio = 20.0 * (0.6f32 / 0.03).log10(); // 26 dB
        let out_ratio = 20.0 * (loud / quiet).log10();
        assert!(out_ratio < in_ratio * 0.6, "26 dB in became {out_ratio:.1} dB out");
    }

    #[test]
    fn deesser_reduces_sibilance_not_voice() {
        let mut d = DeEsser::new();
        let s: Vec<f32> = tone(7000.0, 24_000, 0.4).iter().map(|&v| d.process(v, 1.0)).collect();
        assert!(rms(&s[12_000..]) < 0.4 * 0.707 * 0.7, "sibilance reduced");
        let mut d = DeEsser::new();
        let v: Vec<f32> = tone(300.0, 24_000, 0.4).iter().map(|&x| d.process(x, 1.0)).collect();
        assert!((rms(&v[12_000..]) - 0.4 * 0.707).abs() < 0.02, "voice untouched");
    }

    #[test]
    fn every_voice_mode_is_stable_and_clip_free() {
        for &m in VoiceMode::ALL.iter() {
            let mut c = MicChain::new();
            c.set(MicParams { mode: m, custom_pitch: -3.0, custom_room: 0.5, gate_on: false, ..Default::default() });
            let x = tone(220.0, 48_000, 0.5);
            let y: Vec<f32> = x.iter().map(|&s| c.process(s)).collect();
            assert!(y.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{m:?} produced invalid output");
            assert!(rms(&y[24_000..]) > 0.01, "{m:?} went silent");
        }
    }

    #[test]
    fn deep_and_chipmunk_shift_pitch() {
        let f = |m: VoiceMode| {
            let mut c = MicChain::new();
            c.set(MicParams { mode: m, gate_on: false, comp_on: false, eq_preset: 0, ..Default::default() });
            let y: Vec<f32> = tone(220.0, 96_000, 0.3).iter().map(|&s| c.process(s)).collect();
            zc(&y[19_200..])
        };
        let deep = f(VoiceMode::Deep);
        let chip = f(VoiceMode::Chipmunk);
        assert!((deep - 220.0 * 2f32.powf(-4.0 / 12.0)).abs() < 12.0, "deep {deep}");
        assert!((chip - 220.0 * 2f32.powf(9.0 / 12.0)).abs() < 25.0, "chipmunk {chip}");
    }

    #[test]
    fn mute_and_soundboard() {
        let mut c = MicChain::new();
        c.set(MicParams { muted: true, gate_on: false, ..Default::default() });
        let y: Vec<f32> = tone(300.0, 9_600, 0.5).iter().map(|&s| c.process(s)).collect();
        assert!(rms(&y[4_800..]) < 1e-3, "muted");
        // soundboard still plays while the voice is muted
        c.soundboard.play(Arc::new(tone(500.0, 9_600, 0.4)));
        let y: Vec<f32> = (0..9_600).map(|_| c.process(0.0)).collect();
        assert!(rms(&y[1_000..8_000]) > 0.15, "clip should be heard");
        assert!(!c.soundboard.active());
    }

    #[test]
    fn wav_roundtrip_and_resample() {
        // 16-bit stereo 24 kHz, 0.5 s of 440 Hz
        let n = 12_000usize;
        let mut d = Vec::new();
        for i in 0..n {
            let v = ((2.0 * PI * 440.0 * i as f32 / 24_000.0).sin() * 16_000.0) as i16;
            d.extend_from_slice(&v.to_le_bytes());
            d.extend_from_slice(&v.to_le_bytes());
        }
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + d.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&24_000u32.to_le_bytes());
        w.extend_from_slice(&(24_000u32 * 4).to_le_bytes());
        w.extend_from_slice(&4u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(d.len() as u32).to_le_bytes());
        w.extend_from_slice(&d);
        let s = decode_wav(&w).unwrap();
        assert!((s.len() as i32 - 24_000).abs() < 4, "resampled to 48 kHz: {}", s.len());
        assert!((zc(&s) - 440.0).abs() < 5.0);
        assert!(decode_wav(b"not a wav").is_err());
    }
}
