// Mic view: voice chain, voice modes, presets, devices, soundboard and meters.
// Everything is optional: each stage has its own switch and the whole mic
// system is off until the power button is pressed.
import { invoke, listen } from './bridge.js';
import { GlassSelect } from './select.js';

const $ = (id) => document.getElementById(id);
const fill = (el) => el.style.setProperty('--p', `${((el.value - el.min) / (el.max - el.min)) * 100}%`);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

export const MODES = {
  natural: ['Natural', 'your voice, cleaned'],
  broadcast: ['Broadcast', 'radio-host presence'],
  deep: ['Deep', 'lower and fuller'],
  high: ['High', 'brighter, lighter'],
  chipmunk: ['Chipmunk', '+9 semitones'],
  robot: ['Robot', 'metallic ring'],
  radio: ['Radio', 'AM band, crackle'],
  telephone: ['Telephone', 'narrow band'],
  megaphone: ['Megaphone', 'driven horn'],
  monster: ['Monster', '−8 st, grit'],
  alien: ['Alien', 'warbling pitch'],
  cave: ['Cave', 'big echo'],
  custom: ['Custom', 'your pitch + room'],
};
const VALUE_FMT = {
  boost_db: (v) => `+${(+v).toFixed(1)} dB`,
  gate_db: (v) => `${Math.round(v)} dB`,
  comp_amount: (v) => `${Math.round(v * 100)}%`,
  deess_amount: (v) => `${Math.round(v * 100)}%`,
  out_db: (v) => `${v > 0 ? '+' : ''}${(+v).toFixed(1)} dB`,
};
const SAME = (a, b) => ['ns', 'boost_db', 'gate_on', 'gate_db', 'eq_preset', 'comp_on', 'comp_amount', 'deess_on', 'deess_amount', 'limiter_on', 'out_db']
  .every((k) => (typeof a[k] === 'number' ? Math.abs(a[k] - b[k]) < 1e-3 : a[k] === b[k]));

/** Horizontal row (compact view): mouse wheel, drag and click-through. */
function hScroll(box) {
  const scrolls = () => box.scrollWidth > box.clientWidth + 2;
  box.addEventListener('wheel', (e) => {
    if (!scrolls()) return;
    const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (!d) return;
    e.preventDefault();
    box.scrollBy({ left: d * 1.2, behavior: 'smooth' });
  }, { passive: false });
  let down = null, moved = false;
  box.addEventListener('pointerdown', (e) => {
    if (!scrolls() || (e.pointerType === 'mouse' && e.button !== 0)) return;
    down = { x: e.clientX, left: box.scrollLeft, id: e.pointerId }; moved = false;
  });
  box.addEventListener('pointermove', (e) => {
    if (!down) return;
    const dx = e.clientX - down.x;
    if (!moved && Math.abs(dx) > 5) { moved = true; box.classList.add('dragging'); box.setPointerCapture(down.id); }
    if (moved) box.scrollLeft = down.left - dx;
  });
  const end = () => { if (down && moved) setTimeout(() => box.classList.remove('dragging'), 0); down = null; };
  box.addEventListener('pointerup', end); box.addEventListener('pointercancel', end);
  box.addEventListener('click', (e) => { if (moved) { e.stopPropagation(); e.preventDefault(); moved = false; } }, true);
}
/** Brings the active item of a scrolling row into view without moving the window. */
function reveal(box, sel) {
  const a = box.querySelector(sel);
  if (!a || box.scrollWidth <= box.clientWidth) return;
  const l = a.getBoundingClientRect().left - box.getBoundingClientRect().left + box.scrollLeft, r = l + a.offsetWidth;
  if (l < box.scrollLeft) box.scrollTo({ left: l - 12, behavior: 'smooth' });
  else if (r > box.scrollLeft + box.clientWidth) box.scrollTo({ left: r - box.clientWidth + 24, behavior: 'smooth' });
}

export class MicView {
  constructor({ app, toast }) {
    this.app = app;
    this.toast = toast;
    this.M = null; // MicSettings
    this.builtin = [];
    this.modes = Object.keys(MODES);
    this.status = { running: false };
    this.devices = { inputs: [], outputs: [] };
    this.selects = [];
    this.frame = null;
    this.wave = $('micWave');
    this.ctx = this.wave.getContext('2d');
    this.sendT = 0;
    this.bind();
    hScroll($('micPresets'));
    hScroll($('micModes'));
  }

  // ── data ───────────────────────────────────────────────────────────
  async load() {
    let st;
    try { st = await invoke('mic_get'); } catch (e) { console.warn(e); return; }
    if (!st) return;
    this.M = st.settings;
    this.builtin = st.builtin || [];
    this.modes = st.modes?.length ? st.modes : this.modes;
    this.status = st.status || this.status;
    this.devices = st.devices || this.devices;
    this.render();
  }

  /** Sends the whole mic config; debounced for sliders. */
  send(now = false) {
    clearTimeout(this.sendT);
    const go = async () => {
      try { const s = await invoke('mic_set', { mic: this.M }); if (s) { this.status = s; this.renderState(); } }
      catch (e) { this.toast(String(e)); }
    };
    if (now) go(); else this.sendT = setTimeout(go, 60);
  }

  // ── rendering ──────────────────────────────────────────────────────
  render() {
    if (!this.M) return;
    this.renderPresets();
    this.renderModes();
    this.renderChain();
    this.renderDevices();
    this.renderSoundboard();
    this.renderState();
  }

  allPresets() { return [...this.builtin.map((p) => ({ ...p, builtin: true })), ...(this.M.presets || [])]; }

  renderPresets() {
    const cur = this.M.cfg;
    const match = this.allPresets().find((p) => SAME(p.cfg, cur));
    const name = match ? match.name : 'Custom';
    this.M.preset = name;
    $('micPresetName').textContent = name;
    $('micPresets').innerHTML = this.allPresets().map((p, i) =>
      `<button class="chip${p.name === name ? ' active' : ''}" data-i="${i}" title="${esc(p.name)}">${esc(p.name)}${p.builtin ? '' : '<span class="x" data-del="1">×</span>'}</button>`).join('');
    requestAnimationFrame(() => reveal($('micPresets'), '.chip.active'));
  }

  renderModes() {
    const cur = this.M.cfg.mode;
    $('micModes').innerHTML = this.modes.map((m, i) => {
      const [t, d] = MODES[m] || [m, ''];
      return `<button class="mode${m === cur ? ' active' : ''}" data-m="${m}" title="${t}${i < 9 ? ` (Ctrl+Shift+${i + 1})` : ''}"><b>${t}</b><small>${d}</small>${i < 9 ? `<span class="key">⇧${i + 1}</span>` : ''}</button>`;
    }).join('');
    requestAnimationFrame(() => reveal($('micModes'), '.mode.active'));
    $('customVoice').classList.toggle('show', cur === 'custom');
    $('cvPitch').value = this.M.cfg.custom_pitch; $('cvRoom').value = this.M.cfg.custom_room;
    this.showCustom();
  }
  showCustom() {
    const p = +this.M.cfg.custom_pitch;
    $('cvPitchVal').textContent = `${p > 0 ? '+' : ''}${p.toFixed(1)} st`;
    $('cvRoomVal').textContent = `${Math.round(this.M.cfg.custom_room * 100)}%`;
    fill($('cvPitch')); fill($('cvRoom'));
  }

  renderChain() {
    const c = this.M.cfg;
    document.querySelectorAll('#micChain [data-k]').forEach((el) => {
      const k = el.dataset.k;
      if (el.type === 'checkbox') el.checked = !!c[k];
      else { el.value = c[k]; fill(el); }
    });
    this.showValues();
    document.querySelectorAll('#micEq button').forEach((b) => b.classList.toggle('active', +b.dataset.eq === c.eq_preset));
  }
  showValues() {
    const c = this.M.cfg;
    document.querySelectorAll('#micChain [data-v]').forEach((el) => { el.textContent = VALUE_FMT[el.dataset.v](c[el.dataset.v]); });
    document.querySelectorAll('#micChain .stage').forEach((st) => {
      const sw = st.querySelector('.switch input');
      st.classList.toggle('off', !!sw && !sw.checked && sw.dataset.k !== 'ns');
    });
  }

  renderDevices() {
    const d = this.devices;
    const opts = (sel, list, none) => {
      sel.innerHTML = `<option value="">${none}</option>` + list.map((n) => `<option>${esc(n)}</option>`).join('');
    };
    opts($('micIn'), d.inputs || [], `Windows default${d.default_input ? ` (${d.default_input})` : ''}`);
    opts($('micOut'), d.outputs || [], d.suggested_output ? `Auto (${d.suggested_output})` : 'Choose a virtual cable…');
    opts($('micMonDev'), d.outputs || [], 'Windows default output');
    $('micIn').value = this.M.input || '';
    $('micOut').value = this.M.output || '';
    $('micMonDev').value = this.M.monitor_device || '';
    if (!this.selects.length) this.selects = ['micIn', 'micOut', 'micMonDev'].map((id) => new GlassSelect($(id)));
    else this.selects.forEach((s) => s.refresh());
    $('micMon').checked = !!this.M.monitor_on;
    $('micMonVol').value = this.M.monitor_volume; fill($('micMonVol'));
    $('micMonVolVal').textContent = `${Math.round(this.M.monitor_volume * 100)}%`;
    document.querySelector('.mic-io-panel').classList.toggle('mon', !!this.M.monitor_on);
    document.querySelectorAll('#micPtt button').forEach((b) => b.classList.toggle('active', b.dataset.ptt === this.M.ptt));
    const out = this.M.output || d.suggested_output;
    $('micTip').innerHTML = out
      ? `In Discord, OBS or Zoom pick <b>${esc(out.replace('Input', 'Output').replace('CABLE In ', 'CABLE Out '))}</b> as the microphone.`
      : 'Install a second virtual cable (VB-Audio <b>Hi-Fi Cable</b> or <b>CABLE A/B</b>) to send your processed voice to other apps. The first cable stays with the booster.';
  }

  renderSoundboard() {
    const list = this.M.soundboard || [];
    $('sbGrid').innerHTML = list.length
      ? list.map((c, i) => `<div class="sb-clip" role="button" tabindex="0" data-i="${i}"><span class="nm">${esc(c.name)}</span>${i < 8 ? `<kbd>⇧F${i + 1}</kbd>` : ''}<button class="del" data-del="${i}" title="Remove">×</button></div>`).join('')
      : '<div class="sb-empty">Add short .wav clips: they play into your mic for everyone to hear.</div>';
    $('sbGain').value = this.M.sb_gain; fill($('sbGain'));
    $('sbGainVal').textContent = `${Math.round(this.M.sb_gain * 100)}%`;
  }

  renderState() {
    const on = !!this.M?.enabled, s = this.status || {};
    $('micPower').classList.toggle('on', on);
    $('micMute').classList.toggle('muted', !!this.M?.muted);
    this.app.classList.toggle('mic-live', on && s.running);
    let title = 'Mic off', text = 'Press power to process your voice';
    if (on && s.error) { title = 'Mic problem'; text = s.error; }
    else if (on && s.running) {
      title = this.M.muted ? 'Muted' : (MODES[this.M.cfg.mode]?.[0] || 'Live');
      text = `${s.input || 'Microphone'} → ${s.output || 'cable'}${s.latency_ms ? ` · ${s.latency_ms} ms` : ''}`;
    } else if (on) { title = 'Starting…'; text = 'Opening your microphone'; }
    $('micStateTitle').textContent = title;
    $('micStateText').textContent = text;
    $('micStateText').title = text;
    $('micInfo').textContent = on && s.running ? `48 kHz mono · ${s.latency_ms || 35} ms · monitor ${s.monitor ? 'on' : 'off'}` : '';
  }

  // ── events ─────────────────────────────────────────────────────────
  bind() {
    $('micPower').addEventListener('click', () => {
      if (!this.M) return;
      this.M.enabled = !this.M.enabled; this.renderState(); this.send(true);
      this.toast(this.M.enabled ? 'Mic processing on' : 'Mic processing off');
    });
    $('micMute').addEventListener('click', async () => {
      try { const m = await invoke('mic_mute', {}); if (this.M) this.M.muted = !!m; this.renderState(); this.toast(m ? 'Mic muted' : 'Mic live'); } catch (e) { this.toast(String(e)); }
    });

    // presets
    $('micPresets').addEventListener('click', (e) => {
      const b = e.target.closest('.chip'); if (!b) return;
      const p = this.allPresets()[+b.dataset.i]; if (!p) return;
      if (e.target.dataset.del) {
        this.M.presets = this.M.presets.filter((x) => x.name !== p.name);
        this.renderPresets(); this.send(true); this.toast(`Deleted “${p.name}”`); return;
      }
      const keep = { mode: this.M.cfg.mode, custom_pitch: this.M.cfg.custom_pitch, custom_room: this.M.cfg.custom_room };
      this.M.cfg = { ...structuredClone(p.cfg), ...keep };
      this.renderChain(); this.renderPresets(); this.send(true);
      this.toast(`Mic preset: ${p.name}`);
    });
    $('micPresetSave').addEventListener('click', () => {
      if (!this.M) return;
      const inp = $('micPresetInput');
      if (inp.classList.contains('show')) { this.savePreset(inp.value); return; }
      inp.value = ''; inp.classList.add('show'); $('micPresetSave').textContent = 'Save'; inp.focus();
    });
    $('micPresetInput').addEventListener('keydown', (e) => {
      if (e.key === 'Enter') this.savePreset(e.target.value);
      if (e.key === 'Escape') this.closeSave();
    });
    $('micPresetInput').addEventListener('blur', () => setTimeout(() => { if (document.activeElement !== $('micPresetSave')) this.closeSave(); }, 150));

    // voice modes
    $('micModes').addEventListener('click', (e) => {
      const b = e.target.closest('.mode'); if (!b || !this.M) return;
      this.M.cfg.mode = b.dataset.m; this.renderModes(); this.renderState(); this.send(true);
    });
    for (const id of ['cvPitch', 'cvRoom']) {
      $(id).addEventListener('input', (e) => {
        this.M.cfg[id === 'cvPitch' ? 'custom_pitch' : 'custom_room'] = +e.target.value;
        this.showCustom(); this.send();
      });
    }

    // chain
    $('micChain').addEventListener('input', (e) => {
      const k = e.target.dataset.k; if (!k || !this.M) return;
      this.M.cfg[k] = e.target.type === 'checkbox' ? e.target.checked : +e.target.value;
      if (e.target.type === 'range') fill(e.target);
      this.showValues(); this.renderPresets(); this.send(e.target.type === 'checkbox');
    });
    $('micEq').addEventListener('click', (e) => {
      const b = e.target.closest('button'); if (!b || !this.M) return;
      this.M.cfg.eq_preset = +b.dataset.eq; this.renderChain(); this.renderPresets(); this.send(true);
    });

    // devices + control
    $('micIn').addEventListener('change', (e) => { this.M.input = e.target.value || null; this.send(true); });
    $('micOut').addEventListener('change', (e) => { this.M.output = e.target.value || null; this.renderDevices(); this.send(true); });
    $('micMonDev').addEventListener('change', (e) => { this.M.monitor_device = e.target.value || null; this.send(true); });
    $('micMon').addEventListener('change', (e) => {
      this.M.monitor_on = e.target.checked;
      document.querySelector('.mic-io-panel').classList.toggle('mon', e.target.checked);
      this.send(true);
      if (e.target.checked) this.toast('Use headphones to avoid feedback');
    });
    $('micMonVol').addEventListener('input', (e) => {
      this.M.monitor_volume = +e.target.value; fill(e.target);
      $('micMonVolVal').textContent = `${Math.round(this.M.monitor_volume * 100)}%`; this.send();
    });
    $('micPtt').addEventListener('click', (e) => {
      const b = e.target.closest('button'); if (!b || !this.M) return;
      this.M.ptt = b.dataset.ptt; this.renderDevices(); this.send(true);
      this.toast({ off: 'Push-to-talk off', ptt: 'Hold Ctrl+Alt+T to talk', ptm: 'Hold Ctrl+Alt+T to mute' }[this.M.ptt]);
    });
    $('micRec').addEventListener('click', async () => {
      try { await invoke('mic_test', { action: 'record' }); this.toast('Recording 5 seconds… say something'); }
      catch (e) { this.toast(String(e)); }
    });
    $('micPlay').addEventListener('click', async () => {
      try { await invoke('mic_test', { action: 'play' }); this.toast('Playing your test back'); }
      catch (e) { this.toast(String(e)); }
    });

    // soundboard
    $('sbAdd').addEventListener('click', () => $('sbFile').click());
    $('sbFile').addEventListener('change', async (e) => {
      for (const f of e.target.files) {
        if (f.size > 20 * 1024 * 1024) { this.toast(`${f.name}: keep clips under 20 MB`); continue; }
        try {
          const bytes = Array.from(new Uint8Array(await f.arrayBuffer()));
          const list = await invoke('sb_add', { name: f.name.replace(/\.[^.]+$/, ''), bytes });
          if (list) this.M.soundboard = list;
        } catch (err) { this.toast(`${f.name}: ${err}`); }
      }
      e.target.value = '';
      this.renderSoundboard();
    });
    $('sbGrid').addEventListener('click', async (e) => {
      const del = e.target.closest('[data-del]');
      if (del) {
        try { const list = await invoke('sb_remove', { index: +del.dataset.del }); if (list) this.M.soundboard = list; this.renderSoundboard(); } catch (err) { this.toast(String(err)); }
        return;
      }
      const c = e.target.closest('.sb-clip'); if (!c) return;
      this.play(+c.dataset.i);
    });
    $('sbGrid').addEventListener('keydown', (e) => { const c = e.target.closest('.sb-clip'); if (c && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); this.play(+c.dataset.i); } });
    $('sbStop').addEventListener('click', () => invoke('sb_stop').catch(() => {}));
    $('sbGain').addEventListener('input', (e) => {
      this.M.sb_gain = +e.target.value; fill(e.target);
      $('sbGainVal').textContent = `${Math.round(this.M.sb_gain * 100)}%`; this.send();
    });
  }

  closeSave() { $('micPresetInput').classList.remove('show'); $('micPresetSave').textContent = '+ Save'; }
  savePreset(raw) {
    const name = (raw || '').trim().slice(0, 32);
    this.closeSave();
    if (!name) return;
    if (this.builtin.some((p) => p.name.toLowerCase() === name.toLowerCase())) { this.toast('That name is a built-in preset'); return; }
    const list = (this.M.presets || []).filter((p) => p.name.toLowerCase() !== name.toLowerCase());
    list.push({ name, cfg: structuredClone(this.M.cfg) });
    this.M.presets = list; this.renderPresets(); this.send(true); this.toast(`Saved “${name}”`);
  }

  async play(i) {
    const el = $('sbGrid').querySelector(`.sb-clip[data-i="${i}"]`);
    try {
      await invoke('sb_play', { index: i });
      if (el) { el.classList.add('flash'); setTimeout(() => el.classList.remove('flash'), 220); }
      if (!this.M.enabled) this.toast('Turn the mic on so others hear it');
    } catch (err) { this.toast(String(err)); }
  }

  /** Backend events. */
  async listen() {
    await listen('mic-frame', (f) => { this.frame = f; });
    listen('mic-changed', async (what) => {
      await this.load();
      if (what === 'muted' || what === 'unmuted') this.toast(what === 'muted' ? 'Mic muted' : 'Mic live');
      else if (typeof what === 'string' && MODES[what]) this.toast(`Voice: ${MODES[what][0]}`);
    });
    listen('mic-ptt', (down) => { $('lPtt').classList.toggle('on', !!down); });
    listen('mic-status', (s) => { this.status = s; this.renderState(); if (s?.error) this.toast(`Mic: ${s.error}`); });
  }

  /** Tells the backend whether to stream meters. */
  setVisible(on) {
    invoke('mic_visible', { enabled: on }).catch(() => {});
    if (on) this.load();
  }

  // ── meters (called from the main animation loop) ────────────────────
  tick() {
    const f = this.frame;
    if (!f) return;
    const db = (v) => Math.max(0, Math.min(1, (20 * Math.log10(Math.max(v, 1e-5)) + 60) / 60));
    if (f === this.lastFrame) return; // nothing new from the engine since the last draw
    this.lastFrame = f;
    $('mmIn').style.setProperty('--v', db(f.input).toFixed(3));
    $('mmOut').style.setProperty('--v', db(f.output).toFixed(3));
    $('mmGr').style.setProperty('--v', Math.min(1, (f.gr_db || 0) / 18).toFixed(3));
    $('lGate').classList.toggle('on', !!f.gate);
    $('lVoice').classList.toggle('on', (f.voice || 0) > 0.5);
    $('micRec').classList.toggle('active', !!f.recording);
    // input level against the gate threshold slider (−80…−10 dB)
    const inDb = 20 * Math.log10(Math.max(f.input, 1e-5));
    $('gateLevel').style.left = `calc(${Math.max(0, Math.min(1, (inDb + 80) / 70)) * 100}% - 1px)`;
    this.drawWave(f.wave || []);
  }

  drawWave(w) {
    const c = this.wave, r = devicePixelRatio || 1;
    const W = c.clientWidth, H = c.clientHeight;
    if (!W || !H) return;
    if (c.width !== Math.round(W * r) || c.height !== Math.round(H * r)) { c.width = Math.round(W * r); c.height = Math.round(H * r); }
    const g = this.ctx;
    g.setTransform(r, 0, 0, r, 0, 0);
    g.clearRect(0, 0, W, H);
    if (!w.length) return;
    // reading a CSS variable forces a style pass: refresh the colour twice a second, not every frame
    const t = performance.now();
    if (!this.ink || t - this.inkT > 500) { this.ink = getComputedStyle(this.app).getPropertyValue('--ink').trim() || '#fff'; this.inkT = t; }
    const ink = this.ink;
    const mid = H * 0.5, amp = H * 0.44;
    g.lineWidth = 1.5; g.strokeStyle = ink; g.globalAlpha = 0.9;
    g.beginPath();
    for (let i = 0; i < w.length; i++) {
      const x = (i / (w.length - 1)) * W, y = mid - Math.max(-1, Math.min(1, w[i] * 1.6)) * amp;
      i ? g.lineTo(x, y) : g.moveTo(x, y);
    }
    g.stroke();
    g.globalAlpha = 0.15;
    g.lineTo(W, mid); g.lineTo(0, mid); g.closePath(); g.fillStyle = ink; g.fill();
    g.globalAlpha = 1;
  }
}
