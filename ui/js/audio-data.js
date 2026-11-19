// Smoothed audio features shared by every visual (60/120/144 Hz safe).
export class AudioData {
  constructor(n = 72) {
    this.n = n;
    this.target = new Float32Array(n);
    this.bands = new Float32Array(n);
    this.peaks = new Float32Array(n);
    this.peakVel = new Float32Array(n);
    this.waveT = new Float32Array(256);
    this.wave = new Float32Array(256);
    this.bass = 0; this.mid = 0; this.high = 0; this.energy = 0;
    this.peakL = 0; this.peakR = 0;
    this.beat = 0;           // 1 on a kick, decays to 0
    this.onBeat = null;
    this._avgBass = 0; this._lastBeat = 0;
    this.intensity = 1;
  }

  feed(f) {
    if (!f || !f.bands) return;
    this.target.set(f.bands.slice(0, this.n));
    if (f.wave) this.waveT.set(f.wave.slice(0, 256));
    this.peakL = Math.max(this.peakL * 0.7, f.peak_l || 0);
    this.peakR = Math.max(this.peakR * 0.7, f.peak_r || 0);
  }

  update(dt) {
    // frame-rate independent exponential smoothing
    const up = 1 - Math.exp(-dt * 28);
    const down = 1 - Math.exp(-dt * 7.5);
    const k = this.intensity;
    let b = 0, m = 0, h = 0;
    for (let i = 0; i < this.n; i++) {
      const t = Math.min(1, this.target[i] * k);
      const v = this.bands[i];
      this.bands[i] = v + (t - v) * (t > v ? up : down);
      // peak caps with gravity
      if (this.bands[i] >= this.peaks[i]) { this.peaks[i] = this.bands[i]; this.peakVel[i] = 0; }
      else { this.peakVel[i] += dt * 1.6; this.peaks[i] = Math.max(0, this.peaks[i] - this.peakVel[i] * dt); }
      if (i < 10) b += this.bands[i]; else if (i < 44) m += this.bands[i]; else h += this.bands[i];
    }
    this.bass = b / 10; this.mid = m / 34; this.high = h / (this.n - 44);
    this.energy += ((this.bass * 0.6 + this.mid * 0.3 + this.high * 0.1) - this.energy) * (1 - Math.exp(-dt * 10));
    const wk = 1 - Math.exp(-dt * 22);
    for (let i = 0; i < 256; i++) this.wave[i] += (this.waveT[i] - this.wave[i]) * wk;

    // beat detection on the bass envelope
    this._avgBass += (this.bass - this._avgBass) * (1 - Math.exp(-dt * 2));
    const now = performance.now();
    if (this.bass > this._avgBass * 1.18 + 0.04 && this.bass > 0.3 && now - this._lastBeat > 180) {
      this._lastBeat = now;
      this.beat = 1;
      this.onBeat && this.onBeat(this.bass);
    }
    this.beat *= Math.exp(-dt * 6);
    this.peakL *= Math.exp(-dt * 3); this.peakR *= Math.exp(-dt * 3);
  }

  /** Resample bands to `count` values (linear). */
  sample(count, out) {
    out = out || new Float32Array(count);
    for (let i = 0; i < count; i++) {
      const x = (i / Math.max(1, count - 1)) * (this.n - 1);
      const a = Math.floor(x), f = x - a;
      out[i] = this.bands[a] * (1 - f) + (this.bands[Math.min(this.n - 1, a + 1)] || 0) * f;
    }
    return out;
  }
}
