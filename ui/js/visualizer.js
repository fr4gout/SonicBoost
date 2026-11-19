// Monochrome visualizers: Bars, Radial, Wave, Galaxy + the knob ring.
const TAU = Math.PI * 2;

function fitCanvas(c) {
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const r = c.getBoundingClientRect();
  const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
  if (c.width !== w || c.height !== h) { c.width = w; c.height = h; }
  return { w, h, dpr };
}

export class Visualizer {
  constructor(canvas, audio) {
    this.c = canvas; this.g = canvas.getContext('2d'); this.audio = audio;
    this.mode = 'radial'; this.ink = '255,255,255'; this.bg = '4,4,4';
    this.rot = 0; this.t = 0;
    this.particles = [];
    this.stars = Array.from({ length: 520 }, () => ({
      a: Math.random() * TAU, r: Math.pow(Math.random(), 0.6), s: 0.3 + Math.random() * 1.6,
      z: Math.random(), band: Math.floor(Math.random() * 60),
    }));
    this.tmp = new Float32Array(256);
    this.fade = 1; // crossfade between modes
  }

  setMode(m) {
    if (m === this.mode) return;
    this.mode = m; this.fade = 0; this.particles.length = 0;
  }

  burst(power) {
    if (this.mode !== 'radial' && this.mode !== 'galaxy') return;
    const n = Math.round(14 + power * 30);
    for (let i = 0; i < n; i++) {
      const a = Math.random() * TAU, sp = 0.25 + Math.random() * 0.9 * power;
      this.particles.push({ a, d: 0, sp, life: 1, size: 0.6 + Math.random() * 1.8 });
    }
    if (this.particles.length > 600) this.particles.splice(0, this.particles.length - 600);
  }

  draw(dt) {
    const { w, h, dpr } = fitCanvas(this.c);
    const g = this.g;
    this.t += dt;
    this.fade = Math.min(1, this.fade + dt * 2.5);
    const trail = this.mode === 'wave' || this.mode === 'galaxy';
    g.globalCompositeOperation = 'source-over';
    if (trail) {
      g.fillStyle = `rgba(${this.bg},${0.16 + (1 - this.fade) * 0.6})`;
      g.fillRect(0, 0, w, h);
      // keep the panel see-through: punch the trail back out slowly
      g.globalCompositeOperation = 'destination-out';
      g.fillStyle = 'rgba(0,0,0,.08)'; g.fillRect(0, 0, w, h);
      g.globalCompositeOperation = 'source-over';
    } else {
      g.clearRect(0, 0, w, h);
    }
    g.save();
    g.globalAlpha = 0.25 + 0.75 * easeOut(this.fade);
    this[this.mode](g, w, h, dpr, dt);
    g.restore();
  }

  bars(g, w, h, dpr) {
    const a = this.audio, ink = this.ink;
    const compact = h < 120 * dpr;
    // leave room for the level meters on the right; equal side margins
    const pad = compact ? 6 * dpr : 16 * dpr;
    const usable = w - pad - (compact ? 6 * dpr : 34 * dpr);
    const n = Math.max(16, Math.min(72, Math.floor(usable / ((compact ? 7 : 11) * dpr))));
    const v = a.sample(n, this.tmp);
    const gap = 2.5 * dpr, bw = (usable - gap * (n - 1)) / n;
    const floor = compact ? h - 6 * dpr : h * 0.78;
    const maxH = compact ? h - 12 * dpr : h * 0.62;
    const grad = g.createLinearGradient(0, floor - maxH, 0, floor);
    grad.addColorStop(0, `rgba(${ink},1)`); grad.addColorStop(1, `rgba(${ink},.25)`);
    for (let i = 0; i < n; i++) {
      const val = Math.pow(v[i], 1.25);
      const bh = Math.max(2 * dpr, val * maxH);
      const x = pad + i * (bw + gap);
      g.fillStyle = grad;
      roundRect(g, x, floor - bh, bw, bh, Math.min(bw / 2, 3 * dpr)); g.fill();
      if (!compact) {
        // glassy reflection
        g.fillStyle = `rgba(${ink},${0.07 + val * 0.08})`;
        roundRect(g, x, floor + 4 * dpr, bw, bh * 0.28, Math.min(bw / 2, 3 * dpr)); g.fill();
        // peak cap
        const pi = Math.min(a.n - 1, Math.round((i / (n - 1)) * (a.n - 1)));
        const py = floor - Math.pow(a.peaks[pi], 1.25) * maxH - 5 * dpr;
        g.fillStyle = `rgba(${ink},.9)`;
        g.fillRect(x, py, bw, 2 * dpr);
      }
    }
    if (!compact) { // floor line glow
      const lg = g.createLinearGradient(0, 0, w, 0);
      lg.addColorStop(0, `rgba(${ink},0)`); lg.addColorStop(0.5, `rgba(${ink},${0.25 + a.bass * 0.5})`); lg.addColorStop(1, `rgba(${ink},0)`);
      g.fillStyle = lg; g.fillRect(pad, floor + 1 * dpr, usable, 1 * dpr);
    }
  }

  radial(g, w, h, dpr, dt) {
    const a = this.audio, ink = this.ink;
    const cx = w / 2, cy = h * 0.54, m = Math.min(w, h);
    if (h < 120 * dpr) return this.bars(g, w, h, dpr); // too short: fall back
    this.rot += dt * (0.08 + a.energy * 0.5);
    const R = m * 0.17 * (1 + a.bass * 0.15 + a.beat * 0.05);
    const n = 96, v = a.sample(n / 2, this.tmp);
    g.globalCompositeOperation = 'lighter';
    // halo
    const halo = g.createRadialGradient(cx, cy, R * 0.6, cx, cy, R * 2.4);
    halo.addColorStop(0, `rgba(${ink},${0.12 + a.bass * 0.25})`); halo.addColorStop(1, `rgba(${ink},0)`);
    g.fillStyle = halo; g.beginPath(); g.arc(cx, cy, R * 2.4, 0, TAU); g.fill();
    // spokes (mirrored so it's symmetric)
    g.lineCap = 'round';
    for (let i = 0; i < n; i++) {
      const k = i < n / 2 ? i : n - 1 - i;
      const val = Math.pow(v[k], 1.4);
      const ang = this.rot + (i / n) * TAU - Math.PI / 2;
      const len = 4 * dpr + val * m * 0.2;
      const cs = Math.cos(ang), sn = Math.sin(ang);
      g.strokeStyle = `rgba(${ink},${0.35 + val * 0.65})`;
      g.lineWidth = Math.max(1.5 * dpr, (TAU * R / n) * 0.55);
      g.beginPath(); g.moveTo(cx + cs * (R + 6 * dpr), cy + sn * (R + 6 * dpr));
      g.lineTo(cx + cs * (R + 6 * dpr + len), cy + sn * (R + 6 * dpr + len)); g.stroke();
    }
    // waveform ring
    g.beginPath();
    for (let i = 0; i <= 128; i++) {
      const wv = a.wave[(i * 2) % 256] || 0;
      const ang = (i / 128) * TAU - this.rot * 0.5;
      const rr = R * 0.86 + wv * R * 0.35;
      const x = cx + Math.cos(ang) * rr, y = cy + Math.sin(ang) * rr;
      i ? g.lineTo(x, y) : g.moveTo(x, y);
    }
    g.closePath();
    g.strokeStyle = `rgba(${ink},.85)`; g.lineWidth = 1.6 * dpr; g.stroke();
    // core
    const core = g.createRadialGradient(cx, cy, 0, cx, cy, R * 0.8);
    core.addColorStop(0, `rgba(${ink},${0.1 + a.beat * 0.3})`); core.addColorStop(1, `rgba(${ink},0)`);
    g.fillStyle = core; g.beginPath(); g.arc(cx, cy, R * 0.8, 0, TAU); g.fill();
    this.drawParticles(g, cx, cy, R, m, dpr, dt);
  }

  wave(g, w, h, dpr) {
    const a = this.audio, ink = this.ink;
    const mid = h / 2, amp = h * (h < 120 * dpr ? 0.9 : 0.42);
    g.globalCompositeOperation = 'lighter';
    const layers = [[1, 2.2, 1], [0.6, 1.2, 0.45], [-0.45, 1, 0.3]];
    for (const [scale, lw, alpha] of layers) {
      g.beginPath();
      for (let i = 0; i < 256; i++) {
        const x = (i / 255) * w;
        const env = Math.sin((i / 255) * Math.PI);
        const y = mid + a.wave[i] * amp * scale * env * (0.8 + a.energy);
        i ? g.lineTo(x, y) : g.moveTo(x, y);
      }
      g.strokeStyle = `rgba(${ink},${alpha})`;
      g.lineWidth = lw * dpr;
      g.shadowColor = `rgba(${ink},.8)`; g.shadowBlur = scale === 1 ? 14 * dpr : 0;
      g.stroke();
    }
    g.shadowBlur = 0;
    // faint spectrum floor
    const n = 64, v = a.sample(n, this.tmp), bw = w / n;
    for (let i = 0; i < n; i++) {
      g.fillStyle = `rgba(${ink},${0.05 + v[i] * 0.12})`;
      const bh = v[i] * h * 0.25;
      g.fillRect(i * bw + 1, h - bh, bw - 2, bh);
    }
  }

  galaxy(g, w, h, dpr, dt) {
    const a = this.audio, ink = this.ink;
    const cx = w / 2, cy = h / 2, m = Math.max(w, h) * 0.55;
    g.globalCompositeOperation = 'lighter';
    const speed = 0.12 + a.energy * 1.4 + a.beat * 0.8;
    for (const s of this.stars) {
      const lvl = a.bands[s.band] || 0;
      s.a += dt * speed * (0.4 + (1 - s.r) * 1.6) * (s.z > 0.5 ? 1 : -0.6);
      const rr = s.r * m * (0.92 + lvl * 0.2) + a.beat * 6 * dpr;
      const x = cx + Math.cos(s.a) * rr, y = cy + Math.sin(s.a) * rr * 0.62;
      const sz = (s.s + lvl * 2.2) * dpr;
      g.fillStyle = `rgba(${ink},${0.25 + lvl * 0.75})`;
      g.fillRect(x, y, sz, sz);
    }
    const R = Math.min(w, h) * (0.08 + a.bass * 0.06);
    const core = g.createRadialGradient(cx, cy, 0, cx, cy, R * 3);
    core.addColorStop(0, `rgba(${ink},${0.6 + a.beat * 0.4})`); core.addColorStop(0.3, `rgba(${ink},.15)`); core.addColorStop(1, `rgba(${ink},0)`);
    g.fillStyle = core; g.beginPath(); g.arc(cx, cy, R * 3, 0, TAU); g.fill();
    this.drawParticles(g, cx, cy, R, Math.min(w, h), dpr, dt);
  }

  drawParticles(g, cx, cy, R, m, dpr, dt) {
    const ink = this.ink;
    for (let i = this.particles.length - 1; i >= 0; i--) {
      const p = this.particles[i];
      p.d += p.sp * dt * m * 0.6; p.life -= dt * 0.9;
      if (p.life <= 0) { this.particles.splice(i, 1); continue; }
      const x = cx + Math.cos(p.a) * (R + p.d), y = cy + Math.sin(p.a) * (R + p.d);
      g.fillStyle = `rgba(${ink},${p.life * 0.9})`;
      g.beginPath(); g.arc(x, y, p.size * dpr * (0.5 + p.life), 0, TAU); g.fill();
    }
  }
}

/** Thin reactive ring of bars that hugs the boost knob. */
export class KnobRing {
  constructor(canvas, audio) { this.c = canvas; this.g = canvas.getContext('2d'); this.audio = audio; this.ink = '255,255,255'; this.tmp = new Float32Array(64); this.rot = 0; }
  draw(dt, radiusCss, active) {
    const { w, h, dpr } = fitCanvas(this.c);
    const g = this.g, a = this.audio, ink = this.ink;
    g.clearRect(0, 0, w, h);
    const cx = w / 2, cy = h / 2, R = radiusCss * dpr + 5 * dpr;
    // never draw past the canvas edge: bars only use the room that exists
    const maxR = Math.min(w, h) / 2 - 2 * dpr;
    const room = maxR - R;
    if (room < 2 * dpr) return;
    const n = 72, v = a.sample(n / 2, this.tmp);
    this.rot += dt * 0.12;
    g.lineCap = 'round';
    for (let i = 0; i < n; i++) {
      const k = i < n / 2 ? i : n - 1 - i;
      const val = active ? Math.pow(v[k], 1.5) : 0;
      const ang = this.rot + (i / n) * TAU;
      const len = Math.min(room, 2 * dpr + val * room);
      const cs = Math.cos(ang), sn = Math.sin(ang);
      g.strokeStyle = `rgba(${ink},${0.12 + val * 0.75})`;
      g.lineWidth = 2 * dpr;
      g.beginPath(); g.moveTo(cx + cs * R, cy + sn * R); g.lineTo(cx + cs * (R + len), cy + sn * (R + len)); g.stroke();
    }
  }
}

function roundRect(g, x, y, w, h, r) {
  g.beginPath();
  g.moveTo(x + r, y); g.lineTo(x + w - r, y); g.quadraticCurveTo(x + w, y, x + w, y + r);
  g.lineTo(x + w, y + h); g.lineTo(x, y + h); g.lineTo(x, y + r); g.quadraticCurveTo(x, y, x + r, y);
  g.closePath();
}
function easeOut(t) { return 1 - Math.pow(1 - t, 3); }
