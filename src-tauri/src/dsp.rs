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

/// Stereo-linked peak limiter + soft clip so heavy boost never hard-clips.
pub struct Limiter {
    gain: f32,
    release: f32,
    threshold: f32,
}

impl Limiter {
    pub fn new(fs: f32) -> Self {
        Self { gain: 1.0, release: 1.0 - (-1.0 / (fs * 0.12)).exp(), threshold: 0.94 }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        let peak = l.abs().max(r.abs());
        let target = if peak > self.threshold { self.threshold / peak } else { 1.0 };
        if target < self.gain {
            self.gain = target; // instant attack
        } else {
            self.gain += (target - self.gain) * self.release;
        }
        (soft_clip(l * self.gain), soft_clip(r * self.gain))
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

// ------------------------------------------------------------- processor --

pub struct Processor {
    fs: f32,
    params: DspParams,
    eq: Equalizer,
    pitch: [PitchShifter; 2],
    ratio: f32,
    ratio_target: f32,
    limiter: Limiter,
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
            pitch: [PitchShifter::new(fs), PitchShifter::new(fs)],
            ratio: 1.0,
            ratio_target: 1.0,
            limiter: Limiter::new(fs),
            gain: 1.0,
            width: 1.0,
            bal: (1.0, 1.0),
            smooth: 1.0 - (-1.0 / (fs * 0.03)).exp(), // ~30 ms glide
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.fs
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
            self.limiter.process(l, r)
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
    fn boost_increases_level_of_quiet_audio() {
        let mut p = Processor::new(FS);
        p.set_params(DspParams { boost: 4.0, ..Default::default() });
        let x: Vec<f32> = sine(500.0, 48_000).iter().map(|v| v * 0.1).collect(); // -26 dBFS
        let y: Vec<f32> = x.iter().map(|&s| p.process(s, s).0).collect();
        let gain = rms(&y[9600..]) / rms(&x[9600..]);
        assert!((gain - 4.0).abs() < 0.1, "gain {gain}");
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
