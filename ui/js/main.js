import { invoke, listen, isTauri } from './bridge.js';
import { AudioData } from './audio-data.js';
import { LiquidBackground } from './liquid.js';
import { Visualizer, KnobRing } from './visualizer.js';
import { Knob, PitchSlider, Equalizer } from './controls.js';
import { NowPlaying } from './nowplaying.js';

const $ = (id) => document.getElementById(id);
const app = $('app');

const PRESETS = [
  ['Flat', [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
  ['Bass Boost', [7, 6, 5, 3, 1, 0, 0, 0, 0, 0]],
  ['Bass Extreme', [10, 9, 7, 4, 1, 0, -1, -1, 0, 0]],
  ['Vocal', [-2, -1, 0, 2, 4, 4, 3, 1, 0, -1]],
  ['Rock', [5, 4, 2, -1, -2, -1, 2, 4, 5, 5]],
  ['Pop', [-1, 1, 3, 4, 3, 0, -1, -1, 1, 2]],
  ['Electronic', [6, 5, 1, 0, -2, 1, 0, 2, 5, 6]],
  ['Cinema', [4, 3, 1, 0, -1, 1, 2, 3, 3, 2]],
  ['Gaming', [2, 1, 0, -1, 0, 2, 4, 5, 4, 2]],
  ['Podcast', [-3, -2, 0, 2, 3, 4, 3, 1, -1, -3]],
  ['Treble', [0, 0, 0, 0, 0, 1, 3, 5, 7, 8]],
  ['Night', [3, 2, 1, 0, -1, -1, 0, 1, 2, 2]],
];

let S = null;              // settings mirror
let status = {};
let saveTimer = 0;
const save = () => { clearTimeout(saveTimer); saveTimer = setTimeout(() => invoke('save_settings').catch(() => {}), 900); };

// ── toast ────────────────────────────────────────────────────────────
let toastTimer = 0;
function toast(msg) {
  const t = $('toast'); t.textContent = msg; t.classList.add('show');
  clearTimeout(toastTimer); toastTimer = setTimeout(() => t.classList.remove('show'), 1800);
}

// ── audio + visuals ──────────────────────────────────────────────────
const audio = new AudioData(72);
const liquid = new LiquidBackground($('liquidBg'), audio);
const viz = new Visualizer($('vizCanvas'), audio);
const ring = new KnobRing($('ringCanvas'), audio);
audio.onBeat = (p) => viz.burst(p);

// ── boost ────────────────────────────────────────────────────────────
function showBoost(v) {
  const db = v <= 0 ? '−∞ dB' : `${v >= 1 ? '+' : ''}${(20 * Math.log10(v)).toFixed(1)} dB`;
  $('boostDb').textContent = db;
  document.querySelectorAll('[data-boost]').forEach((b) => b.classList.toggle('active', Math.abs(+b.dataset.boost - v) < 0.01));
}
const knob = new Knob($('knob'), {
  min: 0, max: 6, value: 1,
  onInput: (v) => { S.boost = v; invoke('set_boost', { value: v }); showBoost(v); },
  onCommit: save,
});
document.querySelectorAll('[data-boost]').forEach((b) => b.addEventListener('click', () => {
  const v = +b.dataset.boost; knob.set(v, true); save();
}));

function setBypass(on, fromUser = true) {
  S.bypass = on;
  app.classList.toggle('bypassed', on);
  $('btnPower').classList.toggle('on', !on);
  if (fromUser) { invoke('set_bypass', { enabled: on }); save(); toast(on ? 'Boost off — original sound' : 'Boost on'); }
  renderStatus();
}
$('btnPower').addEventListener('click', () => setBypass(!S.bypass));
$('btnLimiter').addEventListener('click', () => {
  S.limiter = !S.limiter;
  $('btnLimiter').classList.toggle('on', S.limiter);
  invoke('set_limiter', { enabled: S.limiter }); save();
  toast(S.limiter ? 'Limiter on — no distortion' : 'Limiter off — careful at high boost');
});

// ── pitch ────────────────────────────────────────────────────────────
function showPitch(bump) {
  const st = S.pitch_semitones, ct = S.pitch_cents, total = st + ct / 100;
  const stEl = $('pitchSt');
  stEl.textContent = (st > 0 ? '+' : '') + st;
  $('pitchCt').textContent = (ct > 0 ? '+' : '') + ct;
  $('pitchRatio').textContent = Math.pow(2, total / 12).toFixed(3);
  $('pitchDir').textContent = total === 0 ? 'Original' : `${total > 0 ? 'Up' : 'Down'} ${Math.abs(total).toFixed(2)} st`;
  if (document.activeElement !== $('inSt')) $('inSt').value = st;
  if (document.activeElement !== $('inCt')) $('inCt').value = ct;
  if (bump) { stEl.classList.remove('bump'); void stEl.offsetWidth; stEl.classList.add('bump'); }
}
function setPitch(st, ct, fromSlider = false) {
  st = Math.max(-12, Math.min(12, Math.round(+st || 0)));
  ct = Math.max(-100, Math.min(100, Math.round(+ct || 0)));
  const changed = st !== S.pitch_semitones || ct !== S.pitch_cents;
  S.pitch_semitones = st; S.pitch_cents = ct;
  if (!fromSlider) pitch.set(st, false);
  invoke('set_pitch', { semitones: st, cents: ct });
  showPitch(changed);
}
const pitch = new PitchSlider($('pitchSlider'), { onInput: (v) => setPitch(v, S.pitch_cents, true), onCommit: save });
$('inSt').addEventListener('change', (e) => { setPitch(e.target.value, S.pitch_cents); save(); });
$('inCt').addEventListener('change', (e) => { setPitch(S.pitch_semitones, e.target.value); save(); });
for (const id of ['inSt', 'inCt']) $(id).addEventListener('keydown', (e) => { if (e.key === 'Enter') e.target.blur(); });
document.querySelectorAll('[data-st]').forEach((b) => b.addEventListener('click', () => { setPitch(S.pitch_semitones + +b.dataset.st, S.pitch_cents); save(); }));
$('pitchReset').addEventListener('click', () => { setPitch(0, 0); save(); });

// ── equalizer + presets ──────────────────────────────────────────────
const eq = new Equalizer($('eqBands'), $('eqCurve'), {
  onBand: (i, g) => { S.eq[i] = g; S.preset = 'Custom'; invoke('set_eq_band', { index: i, gain: g }); renderPresets(); },
  onCommit: save,
});
function applyPreset(name, gains) {
  S.eq = [...gains]; S.preset = name;
  eq.setAll(gains);
  invoke('set_eq', { gains, preset: name });
  renderPresets(); save();
}
$('eqReset').addEventListener('click', () => applyPreset('Flat', PRESETS[0][1]));

function renderPresets() {
  const box = $('presets');
  box.innerHTML = '';
  const add = (name, gains, custom) => {
    const c = document.createElement('button');
    c.className = 'chip' + (S.preset === name ? ' active' : '') + (custom ? ' custom' : '');
    const label = document.createElement('span'); label.textContent = name; c.appendChild(label);
    c.title = name;
    c.addEventListener('click', () => { applyPreset(name, gains); toast(`Preset: ${name}`); });
    if (custom) {
      const x = document.createElement('span'); x.className = 'x'; x.textContent = '×'; x.title = 'Delete preset';
      x.addEventListener('click', (e) => {
        e.stopPropagation();
        S.custom_presets = S.custom_presets.filter((p) => p.name !== name);
        invoke('set_custom_presets', { presets: S.custom_presets }); renderPresets();
      });
      c.appendChild(x);
    }
    box.appendChild(c);
  };
  PRESETS.forEach(([n, g]) => add(n, g, false));
  (S.custom_presets || []).forEach((p) => add(p.name, p.eq, true));
  $('presetName').textContent = S.preset;
  requestAnimationFrame(() => box.classList.toggle('overflow', box.scrollHeight > box.clientHeight + 1));
}

$('btnSavePreset').addEventListener('click', () => {
  if ($('presetInput')) return $('presetInput').focus();
  const inp = document.createElement('input');
  inp.id = 'presetInput'; inp.className = 'chip'; inp.placeholder = 'Name, then Enter'; inp.maxLength = 24;
  inp.style.width = '150px';
  $('presets').prepend(inp); inp.focus();
  const done = (ok) => {
    const name = inp.value.trim();
    inp.remove();
    if (!ok || !name) return;
    S.custom_presets = (S.custom_presets || []).filter((p) => p.name !== name).concat([{ name, eq: [...S.eq] }]);
    S.preset = name;
    invoke('set_custom_presets', { presets: S.custom_presets }); invoke('set_eq', { gains: S.eq, preset: name });
    renderPresets(); toast(`Saved “${name}”`);
  };
  inp.addEventListener('keydown', (e) => { if (e.key === 'Enter') done(true); if (e.key === 'Escape') done(false); });
  inp.addEventListener('blur', () => done(true));
});

// ── visualizer tabs ──────────────────────────────────────────────────
function setViz(mode, user) {
  viz.setMode(mode); S.viz_mode = mode;
  document.querySelectorAll('[data-viz]').forEach((b) => b.classList.toggle('active', b.dataset.viz === mode));
  if (user) { invoke('set_ui', { viz: mode }); save(); }
}
document.querySelectorAll('[data-viz]').forEach((b) => b.addEventListener('click', () => setViz(b.dataset.viz, true)));
$('vizCanvas').addEventListener('dblclick', () => {
  const modes = ['bars', 'radial', 'wave', 'galaxy'];
  setViz(modes[(modes.indexOf(S.viz_mode) + 1) % modes.length], true);
});

// ── now playing ──────────────────────────────────────────────────────
const np = new NowPlaying(document.querySelector('.np-panel'), { onControl: (a) => invoke('media_control', { action: a }) });

// ── window / mode ────────────────────────────────────────────────────
async function setMode(mode, user = true) {
  if (user && mode === S.mode) return;
  if (user) {
    app.classList.add('morphing');
    await new Promise((r) => setTimeout(r, 280));
    try { await invoke('set_mode', { mode }); } catch (e) { console.warn(e); }
  }
  S.mode = mode;
  app.classList.toggle('compact', mode === 'compact');
  app.classList.toggle('extended', mode === 'extended');
  app.classList.remove('sheet');
  $('btnMode').title = mode === 'compact' ? 'Extended view' : 'Compact view';
  app.classList.toggle('morphed');
  requestAnimationFrame(() => { app.classList.remove('morphing'); eq.dirty = true; });
}
$('btnMode').addEventListener('click', () => setMode(S.mode === 'compact' ? 'extended' : 'compact'));
$('btnSettings').addEventListener('click', () => {
  if (S.mode === 'compact') app.classList.toggle('sheet');
  else document.querySelector('.settings-panel').animate([{ transform: 'scale(1)' }, { transform: 'scale(1.02)' }, { transform: 'scale(1)' }], { duration: 450, easing: 'cubic-bezier(.34,1.56,.64,1)' });
  $('btnSettings').classList.toggle('active', app.classList.contains('sheet'));
});
$('closeSettings').addEventListener('click', () => { app.classList.remove('sheet'); $('btnSettings').classList.remove('active'); });
$('btnMin').addEventListener('click', () => invoke('win_minimize'));
$('btnClose').addEventListener('click', () => { invoke('win_hide'); if (!isTauri) toast('Hidden to tray (keeps boosting)'); });

// ── settings panel ───────────────────────────────────────────────────
function fillDevices(d) {
  const fill = (sel, list, cur, filter) => {
    sel.innerHTML = '';
    const auto = document.createElement('option'); auto.value = ''; auto.textContent = 'Automatic'; sel.appendChild(auto);
    list.filter(filter).forEach((n) => { const o = document.createElement('option'); o.value = o.textContent = n; sel.appendChild(o); });
    sel.value = cur && list.includes(cur) ? cur : '';
  };
  fill($('selInput'), d.inputs, S.input_device, () => true);
  fill($('selOutput'), d.outputs, S.output_device, (n) => !/cable|virtual|voicemeeter/i.test(n));
}
$('btnApplyDev').addEventListener('click', async () => {
  const input = $('selInput').value || null, output = $('selOutput').value || null;
  S.input_device = input; S.output_device = output;
  setStatus(await invoke('set_devices', { input, output }));
  toast(status.running ? 'Audio engine restarted' : 'Could not start — check devices');
});
$('btnRefresh').addEventListener('click', async () => { fillDevices(await invoke('list_devices')); toast('Devices refreshed'); });
$('btnSoundSettings').addEventListener('click', () => invoke('open_link', { target: 'sound-settings' }));
document.querySelectorAll('[data-link]').forEach((b) => b.addEventListener('click', () => invoke('open_link', { target: b.dataset.link })));

function rangeFill(el) { const p = ((el.value - el.min) / (el.max - el.min)) * 100; el.style.setProperty('--p', `${p}%`); }
function showStereo() {
  $('widthVal').textContent = S.width === 0 ? 'Mono' : `${Math.round(S.width * 100)}%`;
  const b = S.balance;
  $('balVal').textContent = Math.abs(b) < 0.03 ? 'Centre' : `${Math.round(Math.abs(b) * 100)}% ${b < 0 ? 'L' : 'R'}`;
}
for (const id of ['rngWidth', 'rngBalance']) {
  $(id).addEventListener('input', () => {
    S.width = +$('rngWidth').value; S.balance = +$('rngBalance').value;
    if (Math.abs(S.balance) < 0.06) { S.balance = 0; $('rngBalance').value = 0; } // snap to centre
    rangeFill($(id)); showStereo();
    invoke('set_stereo', { width: S.width, balance: S.balance });
  });
  $(id).addEventListener('change', save);
  $(id).addEventListener('dblclick', () => { $(id).value = id === 'rngWidth' ? 1 : 0; $(id).dispatchEvent(new Event('input')); save(); });
}
$('rngIntensity').addEventListener('input', (e) => {
  S.intensity = +e.target.value; audio.intensity = S.intensity; rangeFill(e.target);
  $('intVal').textContent = `${Math.round(S.intensity * 100)}%`;
  invoke('set_ui', { intensity: S.intensity });
});
$('rngIntensity').addEventListener('change', save);

$('tglAutostart').addEventListener('change', async (e) => {
  try { e.target.checked = await invoke('set_autostart', { enabled: e.target.checked }); toast(e.target.checked ? 'Will start with Windows' : 'Removed from startup'); }
  catch (err) { toast('Could not change startup'); }
});
$('tglMinimized').addEventListener('change', (e) => { S.start_minimized = e.target.checked; invoke('set_start_minimized', { enabled: e.target.checked }); });

function applyTheme(theme) {
  const light = theme === 'light';
  app.classList.toggle('light', light);
  liquid.light = light;
  const ink = light ? '0,0,0' : '255,255,255';
  viz.ink = ring.ink = eq.ink = ink; viz.bg = light ? '217,217,217' : '4,4,4';
  eq.dirty = true;
}
$('tglLight').addEventListener('change', (e) => { S.theme = e.target.checked ? 'light' : 'dark'; applyTheme(S.theme); invoke('set_ui', { theme: S.theme }); save(); });

const refractSaved = (() => { try { return localStorage.getItem('refract'); } catch { return null; } })();
$('tglRefract').checked = refractSaved !== '0';
app.classList.toggle('refract', $('tglRefract').checked);
$('tglRefract').addEventListener('change', (e) => {
  app.classList.toggle('refract', e.target.checked);
  try { localStorage.setItem('refract', e.target.checked ? '1' : '0'); } catch {}
});

// ── status ───────────────────────────────────────────────────────────
function setStatus(st) {
  status = st || {};
  renderStatus();
  const info = $('engineInfo');
  if (status.running) {
    const rs = status.input_rate && status.input_rate !== status.sample_rate ? ` · resampling ${status.input_rate / 1000} → ${status.sample_rate / 1000} kHz` : '';
    info.innerHTML = `<b>From</b> ${status.input}<br><b>To</b> ${status.output}<br><b>Format</b> ${status.sample_rate / 1000} kHz · ~${status.latency_ms} ms latency${rs}`;
  } else {
    info.innerHTML = `<b>Engine stopped</b><br>${(status.error || '').replace('CABLE_NOT_FOUND: ', '') || 'Press Apply & restart.'}`;
  }
  if (!status.running && /CABLE_NOT_FOUND/.test(status.error || '') && !sessionStorage.getItem('setupLater')) $('setup').classList.add('show');
  else $('setup').classList.remove('show');
}
function renderStatus() {
  const pill = $('statusPill'), txt = pill.querySelector('span');
  pill.className = 'status-pill';
  if (!status.running) { pill.classList.add('warn'); txt.textContent = /CABLE/.test(status.error || '') ? 'Setup needed' : 'Stopped'; }
  else if (S && S.bypass) txt.textContent = 'Bypass';
  else { pill.classList.add('live'); txt.textContent = `Live · ${(status.sample_rate || 48000) / 1000} kHz`; }
}
$('setupRetry').addEventListener('click', async () => {
  fillDevices(await invoke('list_devices'));
  setStatus(await invoke('start_engine'));
  toast(status.running ? 'Connected!' : 'Virtual cable still not found');
});
$('setupLater').addEventListener('click', () => { sessionStorage.setItem('setupLater', '1'); $('setup').classList.remove('show'); });

// ── cursor-following specular light on the glass ─────────────────────
let mouse = null;
app.addEventListener('pointermove', (e) => { mouse = e; });
function updateSheen() {
  if (!mouse) return;
  for (const p of document.querySelectorAll('.panel')) {
    const r = p.getBoundingClientRect();
    p.style.setProperty('--mx', `${mouse.clientX - r.left}px`);
    p.style.setProperty('--my', `${mouse.clientY - r.top}px`);
  }
  mouse = null;
}

// ── render loop ──────────────────────────────────────────────────────
let last = performance.now(), running = true;
function loop(now) {
  if (!running) return;
  const dt = Math.min(0.05, (now - last) / 1000); last = now;
  audio.update(dt);
  app.style.setProperty('--energy', (S && S.bypass ? audio.energy * 0.4 : audio.energy).toFixed(3));
  liquid.draw(dt);
  viz.draw(dt);
  const kr = $('knob').getBoundingClientRect().width / 2;
  ring.draw(dt, kr, !(S && S.bypass));
  knob.tick(dt); pitch.tick(dt); eq.tick(dt); np.tick(dt);
  $('meterL').style.setProperty('--v', Math.min(1, audio.peakL).toFixed(3));
  $('meterR').style.setProperty('--v', Math.min(1, audio.peakR).toFixed(3));
  updateSheen();
  requestAnimationFrame(loop);
}
document.addEventListener('visibilitychange', () => {
  const vis = !document.hidden;
  invoke('set_viz_active', { enabled: vis }).catch(() => {});
  if (vis && !running) { running = true; last = performance.now(); requestAnimationFrame(loop); }
  if (!vis) running = false;
});

// ── boot ─────────────────────────────────────────────────────────────
async function boot() {
  const st = await invoke('get_state');
  S = st.settings;
  S.custom_presets ||= [];
  applyTheme(S.theme);
  $('tglLight').checked = S.theme === 'light';
  await setMode(S.mode, false);
  knob.set(S.boost); showBoost(S.boost);
  setBypass(S.bypass, false);
  $('btnLimiter').classList.toggle('on', S.limiter);
  pitch.set(S.pitch_semitones, false); showPitch(false);
  eq.setAll(S.eq);
  renderPresets();
  setViz(S.viz_mode || 'radial', false);
  $('rngWidth').value = S.width ?? 1; $('rngBalance').value = S.balance ?? 0;
  $('rngIntensity').value = S.intensity ?? 1; audio.intensity = S.intensity ?? 1;
  ['rngWidth', 'rngBalance', 'rngIntensity'].forEach((id) => rangeFill($(id)));
  $('intVal').textContent = `${Math.round(audio.intensity * 100)}%`;
  showStereo();
  $('tglAutostart').checked = st.autostart;
  $('tglMinimized').checked = S.start_minimized;
  fillDevices(st.devices);
  setStatus(st.status);

  listen('audio-frame', (f) => audio.feed(f));
  await listen('media-status', (m) => np.update(m));
  invoke('media_control', { action: 'refresh' }).catch(() => {});
  listen('engine-status', (s) => { setStatus(s); toast(s.running ? 'Audio device changed — reconnected' : 'Audio device lost'); });
  listen('bypass-changed', (on) => setBypass(on, false));
  listen('boost-changed', (v) => { S.boost = v; knob.set(v); showBoost(v); toast(`Boost ${Math.round(v * 100)}%`); });

  requestAnimationFrame(() => app.classList.remove('booting'));
  requestAnimationFrame(loop);
}
boot().catch((e) => { console.error(e); app.classList.remove('booting'); toast('Failed to load: ' + e); });
