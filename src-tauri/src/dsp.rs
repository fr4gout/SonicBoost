//! Real-time DSP chain (dependency free, allocation free in the hot path):
//!   input -> pitch shifter -> 10-band EQ -> boost gain -> limiter -> output

use std::f32::consts::PI;

pub const EQ_BANDS: usize = 10;
pub const EQ_FREQS: [f32; EQ_BANDS] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
pub const EQ_Q: f32 = 1.1;
pub const EQ_RANGE_DB: f32 = 12.0;
pub const BOOST_MAX: f32 = 6.0; // 600 %
pub const PITCH_RANGE_ST: f32 = 12.0;

/// Plain copy of the user parameters, read once per audio buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DspParams {
    pub boost: f32,              // linear gain, 1.0 = 100 %
    pub eq_db: [f32; EQ_BANDS],  // per band gain in dB
    pub pitch_st: f32,           // semitones (+ cents / 100)
    pub width: f32,              // stereo width, 1.0 = normal, 0 = mono, 2 = extra wide
    pub balance: f32,            // -1 = left .. 0 = centre .. +1 = right
    pub bypass: bool,
    pub limiter: bool,
}

impl Default for DspParams {
    fn default() -> Self {
        Self {
            boost: 1.0,
            eq_db: [0.0; EQ_BANDS],
            pitch_st: 0.0,
            width: 1.0,
            balance: 0.0,
            bypass: false,
            limiter: true,
        }
    }
}

// ---------------------------------------------------------------- biquad --

#[derive(Clone, Copy, Debug)]
pub struct Coefs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coefs {
    pub const IDENTITY: Coefs = Coefs { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn norm(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }

    /// RBJ cookbook peaking EQ.
    pub fn peaking(fs: f32, f0: f32, q: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f0.min(fs * 0.45) / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        Self::norm(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
    }

    /// RBJ low shelf, slope 1.
    pub fn low_shelf(fs: f32, f0: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f0.min(fs * 0.45) / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        )
    }

    /// RBJ high shelf, slope 1.
    pub fn high_shelf(fs: f32, f0: f32, gain_db: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * f0.min(fs * 0.45) / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        )
    }

    /// Coefficients for band `i` of the graphic EQ (shelves at the ends).
    pub fn eq_band(fs: f32, i: usize, gain_db: f32) -> Self {
        if gain_db.abs() < 0.01 {
            return Self::IDENTITY;
        }
        match i {
            0 => Self::low_shelf(fs, EQ_FREQS[0] * 1.6, gain_db),
            9 => Self::high_shelf(fs, EQ_FREQS[9] * 0.7, gain_db),
            _ => Self::peaking(fs, EQ_FREQS[i], EQ_Q, gain_db),
        }
    }

    /// Magnitude response in dB at frequency f (used by tests).
    pub fn response_db(&self, fs: f32, f: f32) -> f32 {
        let w = 2.0 * PI * f / fs;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// 10-band stereo graphic equalizer (transposed direct form II).
/// Gain changes glide over ~15 ms so dragging a slider never clicks.
pub struct Equalizer {
    fs: f32,
    coefs: [Coefs; EQ_BANDS],
    active: [bool; EQ_BANDS],
    z: [[[f32; 2]; 2]; EQ_BANDS], // [band][channel][z1,z2]
    cur: [f32; EQ_BANDS],
    target: [f32; EQ_BANDS],
    counter: u32,
}

impl Equalizer {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            coefs: [Coefs::IDENTITY; EQ_BANDS],
            active: [false; EQ_BANDS],
            z: [[[0.0; 2]; 2]; EQ_BANDS],
            cur: [0.0; EQ_BANDS],
            target: [0.0; EQ_BANDS],
            counter: 0,
        }
    }

    fn set_band(&mut self, i: usize, g: f32) {
        self.cur[i] = g;
        self.coefs[i] = Coefs::eq_band(self.fs, i, g);
        let was = self.active[i];
        self.active[i] = g.abs() >= 0.01;
        if !was && self.active[i] {
            self.z[i] = [[0.0; 2]; 2];
        }
    }

    /// Jump straight to these gains.
    pub fn update(&mut self, gains: &[f32; EQ_BANDS]) {
        for i in 0..EQ_BANDS {
            let g = gains[i].clamp(-EQ_RANGE_DB, EQ_RANGE_DB);
            self.target[i] = g;
            self.set_band(i, g);
        }
    }

    /// Glide towards these gains.
    pub fn set_target(&mut self, gains: &[f32; EQ_BANDS]) {
        for i in 0..EQ_BANDS {
            self.target[i] = gains[i].clamp(-EQ_RANGE_DB, EQ_RANGE_DB);
        }
    }

    /// Call once per stereo frame.
    #[inline]
    pub fn tick(&mut self) {
        self.counter += 1;
        if self.counter < 32 {
            return;
        }
        self.counter = 0;
        for i in 0..EQ_BANDS {
            let d = self.target[i] - self.cur[i];
            if d != 0.0 {
                let g = if d.abs() < 0.05 { self.target[i] } else { self.cur[i] + d * 0.25 };
                self.set_band(i, g);
            }
        }
    }

    #[inline]
    pub fn process(&mut self, ch: usize, mut x: f32) -> f32 {
        for i in 0..EQ_BANDS {
            if !self.active[i] {
                continue;
            }
            let c = &self.coefs[i];
            let z = &mut self.z[i][ch];
            let y = c.b0 * x + z[0];
            z[0] = c.b1 * x - c.a1 * y + z[1];
            z[1] = c.b2 * x - c.a2 * y;
            x = y;
        }
        x
    }
}

// -------------------------------------------------------- parametric EQ --

/// Filter types, named with Equalizer APO's codes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilterKind {
    Peaking,   // PK
    LowShelf,  // LSC
    HighShelf, // HSC
    LowPass,   // LP
    HighPass,  // HP
    Notch,     // NO
    BandPass,  // BP
}

impl FilterKind {
    pub fn from_code(c: &str) -> FilterKind {
        match c.to_ascii_uppercase().as_str() {
            "LSC" | "LS" => FilterKind::LowShelf,
            "HSC" | "HS" => FilterKind::HighShelf,
            "LP" | "LPQ" => FilterKind::LowPass,
            "HP" | "HPQ" => FilterKind::HighPass,
            "NO" => FilterKind::Notch,
            "BP" => FilterKind::BandPass,
            _ => FilterKind::Peaking,
        }
    }
    pub fn uses_gain(self) -> bool {
        matches!(self, FilterKind::Peaking | FilterKind::LowShelf | FilterKind::HighShelf)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PeqBand {
    pub enabled: bool,
    pub kind: FilterKind,
    pub freq: f32,
    pub gain_db: f32,
    pub q: f32,
}

pub const PEQ_MAX: usize = 20;
pub const PEQ_GAIN_RANGE: f32 = 24.0;

impl Coefs {
    /// RBJ cookbook designs for every parametric filter type.
    pub fn design(fs: f32, kind: FilterKind, f0: f32, gain_db: f32, q: f32) -> Self {
        let f0 = f0.clamp(10.0, fs * 0.47);
        let q = q.clamp(0.05, 40.0);
        let w0 = 2.0 * PI * f0 / fs;
        let (s, c) = w0.sin_cos();
        let alpha = s / (2.0 * q);
        let a = 10f32.powf(gain_db / 40.0);
        match kind {
            FilterKind::Peaking => {
                Self::norm(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a)
            }
            FilterKind::LowShelf | FilterKind::HighShelf => {
                let sa = 2.0 * a.sqrt() * alpha;
                if kind == FilterKind::LowShelf {
                    Self::norm(
                        a * ((a + 1.0) - (a - 1.0) * c + sa),
                        2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                        a * ((a + 1.0) - (a - 1.0) * c - sa),
                        (a + 1.0) + (a - 1.0) * c + sa,
                        -2.0 * ((a - 1.0) + (a + 1.0) * c),
                        (a + 1.0) + (a - 1.0) * c - sa,
                    )
                } else {
                    Self::norm(
                        a * ((a + 1.0) + (a - 1.0) * c + sa),
                        -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                        a * ((a + 1.0) + (a - 1.0) * c - sa),
                        (a + 1.0) - (a - 1.0) * c + sa,
                        2.0 * ((a - 1.0) - (a + 1.0) * c),
                        (a + 1.0) - (a - 1.0) * c - sa,
                    )
                }
            }
            FilterKind::LowPass => {
                Self::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
            }
            FilterKind::HighPass => {
                Self::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha)
            }
            FilterKind::Notch => Self::norm(1.0, -2.0 * c, 1.0, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
            FilterKind::BandPass => Self::norm(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * c, 1.0 - alpha),
        }
    }
}

#[derive(Clone, Copy)]
struct PeqSlot {
    target: PeqBand,
    cur_f: f32,
    cur_g: f32,
    cur_q: f32,
    kind: FilterKind,
    on: bool,
    coefs: Coefs,
    z: [[f32; 2]; 2],
}

/// Equalizer APO-style parametric EQ: a preamp followed by up to 20 filters.
/// Frequency (geometric), gain and Q glide to their targets, so dragging a
/// filter on the curve is click-free.
pub struct ParametricEq {
    fs: f32,
    slots: [PeqSlot; PEQ_MAX],
    n: usize,
    enabled: bool,
    pre_cur: f32,
    pre_target: f32,
    counter: u32,
}

impl ParametricEq {
    pub fn new(fs: f32) -> Self {
        let band = PeqBand { enabled: false, kind: FilterKind::Peaking, freq: 1000.0, gain_db: 0.0, q: 1.0 };
        let slot = PeqSlot {
            target: band,
            cur_f: 1000.0,
            cur_g: 0.0,
            cur_q: 1.0,
            kind: FilterKind::Peaking,
            on: false,
            coefs: Coefs::IDENTITY,
            z: [[0.0; 2]; 2],
        };
        Self { fs, slots: [slot; PEQ_MAX], n: 0, enabled: false, pre_cur: 1.0, pre_target: 1.0, counter: 0 }
    }

    pub fn set(&mut self, enabled: bool, preamp_db: f32, bands: &[PeqBand]) {
        self.enabled = enabled;
        self.pre_target = 10f32.powf(preamp_db.clamp(-30.0, 30.0) / 20.0);
        let n = bands.len().min(PEQ_MAX);
        for i in 0..PEQ_MAX {
            let s = &mut self.slots[i];
            if i < n {
                let b = bands[i];
                let fresh = i >= self.n || s.kind != b.kind || (!s.on && b.enabled);
                s.target = PeqBand {
                    freq: b.freq.clamp(10.0, 24_000.0),
                    gain_db: b.gain_db.clamp(-PEQ_GAIN_RANGE, PEQ_GAIN_RANGE),
                    q: b.q.clamp(0.05, 40.0),
                    ..b
                };
                if fresh {
                    // new filter or type change: start exactly at the target, clear state
                    s.kind = b.kind;
                    s.cur_f = s.target.freq;
                    s.cur_g = s.target.gain_db;
                    s.cur_q = s.target.q;
                    s.z = [[0.0; 2]; 2];
                    s.coefs = Coefs::design(self.fs, s.kind, s.cur_f, s.cur_g, s.cur_q);
                }
                s.on = b.enabled;
            } else {
                s.on = false;
            }
        }
        self.n = n;
    }

    #[inline]
    pub fn tick(&mut self) {
        self.counter += 1;
        if self.counter < 32 {
            return;
        }
        self.counter = 0;
        for s in self.slots[..self.n].iter_mut() {
            if !s.on {
                continue;
            }
            let t = s.target;
            if s.cur_f != t.freq || s.cur_g != t.gain_db || s.cur_q != t.q {
                let step = |c: f32, t: f32, eps: f32| if (t - c).abs() < eps { t } else { c + (t - c) * 0.25 };
                s.cur_f = (step(s.cur_f.ln(), t.freq.ln(), 0.002)).exp();
                if (s.cur_f - t.freq).abs() < 0.05 {
                    s.cur_f = t.freq;
                }
                s.cur_g = step(s.cur_g, t.gain_db, 0.02);
                s.cur_q = step(s.cur_q, t.q, 0.002);
                s.coefs = Coefs::design(self.fs, s.kind, s.cur_f, s.cur_g, s.cur_q);
            }
        }
    }

    /// Smoothed preamp gain for this frame (call once per frame).
    #[inline]
    pub fn preamp(&mut self, k: f32) -> f32 {
        let target = if self.enabled { self.pre_target } else { 1.0 };
        self.pre_cur += (target - self.pre_cur) * k;
        self.pre_cur
    }

    #[inline]
    pub fn process(&mut self, ch: usize, mut x: f32) -> f32 {
        if !self.enabled {
            return x;
        }
        for s in self.slots[..self.n].iter_mut() {
            if !s.on {
                continue;
            }
            let c = s.coefs;
            let z = &mut s.z[ch];
            let y = c.b0 * x + z[0];
            z[0] = c.b1 * x - c.a1 * y + z[1];
            z[1] = c.b2 * x - c.a2 * y;
            x = y;
        }
        x
    }

    /// Combined magnitude response in dB (preamp included), for tests.
    pub fn response_db(&self, f: f32) -> f32 {
        let mut db = 20.0 * self.pre_target.log10();
        for s in &self.slots[..self.n] {
            if s.on {
                db += Coefs::design(self.fs, s.kind, s.target.freq, s.target.gain_db, s.target.q).response_db(self.fs, f);
            }
        }
        db
    }
}

// ---------------------------------------------------------- pitch shift --

/// Low-latency pitch shifter: two crossfaded read taps sweeping through a
/// delay line (the classic "rotating tape head" design). Latency is about
/// half the window (~20 ms at 48 kHz). Pitch changes without changing speed.
pub struct PitchShifter {
    buf: Vec<f32>,
    mask: usize,
    write: usize,
    phase: f32,
    window: f32,
}

impl PitchShifter {
    pub fn new(fs: f32) -> Self {
        let window = (fs * 0.042).round().max(256.0); // ~42 ms
        let size = ((window as usize) * 2 + 8).next_power_of_two();
        Self { buf: vec![0.0; size], mask: size - 1, write: 0, phase: 0.0, window }
    }

    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let pos = self.write as f32 - delay;
        let pos = if pos < 0.0 { pos + self.buf.len() as f32 } else { pos };
        let i = pos.floor();
        let frac = pos - i;
        let i0 = (i as usize) & self.mask;
        let i1 = (i0 + 1) & self.mask;
        self.buf[i0] + (self.buf[i1] - self.buf[i0]) * frac
    }

    /// `ratio` = 2^(semitones/12). ratio 1.0 passes through with no latency.
    #[inline]
    pub fn process(&mut self, x: f32, ratio: f32) -> f32 {
        self.buf[self.write] = x;
        let out = if (ratio - 1.0).abs() < 1e-4 {
            x
        } else {
            self.phase += (1.0 - ratio) / self.window;
            self.phase -= self.phase.floor(); // wrap into [0,1)
            let p2 = {
                let p = self.phase + 0.5;
                p - p.floor()
            };
            let s1 = self.read(self.phase * self.window);
            let s2 = self.read(p2 * self.window);
            // sin^2 windows sum to exactly 1 and are 0 where each tap jumps.
            let g1 = (PI * self.phase).sin().powi(2);
            let g2 = (PI * p2).sin().powi(2);
            s1 * g1 + s2 * g2
        };
        self.write = (self.write + 1) & self.mask;
        out
    }
}

// --------------------------------------------------------------- limiter --

/// Stereo-linked **look-ahead** peak limiter.
///
/// The signal is delayed by 5 ms while the gain computer looks at the
/// incoming samples, so gain reduction is already in place when a peak
/// reaches the output. Compared with an instant-attack limiter this keeps
/// transients clean and lets the boost get much louder before it sounds
/// squashed. A soft clip after it guarantees the output never exceeds ±1.
pub struct Limiter {
    delay: Vec<[f32; 2]>,
    pos: usize,
    hold: usize,
    held: f32,
    gain: f32,
    attack: f32,
    release: f32,
    threshold: f32,
}

impl Limiter {
    pub const LOOKAHEAD_MS: f32 = 5.0;

    pub fn new(fs: f32) -> Self {
        let la = ((fs * Self::LOOKAHEAD_MS / 1000.0) as usize).max(16);
        Self {
            delay: vec![[0.0; 2]; la],
            pos: 0,
            hold: 0,
            held: 1.0,
            gain: 1.0,
            // reach ~99.3 % of the needed reduction within the look-ahead window
            attack: 1.0 - (-5.0 / la as f32).exp(),
            release: 1.0 - (-1.0 / (fs * 0.06)).exp(),
            threshold: 0.93,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let la = self.delay.len();
        let peak = l.abs().max(r.abs());
        let need = if peak > self.threshold { self.threshold / peak } else { 1.0 };
        // hold the deepest reduction until that peak has left the delay line
        if need <= self.held {
            self.held = need;
            self.hold = la;
        } else if self.hold > 0 {
            self.hold -= 1;
        } else {
            self.held = need;
        }
        let k = if self.held < self.gain { self.attack } else { self.release };
        self.gain += (self.held - self.gain) * k;

        let [dl, dr] = self.delay[self.pos];
        self.delay[self.pos] = [l, r];
        self.pos = (self.pos + 1) % la;
        (soft_clip(dl * self.gain), soft_clip(dr * self.gain))
    }
}

#[inline]
pub fn soft_clip(x: f32) -> f32 {
    const K: f32 = 0.95;
    let a = x.abs();
    if a <= K {
        x
    } else {
        // smooth knee from K to 1.0
        let over = a - K;
        let y = K + (1.0 - K) * (over / (1.0 - K)).tanh();
        y.copysign(x)
    }
}

/// Loudness compressor used by the booster.
///
/// A peak limiter alone can't make already-loud music much louder: it can
/// only turn the peaks down. This stage gently narrows the gap between loud
/// and quiet parts (soft knee, stereo-linked), so after the limiter the
/// average level, and therefore the perceived loudness, rises a lot more.
/// Its strength follows the boost: off at 100 %, about 3.5:1 at 600 %.
pub struct Compressor {
    env: f32,
    attack: f32,
    release: f32,
}

impl Compressor {
    const THRESHOLD_DB: f32 = -16.0;
    const KNEE_DB: f32 = 8.0;

    pub fn new(fs: f32) -> Self {
        Self { env: 0.0, attack: 1.0 - (-1.0 / (fs * 0.004)).exp(), release: 1.0 - (-1.0 / (fs * 0.12)).exp() }
    }

    /// `amount` 0..1. Returns the gain to apply (≤ 1 below makeup).
    #[inline]
    pub fn gain(&mut self, l: f32, r: f32, amount: f32) -> f32 {
        let x = l.abs().max(r.abs());
        let k = if x > self.env { self.attack } else { self.release };
        self.env += (x - self.env) * k;
        if amount <= 0.0 {
            return 1.0;
        }
        let ratio = 1.0 + 2.5 * amount;
        let lvl = 20.0 * (self.env + 1e-9).log10();
        let over = lvl - Self::THRESHOLD_DB;
        let half = Self::KNEE_DB / 2.0;
        let gr = if over <= -half {
            0.0
        } else if over >= half {
            over * (1.0 - 1.0 / ratio)
        } else {
            (1.0 - 1.0 / ratio) * (over + half).powi(2) / (2.0 * Self::KNEE_DB)
        };
        // makeup restores the level at the threshold so quiet parts get the full boost
        let makeup = -Self::THRESHOLD_DB * (1.0 - 1.0 / ratio);
        10f32.powf((makeup - gr) / 20.0)
    }
}

// ------------------------------------------------------------- processor --

pub struct Processor {
    fs: f32,
    params: DspParams,
    eq: Equalizer,
    peq: ParametricEq,
    pitch: [PitchShifter; 2],
    ratio: f32,
    ratio_target: f32,
    limiter: Limiter,
    comp: Compressor,
    gain: f32, // smoothed boost
    width: f32,
    bal: (f32, f32),
    smooth: f32,
}

impl Processor {
    pub fn new(fs: f32) -> Self {
        Self {
            fs,
            params: DspParams::default(),
            eq: Equalizer::new(fs),
            peq: ParametricEq::new(fs),
            pitch: [PitchShifter::new(fs), PitchShifter::new(fs)],
            ratio: 1.0,
            ratio_target: 1.0,
            limiter: Limiter::new(fs),
            comp: Compressor::new(fs),
            gain: 1.0,
            width: 1.0,
            bal: (1.0, 1.0),
            smooth: 1.0 - (-1.0 / (fs * 0.03)).exp(), // ~30 ms glide
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.fs
    }

    /// Parametric EQ (preamp + filters). Allocation-free; safe in the callback.
    pub fn set_peq(&mut self, enabled: bool, preamp_db: f32, bands: &[PeqBand]) {
        self.peq.set(enabled, preamp_db, bands);
    }

    pub fn set_params(&mut self, p: DspParams) {
        self.eq.set_target(&p.eq_db);
        let st = p.pitch_st.clamp(-PITCH_RANGE_ST, PITCH_RANGE_ST);
        self.ratio_target = 2f32.powf(st / 12.0);
        if st.abs() < 1e-3 {
            self.ratio_target = 1.0;
        }
        self.params = p;
    }

    #[inline]
    fn glide(cur: &mut f32, target: f32, k: f32) {
        let d = target - *cur;
        *cur = if d.abs() < 1e-5 { target } else { *cur + d * k };
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if self.params.bypass {
            return (l, r);
        }
        let k = self.smooth;
        Self::glide(&mut self.ratio, self.ratio_target, k);
        let mut l = self.pitch[0].process(l, self.ratio);
        let mut r = self.pitch[1].process(r, self.ratio);

        // parametric EQ: preamp, then filters (Equalizer APO order)
        let pre = self.peq.preamp(k);
        l *= pre;
        r *= pre;
        self.peq.tick();
        l = self.peq.process(0, l);
        r = self.peq.process(1, r);

        self.eq.tick();
        l = self.eq.process(0, l);
        r = self.eq.process(1, r);

        // stereo width (mid/side) and balance
        Self::glide(&mut self.width, self.params.width.clamp(0.0, 2.0), k);
        let m = (l + r) * 0.5;
        let s = (l - r) * 0.5 * self.width;
        l = m + s;
        r = m - s;
        let b = self.params.balance.clamp(-1.0, 1.0);
        let (tl, tr) = (if b > 0.0 { 1.0 - b } else { 1.0 }, if b < 0.0 { 1.0 + b } else { 1.0 });
        Self::glide(&mut self.bal.0, tl, k);
        Self::glide(&mut self.bal.1, tr, k);
        l *= self.bal.0;
        r *= self.bal.1;

        Self::glide(&mut self.gain, self.params.boost.clamp(0.0, BOOST_MAX), k);
        l *= self.gain;
        r *= self.gain;
        if self.params.limiter {
            // loudness compression only kicks in above 100 % boost
            let amount = ((self.gain - 1.0) / 4.0).clamp(0.0, 1.0);
            let g = self.comp.gain(l, r, amount);
            self.limiter.process(l * g, r * g)
        } else {
            (l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0))
        }
    }
}

// ------------------------------------------------------------- resampler --

/// Linear resampler used when the capture and playback devices run at
/// different sample rates (e.g. 44.1 kHz cable -> 48 kHz speakers).
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: (f32, f32),
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self { step: in_rate as f64 / out_rate as f64, pos: 0.0, prev: (0.0, 0.0) }
    }

    pub fn is_passthrough(&self) -> bool {
        (self.step - 1.0).abs() < 1e-9
    }

    #[inline]
    pub fn push(&mut self, l: f32, r: f32, mut out: impl FnMut(f32, f32)) {
        if self.is_passthrough() {
            out(l, r);
            return;
        }
        while self.pos < 1.0 {
            let t = self.pos as f32;
            out(self.prev.0 + (l - self.prev.0) * t, self.prev.1 + (r - self.prev.1) * t);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = (l, r);
    }
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f32 = 48_000.0;

    fn sine(f: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| (2.0 * PI * f * i as f32 / FS).sin() * 0.5).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    /// Dominant frequency by zero-crossing count.
    fn zc_freq(x: &[f32]) -> f32 {
        let c = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        c as f32 * FS / x.len() as f32
    }

    #[test]
    fn peaking_hits_target_gain() {
        for &g in &[-12.0, -6.0, 3.0, 12.0] {
            let c = Coefs::peaking(FS, 1000.0, EQ_Q, g);
            let r = c.response_db(FS, 1000.0);
            assert!((r - g).abs() < 0.05, "gain {g} got {r}");
            assert!(c.response_db(FS, 20.0).abs() < 0.3);
        }
    }

    #[test]
    fn shelves_behave() {
        let ls = Coefs::low_shelf(FS, 50.0, 9.0);
        assert!((ls.response_db(FS, 10.0) - 9.0).abs() < 0.5);
        assert!(ls.response_db(FS, 5000.0).abs() < 0.2);
        let hs = Coefs::high_shelf(FS, 11200.0, 6.0);
        assert!((hs.response_db(FS, 22000.0) - 6.0).abs() < 1.0);
        assert!(hs.response_db(FS, 200.0).abs() < 0.2);
    }

    #[test]
    fn eq_boosts_band_in_time_domain() {
        let mut eq = Equalizer::new(FS);
        let mut g = [0.0; EQ_BANDS];
        g[5] = 12.0; // 1 kHz
        eq.update(&g);
        let x = sine(1000.0, 48_000);
        let y: Vec<f32> = x.iter().map(|&s| eq.process(0, s)).collect();
        let ratio_db = 20.0 * (rms(&y[4800..]) / rms(&x[4800..])).log10();
        assert!((ratio_db - 12.0).abs() < 0.5, "got {ratio_db}");
    }

    #[test]
    fn pitch_up_one_octave() {
        let mut ps = PitchShifter::new(FS);
        let x = sine(220.0, 96_000);
        let y: Vec<f32> = x.iter().map(|&s| ps.process(s, 2.0)).collect();
        let f = zc_freq(&y[9600..]);
        assert!((f - 440.0).abs() < 20.0, "expected ~440 Hz, got {f}");
    }

    #[test]
    fn pitch_down_five_semitones() {
        let mut ps = PitchShifter::new(FS);
        let ratio = 2f32.powf(-5.0 / 12.0);
        let x = sine(440.0, 96_000);
        let y: Vec<f32> = x.iter().map(|&s| ps.process(s, ratio)).collect();
        let f = zc_freq(&y[9600..]);
        let want = 440.0 * ratio;
        assert!((f - want).abs() < 15.0, "expected ~{want} Hz, got {f}");
    }

    #[test]
    fn pitch_unity_is_transparent() {
        let mut ps = PitchShifter::new(FS);
        for i in 0..1000 {
            let s = (i as f32 * 0.01).sin();
            assert_eq!(ps.process(s, 1.0), s);
        }
    }

    #[test]
    fn boost_with_limiter_never_clips() {
        let mut p = Processor::new(FS);
        p.set_params(DspParams { boost: 6.0, ..Default::default() });
        let x = sine(100.0, 48_000);
        let mut max = 0f32;
        for &s in &x {
            let (l, r) = p.process(s, s);
            max = max.max(l.abs()).max(r.abs());
        }
        assert!(max <= 1.0, "peak {max}");
        assert!(max > 0.9, "should be loud, peak {max}");
    }

    #[test]
    fn lookahead_limiter_catches_peaks_cleanly() {
        // A loud, already-mastered signal boosted 6x: the limiter (not the
        // soft clipper) should do the work, keeping peaks just under 0.95.
        let mut p = Processor::new(FS);
        p.set_params(DspParams { boost: 6.0, ..Default::default() });
        let x = sine(220.0, 96_000);
        let mut max = 0f32;
        for (i, &s) in x.iter().enumerate() {
            let (l, _) = p.process(s * 1.8, s * 1.8);
            if i > 9_600 {
                max = max.max(l.abs());
            }
        }
        assert!(max < 0.955, "limiter let peaks through: {max}");
        assert!(max > 0.88, "too much gain reduction: {max}");
    }

    /// Music-like test signal: noise bed + kick drum, ~15 dB crest factor.
    fn music(n: usize, level: f32) -> Vec<f32> {
        let mut seed = 12345u32;
        let mut lp = 0.0f32;
        (0..n)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let w = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
                lp += (w - lp) * 0.15;
                let t = i as f32 / FS;
                let kick = (-(t * 2.0).fract() * 18.0).exp() * (2.0 * PI * 55.0 * t).sin();
                level * (0.55 * kick + 0.77 * lp * (0.6 + 0.4 * (t * 0.7).sin()))
            })
            .collect()
    }

    fn loudness_gain_db(level: f32, boost: f32) -> (f32, f32) {
        let x = music(96_000, level);
        let mut p = Processor::new(FS);
        p.set_params(DspParams { boost, ..Default::default() });
        let y: Vec<f32> = x.iter().map(|&s| p.process(s, s).0).collect();
        let peak = y[9_600..].iter().fold(0f32, |m, v| m.max(v.abs()));
        (20.0 * (rms(&y[9_600..]) / rms(&x[9_600..])).log10(), peak)
    }

    #[test]
    fn boost_makes_music_much_louder() {
        // streamed music (~-24 dBFS RMS): 500 % must add well over 12 dB
        let (g, pk) = loudness_gain_db(0.36, 5.0);
        assert!(g > 12.5, "only +{g:.1} dB");
        assert!(pk < 0.96, "peak {pk}");
        // quiet audio (videos / podcasts): close to the full boost
        let (g, _) = loudness_gain_db(0.18, 6.0);
        assert!(g > 18.0, "only +{g:.1} dB");
        // already-loud music still gets clearly louder without clipping
        let (g, pk) = loudness_gain_db(0.71, 5.0);
        assert!(g > 7.0 && pk < 0.96, "+{g:.1} dB, peak {pk}");
    }

    #[test]
    fn boost_is_monotonic() {
        let mut last = -100.0;
        for b in [1.0f32, 1.5, 2.0, 3.0, 4.0, 5.0, 6.0] {
            let (g, _) = loudness_gain_db(0.36, b);
            assert!(g >= last - 0.05, "louder knob gave quieter sound at {b}");
            last = g;
        }
    }

    #[test]
    fn boost_increases_level_of_quiet_audio() {
        let mut p = Processor::new(FS);
        p.set_params(DspParams { boost: 4.0, ..Default::default() });
        let x: Vec<f32> = sine(500.0, 48_000).iter().map(|v| v * 0.1).collect(); // -26 dBFS
        let y: Vec<f32> = x.iter().map(|&s| p.process(s, s).0).collect();
        let gain = rms(&y[9600..]) / rms(&x[9600..]);
        assert!(gain >= 3.9, "gain {gain}");
    }

    #[test]
    fn bypass_is_bit_exact() {
        let mut p = Processor::new(FS);
        p.set_params(DspParams { boost: 5.0, bypass: true, pitch_st: 7.0, ..Default::default() });
        assert_eq!(p.process(0.3, -0.2), (0.3, -0.2));
    }

    #[test]
    fn eq_glides_to_target() {
        let mut eq = Equalizer::new(FS);
        let mut g = [0.0; EQ_BANDS];
        g[3] = 9.0;
        eq.set_target(&g);
        for _ in 0..32 {
            eq.tick();
        }
        assert!(eq.cur[3] > 0.0 && eq.cur[3] < 9.0, "should be gliding: {}", eq.cur[3]);
        for _ in 0..4800 {
            eq.tick();
        }
        assert_eq!(eq.cur[3], 9.0);
    }

    #[test]
    fn width_zero_is_mono_and_balance_left() {
        let mut p = Processor::new(FS);
        p.set_params(DspParams { width: 0.0, limiter: false, ..Default::default() });
        let mut out = (0.0, 0.0);
        for _ in 0..48_000 {
            out = p.process(0.4, -0.2);
        }
        assert!((out.0 - out.1).abs() < 1e-4, "{out:?}");
        p.set_params(DspParams { balance: -1.0, limiter: false, ..Default::default() });
        for _ in 0..48_000 {
            out = p.process(0.4, 0.4);
        }
        assert!(out.1.abs() < 1e-3 && out.0 > 0.39, "{out:?}");
    }

    fn apo_bands() -> Vec<PeqBand> {
        let b = |f, g, q| PeqBand { enabled: true, kind: FilterKind::Peaking, freq: f, gain_db: g, q };
        vec![b(80.0, -5.0, 10.0), b(1800.0, 3.0, 1.0), b(3200.0, 3.5, 1.2), b(8000.0, -2.0, 1.0)]
    }

    #[test]
    fn peq_design_hits_targets() {
        let pk = Coefs::design(FS, FilterKind::Peaking, 80.0, -5.0, 10.0);
        assert!((pk.response_db(FS, 80.0) + 5.0).abs() < 0.05);
        assert!(pk.response_db(FS, 160.0).abs() < 0.3, "Q10 must be narrow");
        let lp = Coefs::design(FS, FilterKind::LowPass, 1000.0, 0.0, 0.707);
        assert!((lp.response_db(FS, 1000.0) + 3.0).abs() < 0.2);
        assert!(lp.response_db(FS, 4000.0) < -20.0 && lp.response_db(FS, 100.0).abs() < 0.1);
        let hp = Coefs::design(FS, FilterKind::HighPass, 100.0, 0.0, 0.707);
        assert!(hp.response_db(FS, 25.0) < -20.0 && hp.response_db(FS, 2000.0).abs() < 0.1);
        let no = Coefs::design(FS, FilterKind::Notch, 1000.0, 0.0, 5.0);
        assert!(no.response_db(FS, 1000.0) < -40.0 && no.response_db(FS, 3000.0).abs() < 0.3);
        let bp = Coefs::design(FS, FilterKind::BandPass, 1000.0, 0.0, 1.0);
        assert!(bp.response_db(FS, 1000.0).abs() < 0.1 && bp.response_db(FS, 100.0) < -15.0);
        let ls = Coefs::design(FS, FilterKind::LowShelf, 100.0, 6.0, 0.707);
        assert!((ls.response_db(FS, 15.0) - 6.0).abs() < 0.3 && ls.response_db(FS, 3000.0).abs() < 0.1);
        let hs = Coefs::design(FS, FilterKind::HighShelf, 8000.0, -4.0, 0.707);
        assert!((hs.response_db(FS, 21000.0) + 4.0).abs() < 0.6 && hs.response_db(FS, 500.0).abs() < 0.1);
    }

    #[test]
    fn peq_matches_equalizer_apo_config() {
        // Preamp -5 dB; PK 80 Hz -5 dB Q10; PK 1.8 kHz +3 dB Q1; PK 3.2 kHz +3.5 dB Q1.2; PK 8 kHz -2 dB Q1
        let mut peq = ParametricEq::new(FS);
        peq.set(true, -5.0, &apo_bands());
        let r = |f| peq.response_db(f);
        assert!((r(80.0) - (-10.0)).abs() < 0.2, "80 Hz: {}", r(80.0));
        assert!((r(1000.0) - (-5.0 + 1.2)).abs() < 1.2, "1 kHz: {}", r(1000.0));
        assert!(r(2400.0) > -1.0 && r(2400.0) < 1.5, "2.4 kHz: {}", r(2400.0));
        assert!((r(20.0) + 5.0).abs() < 0.2, "20 Hz is just the preamp: {}", r(20.0));
    }

    #[test]
    fn peq_time_domain_and_toggle() {
        let mut p = Processor::new(FS);
        p.set_params(DspParams { limiter: false, ..Default::default() });
        p.set_peq(true, -5.0, &apo_bands());
        let x = sine(80.0, 96_000);
        let y: Vec<f32> = x.iter().map(|&s| p.process(s, s).0).collect();
        let db = 20.0 * (rms(&y[19_200..]) / rms(&x[19_200..])).log10();
        assert!((db + 10.0).abs() < 0.5, "80 Hz through APO curve: {db}");
        // switching it off returns to unity
        p.set_peq(false, -5.0, &apo_bands());
        let y: Vec<f32> = x.iter().map(|&s| p.process(s, s).0).collect();
        let db = 20.0 * (rms(&y[19_200..]) / rms(&x[19_200..])).log10();
        assert!(db.abs() < 0.2, "disabled: {db}");
    }

    #[test]
    fn peq_glides_and_is_stable() {
        let mut peq = ParametricEq::new(FS);
        peq.set(true, 0.0, &apo_bands());
        let mut moved = apo_bands();
        moved[1].freq = 5000.0;
        moved[1].gain_db = 12.0;
        moved[1].q = 4.0;
        peq.set(true, 0.0, &moved);
        let mut mx = 0f32;
        for i in 0..48_000 {
            peq.tick();
            let v = peq.process(0, ((i as f32) * 0.37).sin() * 0.3);
            assert!(v.is_finite());
            mx = mx.max(v.abs());
        }
        assert!(mx < 3.0);
        assert_eq!(peq.slots[1].cur_f, 5000.0);
        assert_eq!(peq.slots[1].cur_g, 12.0);
    }

    #[test]
    fn resampler_ratio() {
        let mut rs = Resampler::new(44_100, 48_000);
        let mut n = 0;
        for i in 0..44_100 {
            rs.push(i as f32, 0.0, |_, _| n += 1);
        }
        assert!((n as i32 - 48_000).abs() <= 2, "{n}");
    }
}
