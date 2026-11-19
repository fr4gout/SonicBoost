// Boost knob, pitch slider and 10-band EQ — all spring-animated.
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));

/** Critically damped spring: smooth, no overshoot jitter, frame-rate independent. */
export class Spring {
  constructor(v = 0, stiffness = 170, damping = 22) { this.v = v; this.target = v; this.vel = 0; this.k = stiffness; this.d = damping; }
  step(dt) {
    const steps = Math.ceil(dt / (1 / 240));
    const h = dt / steps;
    for (let i = 0; i < steps; i++) {
      const f = -this.k * (this.v - this.target) - this.d * this.vel;
      this.vel += f * h; this.v += this.vel * h;
    }
    if (Math.abs(this.v - this.target) < 1e-4 && Math.abs(this.vel) < 1e-4) { this.v = this.target; this.vel = 0; }
    return this.v;
  }
  get settled() { return this.v === this.target; }
}

// ─────────────────────────────────────────────────────────── knob ──
const START = 135, SWEEP = 270;
function polar(cx, cy, r, deg) { const a = (deg * Math.PI) / 180; return [cx + r * Math.cos(a), cy + r * Math.sin(a)]; }
function arcPath(r, a0, a1) {
  const [x0, y0] = polar(100, 100, r, a0), [x1, y1] = polar(100, 100, r, a1);
  return `M${x0} ${y0} A${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1} ${y1}`;
}

export class Knob {
  constructor(el, { min = 0, max = 6, value = 1, onInput, onCommit }) {
    Object.assign(this, { el, min, max, onInput, onCommit });
    this.value = value;
    this.spring = new Spring(value, 220, 26);
    let ticks = '';
    for (let i = 0; i <= 24; i++) {
      const a = START + (SWEEP * i) / 24, major = i % 4 === 0;
      const [x0, y0] = polar(100, 100, major ? 93 : 95, a), [x1, y1] = polar(100, 100, 99, a);
      ticks += `<line class="tick ${major ? 'major' : ''}" x1="${x0}" y1="${y0}" x2="${x1}" y2="${y1}"/>`;
    }
    el.innerHTML = `
      <svg viewBox="0 0 200 200">${ticks}
        <path class="track" d="${arcPath(86, START, START + SWEEP)}"/>
        <path class="arc" d=""/>
        <circle class="dot" r="5" cx="0" cy="0"/>
      </svg>
      <div class="orb"><div><div class="val"><span class="n">100</span><small>%</small></div><div class="sub">boost</div></div></div>`;
    el.tabIndex = 0;
    this.arc = el.querySelector('.arc'); this.dot = el.querySelector('.dot'); this.num = el.querySelector('.n');
    this.render(value);

    let startY = 0, startV = 0;
    el.addEventListener('pointerdown', (e) => {
      el.setPointerCapture(e.pointerId); el.classList.add('dragging');
      startY = e.clientY; startV = this.value;
    });
    el.addEventListener('pointermove', (e) => {
      if (!el.hasPointerCapture(e.pointerId)) return;
      const range = this.max - this.min;
      const fine = e.shiftKey ? 0.25 : 1;
      this.set(startV + ((startY - e.clientY) / 220) * range * fine, true);
    });
    const end = (e) => { if (el.hasPointerCapture?.(e.pointerId)) el.releasePointerCapture(e.pointerId); el.classList.remove('dragging'); this.onCommit?.(this.value); };
    el.addEventListener('pointerup', end); el.addEventListener('pointercancel', end);
    el.addEventListener('wheel', (e) => { e.preventDefault(); this.set(this.value + (e.deltaY < 0 ? 0.05 : -0.05), true); this.onCommit?.(this.value); }, { passive: false });
    el.addEventListener('dblclick', () => { this.set(1, true); this.onCommit?.(1); });
    el.addEventListener('keydown', (e) => {
      const d = { ArrowUp: 0.05, ArrowRight: 0.05, ArrowDown: -0.05, ArrowLeft: -0.05 }[e.key];
      if (d) { e.preventDefault(); this.set(this.value + d, true); this.onCommit?.(this.value); }
    });
  }

  set(v, fromUser = false) {
    v = clamp(Math.round(v * 100) / 100, this.min, this.max);
    this.value = v; this.spring.target = v;
    if (fromUser) this.onInput?.(v);
  }

  tick(dt) { if (!this.spring.settled) this.render(this.spring.step(dt)); }

  render(v) {
    const p = clamp((v - this.min) / (this.max - this.min), 0, 1);
    const a1 = START + SWEEP * Math.max(0.0005, p);
    this.arc.setAttribute('d', arcPath(86, START, a1));
    const [x, y] = polar(100, 100, 86, a1);
    this.dot.setAttribute('cx', x); this.dot.setAttribute('cy', y);
    this.num.textContent = Math.round(v * 100);
  }
}

// ────────────────────────────────────────────────────── pitch slider ──
export class PitchSlider {
  constructor(el, { onInput, onCommit }) {
    Object.assign(this, { el, onInput, onCommit });
    this.fill = el.querySelector('.ps-fill'); this.thumb = el.querySelector('.ps-thumb');
    const ticks = el.querySelector('.ps-ticks');
    for (let i = -12; i <= 12; i++) {
      const t = document.createElement('i');
      t.style.left = `${((i + 12) / 24) * 100}%`;
      if (i % 6 === 0) t.className = 'major';
      ticks.appendChild(t);
    }
    this.value = 0; this.spring = new Spring(0, 260, 28);
    const fromX = (x) => {
      const r = el.getBoundingClientRect();
      return Math.round(clamp((x - r.left) / r.width, 0, 1) * 24 - 12);
    };
    el.addEventListener('pointerdown', (e) => { el.setPointerCapture(e.pointerId); el.classList.add('dragging'); this.set(fromX(e.clientX), true); });
    el.addEventListener('pointermove', (e) => { if (el.hasPointerCapture(e.pointerId)) this.set(fromX(e.clientX), true); });
    const end = () => { el.classList.remove('dragging'); this.onCommit?.(this.value); };
    el.addEventListener('pointerup', end); el.addEventListener('pointercancel', end);
    el.addEventListener('wheel', (e) => { e.preventDefault(); this.set(this.value + (e.deltaY < 0 ? 1 : -1), true); this.onCommit?.(this.value); }, { passive: false });
    this.render(0);
  }
  set(v, fromUser) {
    v = clamp(v, -12, 12);
    if (v === this.value && fromUser) return;
    this.value = v; this.spring.target = v;
    if (fromUser) this.onInput?.(v);
  }
  tick(dt) { if (!this.spring.settled) this.render(this.spring.step(dt)); }
  render(v) {
    const p = (v + 12) / 24;
    this.thumb.style.left = `${p * 100}%`;
    const a = Math.min(p, 0.5), b = Math.max(p, 0.5);
    this.fill.style.left = `${a * 100}%`; this.fill.style.width = `${(b - a) * 100}%`;
  }
}

// ─────────────────────────────────────────────────────────── EQ ──
export const EQ_FREQS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
const EQ_LABELS = ['31', '62', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'];

// Same RBJ formulas as the Rust engine, so the drawn curve is the real one.
function coefs(fs, i, g) {
  if (Math.abs(g) < 0.01) return null;
  const A = Math.pow(10, g / 40);
  const f0 = i === 0 ? 31 * 1.6 : i === 9 ? 16000 * 0.7 : EQ_FREQS[i];
  const w0 = (2 * Math.PI * Math.min(f0, fs * 0.45)) / fs, c = Math.cos(w0), s = Math.sin(w0);
  let b0, b1, b2, a0, a1, a2;
  if (i === 0 || i === 9) {
    const al = (s / 2) * Math.SQRT2, sa = 2 * Math.sqrt(A) * al, sg = i === 0 ? 1 : -1;
    b0 = A * ((A + 1) - sg * (A - 1) * c + sa); b1 = sg * 2 * A * ((A - 1) - sg * (A + 1) * c);
    b2 = A * ((A + 1) - sg * (A - 1) * c - sa); a0 = (A + 1) + sg * (A - 1) * c + sa;
    a1 = -sg * 2 * ((A - 1) + sg * (A + 1) * c); a2 = (A + 1) + sg * (A - 1) * c - sa;
  } else {
    const al = s / (2 * 1.1);
    b0 = 1 + al * A; b1 = -2 * c; b2 = 1 - al * A; a0 = 1 + al / A; a1 = -2 * c; a2 = 1 - al / A;
  }
  return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
}
function respDb(cf, fs, f) {
  const [b0, b1, b2, a1, a2] = cf, w = (2 * Math.PI * f) / fs;
  const c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const nr = b0 + b1 * c1 + b2 * c2, ni = -(b1 * s1 + b2 * s2), dr = 1 + a1 * c1 + a2 * c2, di = -(a1 * s1 + a2 * s2);
  return 10 * Math.log10((nr * nr + ni * ni) / (dr * dr + di * di));
}

export class Equalizer {
  constructor(root, curveCanvas, { onBand, onCommit }) {
    Object.assign(this, { onBand, onCommit });
    this.canvas = curveCanvas; this.g = curveCanvas.getContext('2d');
    this.springs = EQ_FREQS.map(() => new Spring(0, 200, 24));
    this.values = Array(10).fill(0);
    this.ink = '255,255,255';
    this.bands = EQ_FREQS.map((_, i) => {
      const b = document.createElement('div');
      b.className = 'eq-band';
      b.innerHTML = `<div class="v">0</div><div class="t"><i class="f"></i><b class="k"></b></div><div class="hz">${EQ_LABELS[i]}</div>`;
      root.appendChild(b);
      const t = b.querySelector('.t');
      const fromY = (y) => { const r = t.getBoundingClientRect(); return Math.round(((1 - clamp((y - r.top) / r.height, 0, 1)) * 24 - 12) * 2) / 2; };
      t.addEventListener('pointerdown', (e) => { t.setPointerCapture(e.pointerId); b.classList.add('dragging'); this.setBand(i, fromY(e.clientY), true); });
      t.addEventListener('pointermove', (e) => { if (t.hasPointerCapture(e.pointerId)) this.setBand(i, fromY(e.clientY), true); });
      const end = () => { b.classList.remove('dragging'); this.onCommit?.(); };
      t.addEventListener('pointerup', end); t.addEventListener('pointercancel', end);
      t.addEventListener('wheel', (e) => { e.preventDefault(); this.setBand(i, this.values[i] + (e.deltaY < 0 ? 0.5 : -0.5), true); this.onCommit?.(); }, { passive: false });
      t.addEventListener('dblclick', () => { this.setBand(i, 0, true); this.onCommit?.(); });
      return { el: b, v: b.querySelector('.v'), f: b.querySelector('.f'), k: b.querySelector('.k') };
    });
    this.bands.forEach((_, i) => this.renderBand(i, 0));
    this.dirty = true;
  }

  setBand(i, g, fromUser) {
    g = clamp(g, -12, 12);
    if (fromUser && g === this.values[i]) return;
    this.values[i] = g; this.springs[i].target = g;
    const b = this.bands[i];
    b.v.textContent = (g > 0 ? '+' : '') + (Number.isInteger(g) ? g : g.toFixed(1));
    b.el.classList.toggle('changed', g !== 0);
    if (fromUser) this.onBand?.(i, g);
  }

  renderBand(i, v) {
    const p = (v + 12) / 24, b = this.bands[i];
    b.k.style.top = `${(1 - p) * 100}%`;
    const top = Math.min(p, 0.5), bot = Math.max(p, 0.5);
    b.f.style.top = `${(1 - bot) * 100}%`; b.f.style.height = `${(bot - top) * 100}%`;
  }

  setAll(gains) { gains.forEach((g, i) => this.setBand(i, g, false)); }

  tick(dt) {
    let moving = this.dirty;
    this.springs.forEach((s, i) => {
      if (s.settled) return;
      moving = true;
      this.renderBand(i, s.step(dt));
    });
    if (moving) { this.drawCurve(); this.dirty = false; }
  }

  drawCurve() {
    const c = this.canvas, dpr = Math.min(devicePixelRatio || 1, 2);
    const r = c.getBoundingClientRect();
    if (!r.width) return;
    c.width = r.width * dpr; c.height = r.height * dpr;
    const g = this.g, w = c.width, h = c.height, fs = 48000, ink = this.ink;
    g.clearRect(0, 0, w, h);
    const cf = this.springs.map((s, i) => coefs(fs, i, s.v)).filter(Boolean);
    // band centres sit at column centres; map x→freq through the 10 columns
    const pts = [];
    for (let x = 0; x <= w; x += 3 * dpr) {
      const col = clamp(x / w * 10 - 0.5, -0.5, 9.5);
      const i0 = Math.floor(clamp(col, 0, 8)), t = col - i0;
      const f = EQ_FREQS[i0] * Math.pow(EQ_FREQS[i0 + 1] / EQ_FREQS[i0], t);
      let db = 0; for (const k of cf) db += respDb(k, fs, f);
      pts.push([x, h / 2 - (clamp(db, -15, 15) / 12) * (h / 2) * 0.92]);
    }
    g.beginPath(); pts.forEach(([x, y], i) => (i ? g.lineTo(x, y) : g.moveTo(x, y)));
    g.strokeStyle = `rgba(${ink},.85)`; g.lineWidth = 2 * dpr; g.shadowColor = `rgba(${ink},.6)`; g.shadowBlur = 10 * dpr; g.stroke(); g.shadowBlur = 0;
    g.lineTo(w, h / 2); g.lineTo(0, h / 2); g.closePath();
    const grd = g.createLinearGradient(0, 0, 0, h);
    grd.addColorStop(0, `rgba(${ink},.16)`); grd.addColorStop(0.5, `rgba(${ink},.02)`); grd.addColorStop(1, `rgba(${ink},.16)`);
    g.fillStyle = grd; g.fill();
  }
}
