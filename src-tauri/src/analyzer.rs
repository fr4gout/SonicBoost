//! Spectrum / waveform analysis for the visualizer (dependency-free FFT).

use std::f32::consts::PI;

pub const FFT_SIZE: usize = 2048;
pub const BANDS: usize = 72;
pub const WAVE_POINTS: usize = 256;

#[derive(Clone, Copy)]
struct C {
    re: f32,
    im: f32,
}

/// In-place iterative radix-2 FFT with precomputed twiddles.
pub struct Fft {
    n: usize,
    tw: Vec<C>,
    rev: Vec<usize>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two());
        let bits = n.trailing_zeros();
        let rev = (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        let tw = (0..n / 2)
            .map(|k| {
                let a = -2.0 * PI * k as f32 / n as f32;
                C { re: a.cos(), im: a.sin() }
            })
            .collect();
        Self { n, tw, rev }
    }

    /// Returns magnitudes of bins 0..n/2.
    pub fn magnitudes(&self, input: &[f32], out: &mut [f32]) {
        let n = self.n;
        let mut d: Vec<C> = (0..n).map(|i| C { re: input[self.rev[i]], im: 0.0 }).collect();
        let mut len = 2;
        while len <= n {
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let w = self.tw[k * step];
                    let a = d[start + k];
                    let b = d[start + k + len / 2];
                    let t = C { re: b.re * w.re - b.im * w.im, im: b.re * w.im + b.im * w.re };
                    d[start + k] = C { re: a.re + t.re, im: a.im + t.im };
                    d[start + k + len / 2] = C { re: a.re - t.re, im: a.im - t.im };
                }
            }
            len <<= 1;
        }
        for (i, o) in out.iter_mut().enumerate().take(n / 2) {
            *o = (d[i].re * d[i].re + d[i].im * d[i].im).sqrt();
        }
    }
}

pub struct Analyzer {
    fft: Fft,
    window: Vec<f32>,
    hist: Vec<f32>,
    tmp: Vec<f32>,
    mags: Vec<f32>,
}

/// Serialized to the UI (see `impl Serialize` in audio.rs).
#[derive(Clone)]
pub struct Frame {
    pub bands: Vec<f32>, // 0..1, log-spaced 30 Hz – 18 kHz
    pub wave: Vec<f32>,  // -1..1
    pub peak_l: f32,
    pub peak_r: f32,
    pub rms: f32,
}

impl Analyzer {
    pub fn new() -> Self {
        let n = FFT_SIZE;
        Self {
            fft: Fft::new(n),
            window: (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1) as f32).cos()).collect(),
            hist: vec![0.0; n],
            tmp: vec![0.0; n],
            mags: vec![0.0; n / 2],
        }
    }

    /// Append freshly played (mono) samples to the rolling history.
    pub fn feed(&mut self, new: &[f32]) {
        let n = FFT_SIZE;
        if new.len() >= n {
            self.hist.copy_from_slice(&new[new.len() - n..]);
        } else if !new.is_empty() {
            self.hist.copy_within(new.len().., 0);
            self.hist[n - new.len()..].copy_from_slice(new);
        }
    }

    pub fn frame(&mut self, sample_rate: f32, peak_l: f32, peak_r: f32) -> Frame {
        let n = FFT_SIZE;
        for i in 0..n {
            self.tmp[i] = self.hist[i] * self.window[i];
        }
        self.fft.magnitudes(&self.tmp, &mut self.mags);

        let bin_hz = sample_rate / n as f32;
        let f_lo = 30.0f32;
        let f_hi = 18_000f32.min(sample_rate * 0.49);
        let ratio = (f_hi / f_lo).powf(1.0 / BANDS as f32);
        let scale = 4.0 / n as f32; // hann coherent gain 0.5, one-sided x2
        let mut bands = Vec::with_capacity(BANDS);
        for b in 0..BANDS {
            let lo = f_lo * ratio.powi(b as i32);
            let hi = lo * ratio;
            let mut i0 = (lo / bin_hz).floor() as usize;
            let mut i1 = (hi / bin_hz).ceil() as usize;
            i0 = i0.clamp(1, n / 2 - 1);
            i1 = i1.clamp(i0 + 1, n / 2);
            let m = self.mags[i0..i1].iter().cloned().fold(0.0f32, f32::max) * scale;
            let db = 20.0 * (m + 1e-9).log10();
            // gentle tilt so highs are visible like on hardware analyzers
            let tilt = 3.0 * (b as f32 / BANDS as f32) * 4.0;
            bands.push(((db + tilt + 66.0) / 66.0).clamp(0.0, 1.0));
        }

        let stride = (n / 2) / WAVE_POINTS;
        let wave = (0..WAVE_POINTS).map(|i| self.hist[n / 2 + i * stride].clamp(-1.0, 1.0)).collect();
        let rms = (self.hist.iter().map(|v| v * v).sum::<f32>() / n as f32).sqrt();
        Frame { bands, wave, peak_l, peak_r, rms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_tone() {
        let fft = Fft::new(1024);
        let x: Vec<f32> = (0..1024).map(|i| (2.0 * PI * 64.0 * i as f32 / 1024.0).sin()).collect();
        let mut m = vec![0.0; 512];
        fft.magnitudes(&x, &mut m);
        let (idx, _) = m.iter().enumerate().fold((0, 0.0), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
        assert_eq!(idx, 64);
        assert!((m[64] - 512.0).abs() < 1.0);
    }
}
