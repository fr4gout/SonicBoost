// Parametric EQ editor — Equalizer APO style.
// Preamp + up to 20 filters (PK, LSC, HSC, LP, HP, NO, BP), each with its own
// on/off, frequency, gain and Q. A live response curve with draggable nodes,
// and import/export in Equalizer APO's text format (works with AutoEQ files).
import { GlassSelect } from './select.js';

export const PEQ_MAX = 20;
export const KINDS = [
  ['PK', 'Peaking'], ['LSC', 'Low shelf'], ['HSC', 'High shelf'],
  ['LP', 'Low pass'], ['HP', 'High pass'], ['NO', 'Notch'], ['BP', 'Band pass'],
];
const usesGain = (k) => k === 'PK' || k === 'LSC' || k === 'HSC';
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const FS = 48000, FMIN = 20, FMAX = 20000;

export const SIGNATURE = () => ({
  enabled: true, preamp: -5,
  filters: [
    { enabled: true, kind: 'PK', freq: 80, gain: -5, q: 10 },
    { enabled: true, kind: 'PK', freq: 1800, gain: 3, q: 1 },
    { enabled: true, kind: 'PK', freq: 3200, gain: 3.5, q: 1.2 },
    { enabled: true, kind: 'PK', freq: 8000, gain: -2, q: 1 },
  ],
});

// ── filter maths (same RBJ designs as the Rust engine) ────────────────
export function design(kind, f0, g, q) {
  f0 = clamp(f0, 10, FS * 0.47); q = clamp(q, 0.05, 40);
  const w0 = (2 * Math.PI * f0) / FS, c = Math.cos(w0), s = Math.sin(w0), al = s / (2 * q), A = Math.pow(10, g / 40);
  let b0, b1, b2, a0, a1, a2;
  switch (kind) {
    case 'LSC': case 'HSC': {
      const sa = 2 * Math.sqrt(A) * al, sg = kind === 'LSC' ? 1 : -1;
      b0 = A * ((A + 1) - sg * (A - 1) * c + sa); b1 = sg * 2 * A * ((A - 1) - sg * (A + 1) * c);
      b2 = A * ((A + 1) - sg * (A - 1) * c - sa); a0 = (A + 1) + sg * (A - 1) * c + sa;
      a1 = -sg * 2 * ((A - 1) + sg * (A + 1) * c); a2 = (A + 1) + sg * (A - 1) * c - sa; break;
    }
    case 'LP': b0 = (1 - c) / 2; b1 = 1 - c; b2 = (1 - c) / 2; a0 = 1 + al; a1 = -2 * c; a2 = 1 - al; break;
    case 'HP': b0 = (1 + c) / 2; b1 = -(1 + c); b2 = (1 + c) / 2; a0 = 1 + al; a1 = -2 * c; a2 = 1 - al; break;
    case 'NO': b0 = 1; b1 = -2 * c; b2 = 1; a0 = 1 + al; a1 = -2 * c; a2 = 1 - al; break;
    case 'BP': b0 = al; b1 = 0; b2 = -al; a0 = 1 + al; a1 = -2 * c; a2 = 1 - al; break;
    default: b0 = 1 + al * A; b1 = -2 * c; b2 = 1 - al * A; a0 = 1 + al / A; a1 = -2 * c; a2 = 1 - al / A;
  }
  return [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0];
}
export function respDb([b0, b1, b2, a1, a2], f) {
  const w = (2 * Math.PI * f) / FS, c1 = Math.cos(w), s1 = Math.sin(w), c2 = Math.cos(2 * w), s2 = Math.sin(2 * w);
  const nr = b0 + b1 * c1 + b2 * c2, ni = -(b1 * s1 + b2 * s2), dr = 1 + a1 * c1 + a2 * c2, di = -(a1 * s1 + a2 * s2);
  return 10 * Math.log10(Math.max(1e-12, (nr * nr + ni * ni) / (dr * dr + di * di)));
}

// ── Equalizer APO text format ─────────────────────────────────────────
const num = (s) => parseFloat(String(s).replace(',', '.'));
export function parseApo(text) {
  let preamp = 0; const filters = []; let seen = 0;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.replace(/#.*$/, '').trim();
    if (!line) continue;
    let m = /^preamp\s*:\s*([-+]?[\d.,]+)\s*db/i.exec(line);
    if (m) { preamp += num(m[1]); seen++; continue; }
    m = /^filter\s*\d*\s*:\s*(on|off)\s+([a-z]+)(?:\s+\d+\s*db)?\s*(.*)$/i.exec(line);
    if (!m) continue;
    const on = m[1].toUpperCase() === 'ON';
    let code = m[2].toUpperCase();
    const rest = m[3];
    const fc = /fc\s+([\d.,]+)\s*(k?)hz/i.exec(rest);
    if (!fc) continue;
    const freq = num(fc[1]) * (fc[2] ? 1000 : 1);
    const gain = num((/gain\s+([-+]?[\d.,]+)\s*db/i.exec(rest) || [0, 0])[1]);
    let q = num((/\bq\s+([\d.,]+)/i.exec(rest) || [0, NaN])[1]);
    const bw = /bw\s+oct\s+([\d.,]+)/i.exec(rest);
    if (isNaN(q) && bw) { const n = Math.pow(2, num(bw[1])); q = Math.sqrt(n) / (n - 1); }
    const map = { PK: 'PK', PEQ: 'PK', MODAL: 'PK', LS: 'LSC', LSC: 'LSC', HS: 'HSC', HSC: 'HSC', LP: 'LP', LPQ: 'LP', HP: 'HP', HPQ: 'HP', NO: 'NO', BP: 'BP' };
    code = map[code];
    if (!code) continue; // all-pass and unknown types are skipped
    if (isNaN(q)) q = code === 'PK' || code === 'NO' || code === 'BP' ? 1 : 0.707;
    filters.push({ enabled: on, kind: code, freq: clamp(freq, 10, 24000), gain: clamp(gain, -24, 24), q: clamp(q, 0.05, 40) });
    seen++;
  }
  if (!seen) throw new Error('No Equalizer APO lines found. Expected lines like “Filter 1: ON PK Fc 80 Hz Gain -5 dB Q 10”.');
  return { enabled: true, preamp: clamp(preamp, -30, 30), filters: filters.slice(0, PEQ_MAX), skipped: Math.max(0, filters.length - PEQ_MAX) };
}
export function toApo(peq) {
  const f2 = (v, d) => (+v).toFixed(d).replace(/\.?0+$/, (m) => (m.startsWith('.') ? '' : m)) || '0';
  const lines = [`# SonicBoost parametric EQ`, `Preamp: ${(+peq.preamp).toFixed(1)} dB`];
  peq.filters.forEach((f, i) => {
    let s = `Filter ${i + 1}: ${f.enabled ? 'ON' : 'OFF'} ${f.kind} Fc ${f2(f.freq, 1)} Hz`;
    if (usesGain(f.kind)) s += ` Gain ${(+f.gain).toFixed(1)} dB`;
    s += ` Q ${(+f.q).toFixed(f.q < 1 ? 3 : 2)}`;
    lines.push(s);
  });
  return lines.join('\r\n');
}

// ── editor ────────────────────────────────────────────────────────────
const POWER = '<svg viewBox="0 0 24 24"><path d="M12 3v9"/><path d="M6.3 6.3a8 8 0 1 0 11.4 0"/></svg>';
const CLOSE = '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>';

export class ParametricEditor {
  constructor({ list, canvas, preRange, preInput, onChange }) {
    Object.assign(this, { listEl: list, canvas, preRange, preInput, onChange });
    this.g = canvas.getContext('2d');
    this.ink = '255,255,255';
    this.peq = SIGNATURE(); this.peq.enabled = false;
    this.selected = 0; this.selects = []; this.drag = null; this.hover = -1;
    this.bindCurve();
    preRange.addEventListener('input', () => this.setPreamp(+preRange.value));
    preInput.addEventListener('change', () => this.setPreamp(+preInput.value));
    preInput.addEventListener('keydown', (e) => { if (e.key === 'Enter') preInput.blur(); });
    new ResizeObserver(() => this.draw()).observe(canvas);
  }

  load(peq) {
    this.peq = JSON.parse(JSON.stringify(peq || SIGNATURE()));
    this.peq.filters ||= [];
    this.selected = Math.min(this.selected, Math.max(0, this.peq.filters.length - 1));
    this.renderRows(); this.syncPre(); this.draw();
  }

  emit() {
    if (this._raf) return;
    this._raf = requestAnimationFrame(() => { this._raf = 0; this.onChange?.(JSON.parse(JSON.stringify(this.peq))); });
  }

  setEnabled(on) { this.peq.enabled = on; this.draw(); this.emit(); }
  setPreamp(v) { this.peq.preamp = clamp(Math.round((+v || 0) * 10) / 10, -30, 30); this.syncPre(); this.draw(); this.emit(); }
  syncPre() {
    this.preRange.value = clamp(this.peq.preamp, -20, 10);
    const p = ((this.preRange.value - this.preRange.min) / (this.preRange.max - this.preRange.min)) * 100;
    this.preRange.style.setProperty('--p', `${p}%`);
    if (document.activeElement !== this.preInput) this.preInput.value = this.peq.preamp.toFixed(1);
  }

  add(freq = 1000, gain = 0) {
    if (this.peq.filters.length >= PEQ_MAX) return false;
    this.peq.filters.push({ enabled: true, kind: 'PK', freq: Math.round(freq), gain: Math.round(gain * 10) / 10, q: 1 });
    this.selected = this.peq.filters.length - 1;
    this.renderRows(); this.draw(); this.emit();
    requestAnimationFrame(() => this.listEl.scrollTo({ top: this.listEl.scrollHeight, behavior: 'smooth' }));
    return true;
  }
  remove(i) { this.peq.filters.splice(i, 1); this.selected = Math.max(0, Math.min(this.selected, this.peq.filters.length - 1)); this.renderRows(); this.draw(); this.emit(); }
  update(i, patch, rerenderRow = true) {
    Object.assign(this.peq.filters[i], patch);
    if (rerenderRow) this.syncRow(i);
    this.draw(); this.emit();
  }

  // rows ---------------------------------------------------------------
  renderRows() {
    this.selects.forEach((s) => s.destroy());
    this.selects = [];
    this.listEl.innerHTML = '';
    if (!this.peq.filters.length) {
      this.listEl.innerHTML = '<div class="peq-empty">No filters. Click <b>+ Filter</b>, or double-click the curve.</div>';
      return;
    }
    this.peq.filters.forEach((f, i) => {
      const row = document.createElement('div');
      row.className = 'peq-row';
      row.dataset.i = i;
      row.innerHTML = `
        <span class="pr-idx">${i + 1}</span>
        <button class="pr-on" title="Filter on / off">${POWER}</button>
        <div class="pr-type"><select>${KINDS.map(([c, n]) => `<option value="${c}">${n}</option>`).join('')}</select></div>
        <label class="nf pr-f"><input type="number" min="10" max="24000" step="1" /><em>Hz</em></label>
        <label class="nf pr-g"><input type="number" min="-24" max="24" step="0.1" /><em>dB</em></label>
        <label class="nf pr-q"><span>Q</span><input type="number" min="0.05" max="40" step="0.01" /></label>
        <button class="pr-del" title="Remove filter">${CLOSE}</button>`;
      const sel = row.querySelector('select');
      sel.value = f.kind;
      this.listEl.appendChild(row);
      this.selects.push(new GlassSelect(sel));
      sel.addEventListener('change', () => { this.update(i, { kind: sel.value, q: usesGain(sel.value) || sel.value === 'NO' || sel.value === 'BP' ? f.q : 0.707 }); });
      row.querySelector('.pr-on').addEventListener('click', () => this.update(i, { enabled: !this.peq.filters[i].enabled }));
      row.querySelector('.pr-del').addEventListener('click', () => this.remove(i));
      row.addEventListener('pointerdown', () => this.select(i));
      const fields = [['.pr-f input', 'freq', 10, 24000, (v, d) => v * Math.pow(1.03, d)],
                      ['.pr-g input', 'gain', -24, 24, (v, d) => v + d * 0.5],
                      ['.pr-q input', 'q', 0.05, 40, (v, d) => v * Math.pow(1.06, d)]];
      for (const [q, key, lo, hi, nudge] of fields) {
        const inp = row.querySelector(q);
        inp.addEventListener('change', () => this.update(i, { [key]: clamp(+inp.value || 0, lo, hi) }));
        inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') inp.blur(); });
        inp.addEventListener('wheel', (e) => {
          if (document.activeElement !== inp) return;
          e.preventDefault();
          const v = nudge(+this.peq.filters[i][key], e.deltaY < 0 ? 1 : -1);
          this.update(i, { [key]: clamp(key === 'freq' ? Math.round(v) : Math.round(v * 100) / 100, lo, hi) });
        }, { passive: false });
      }
      this.syncRow(i);
    });
  }
  syncRow(i) {
    const row = this.listEl.children[i], f = this.peq.filters[i];
    if (!row || !f) return;
    row.classList.toggle('off', !f.enabled);
    row.classList.toggle('sel', i === this.selected);
    const set = (q, v) => { const el = row.querySelector(q); if (document.activeElement !== el) el.value = v; };
    set('.pr-f input', Math.round(f.freq));
    set('.pr-g input', usesGain(f.kind) ? (+f.gain).toFixed(1) : '');
    set('.pr-q input', (+f.q).toFixed(2));
    row.querySelector('.pr-g input').disabled = !usesGain(f.kind);
    row.querySelector('.pr-g').classList.toggle('na', !usesGain(f.kind));
  }
  select(i) {
    this.selected = i;
    [...this.listEl.children].forEach((r, k) => r.classList?.toggle('sel', k === i));
    this.draw();
  }

  // curve --------------------------------------------------------------
  range() {
    let m = 12;
    for (const f of this.peq.filters) if (f.enabled && usesGain(f.kind)) m = Math.max(m, Math.abs(f.gain) + 3);
    m = Math.max(m, Math.abs(this.peq.preamp) + 3);
    return Math.min(30, Math.ceil(m / 6) * 6);
  }
  fx(f, w) { return (Math.log(f / FMIN) / Math.log(FMAX / FMIN)) * w; }
  xf(x, w) { return FMIN * Math.pow(FMAX / FMIN, clamp(x / w, 0, 1)); }
  totalDb(f, coefs) { let db = this.peq.preamp; for (const c of coefs) db += respDb(c, f); return db; }

  draw() {
    const c = this.canvas, dpr = Math.min(devicePixelRatio || 1, 2), r = c.getBoundingClientRect();
    if (!r.width) return;
    if (c.width !== Math.round(r.width * dpr) || c.height !== Math.round(r.height * dpr)) { c.width = Math.round(r.width * dpr); c.height = Math.round(r.height * dpr); }
    const g = this.g, w = c.width, h = c.height, ink = this.ink, R = this.range();
    const y = (db) => h / 2 - (db / R) * (h / 2 - 10 * dpr);
    g.clearRect(0, 0, w, h);
    g.font = `${10 * dpr}px Segoe UI, system-ui`; g.textBaseline = 'top';
    // grid
    g.lineWidth = 1;
    for (const f of [50, 100, 200, 500, 1000, 2000, 5000, 10000]) {
      const x = Math.round(this.fx(f, w)) + 0.5;
      g.strokeStyle = `rgba(${ink},.06)`; g.beginPath(); g.moveTo(x, 0); g.lineTo(x, h); g.stroke();
      g.fillStyle = `rgba(${ink},.32)`; g.fillText(f >= 1000 ? `${f / 1000}k` : `${f}`, x + 3 * dpr, h - 13 * dpr);
    }
    for (let db = -R; db <= R; db += R >= 18 ? 6 : 3) {
      const yy = Math.round(y(db)) + 0.5;
      g.strokeStyle = `rgba(${ink},${db === 0 ? 0.22 : 0.05})`; g.beginPath(); g.moveTo(0, yy); g.lineTo(w, yy); g.stroke();
      if (db !== 0 && db % 6 === 0) { g.fillStyle = `rgba(${ink},.3)`; g.fillText(`${db > 0 ? '+' : ''}${db}`, 4 * dpr, yy + 2 * dpr); }
    }
    const on = this.peq.enabled;
    const active = this.peq.filters.map((f) => (f.enabled ? design(f.kind, f.freq, f.gain, f.q) : null));
    const coefs = active.filter(Boolean);
    // selected filter's own contribution
    const sf = this.peq.filters[this.selected], sc = active[this.selected];
    if (sf && sc) {
      g.beginPath();
      for (let x = 0; x <= w; x += 3 * dpr) { const db = respDb(sc, this.xf(x, w)); x ? g.lineTo(x, y(db)) : g.moveTo(x, y(db)); }
      g.lineTo(w, y(0)); g.lineTo(0, y(0)); g.closePath();
      g.fillStyle = `rgba(${ink},.08)`; g.fill();
    }
    // combined curve
    g.beginPath();
    for (let x = 0; x <= w; x += 2 * dpr) { const yy = y(this.totalDb(this.xf(x, w), coefs)); x ? g.lineTo(x, yy) : g.moveTo(x, yy); }
    g.strokeStyle = `rgba(${ink},${on ? 0.95 : 0.35})`; g.lineWidth = 2 * dpr;
    g.shadowColor = `rgba(${ink},.45)`; g.shadowBlur = on ? 8 * dpr : 0; g.stroke(); g.shadowBlur = 0;
    // nodes
    this.nodes = [];
    this.peq.filters.forEach((f, i) => {
      const x = this.fx(clamp(f.freq, FMIN, FMAX), w);
      // points sit on the curve: their gain plus the preamp (pass/notch types: the curve value)
      const yy = usesGain(f.kind) ? y(f.gain + this.peq.preamp) : y(this.totalDb(f.freq, coefs));
      this.nodes.push({ x, y: yy, i });
      const sel = i === this.selected, hov = i === this.hover, rr = (sel ? 9 : 7.5) * dpr * (hov ? 1.12 : 1);
      g.beginPath(); g.arc(x, yy, rr, 0, Math.PI * 2);
      g.fillStyle = f.enabled ? (sel ? `rgba(${ink},1)` : `rgba(${ink},.85)`) : `rgba(${ink},.18)`;
      g.shadowColor = 'rgba(0,0,0,.35)'; g.shadowBlur = 6 * dpr; g.fill(); g.shadowBlur = 0;
      g.fillStyle = ink === '255,255,255' ? (f.enabled ? '#0a0a0a' : '#bbb') : (f.enabled ? '#f4f4f4' : '#333');
      g.font = `700 ${9 * dpr}px Segoe UI, system-ui`; g.textAlign = 'center'; g.textBaseline = 'middle';
      g.fillText(String(i + 1), x, yy + 0.5 * dpr);
      g.textAlign = 'left'; g.textBaseline = 'top'; g.font = `${10 * dpr}px Segoe UI, system-ui`;
    });
    if (!on) {
      g.fillStyle = `rgba(${ink},.45)`; g.font = `600 ${11 * dpr}px Segoe UI, system-ui`; g.textAlign = 'right';
      g.fillText('PARAMETRIC EQ OFF', w - 10 * dpr, 8 * dpr); g.textAlign = 'left';
    }
  }

  bindCurve() {
    const c = this.canvas;
    const pos = (e) => { const r = c.getBoundingClientRect(), d = Math.min(devicePixelRatio || 1, 2); return { x: (e.clientX - r.left) * d, y: (e.clientY - r.top) * d, w: r.width * d, h: r.height * d, d }; };
    const hit = (p) => (this.nodes || []).reduce((best, n) => { const dd = Math.hypot(n.x - p.x, n.y - p.y); return dd < 14 * p.d && (!best || dd < best.dd) ? { i: n.i, dd } : best; }, null);
    const dbAt = (p) => { const R = this.range(); return ((p.h / 2 - p.y) / (p.h / 2 - 10 * p.d)) * R; };
    c.addEventListener('pointerdown', (e) => {
      const p = pos(e), n = hit(p);
      if (!n) return;
      c.setPointerCapture(e.pointerId);
      this.select(n.i);
      this.drag = { i: n.i };
      c.classList.add('dragging');
    });
    c.addEventListener('pointermove', (e) => {
      const p = pos(e);
      if (!this.drag) {
        const n = hit(p), hv = n ? n.i : -1;
        if (hv !== this.hover) { this.hover = hv; c.style.cursor = hv >= 0 ? 'grab' : 'crosshair'; this.draw(); }
        return;
      }
      const f = this.peq.filters[this.drag.i];
      const patch = { freq: Math.round(this.xf(p.x, p.w)) };
      if (usesGain(f.kind)) patch.gain = clamp(Math.round((dbAt(p) - this.peq.preamp) * (e.shiftKey ? 20 : 2)) / (e.shiftKey ? 20 : 2), -24, 24);
      this.update(this.drag.i, patch);
    });
    const end = () => { this.drag = null; c.classList.remove('dragging'); };
    c.addEventListener('pointerup', end); c.addEventListener('pointercancel', end);
    c.addEventListener('wheel', (e) => {
      const n = hit(pos(e)); const i = n ? n.i : null;
      if (i === null) return;
      e.preventDefault();
      const f = this.peq.filters[i];
      this.select(i);
      this.update(i, { q: clamp(Math.round(f.q * Math.pow(1.08, e.deltaY < 0 ? 1 : -1) * 100) / 100, 0.05, 40) });
    }, { passive: false });
    c.addEventListener('dblclick', (e) => {
      const p = pos(e), n = hit(p);
      if (n) { const f = this.peq.filters[n.i]; if (usesGain(f.kind)) this.update(n.i, { gain: 0 }); return; }
      this.add(this.xf(p.x, p.w), clamp(dbAt(p) - this.peq.preamp, -24, 24));
    });
  }
}
