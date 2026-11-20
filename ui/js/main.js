import { invoke, listen, isTauri } from './bridge.js';
import { AudioData } from './audio-data.js';
import { LiquidBackground } from './liquid.js';
import { Visualizer, KnobRing, setRenderQuality } from './visualizer.js';
import { Knob, PitchSlider, Equalizer } from './controls.js';
import { NowPlaying } from './nowplaying.js';
import { GlassSelect } from './select.js';
import { MicView } from './mic.js';
import { ParametricEditor, parseApo, toApo, SIGNATURE, autoPreamp, profileNameFromFile } from './peq.js';

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

// ── notice bar (one at a time; higher priority wins) ──────────────────
let noticeKey = null, noticePrio = 0, noticeAction = null;
function showNotice({ key, prio = 1, title, text, action, onAction, warn = false }) {
  if (noticeKey && noticeKey !== key && prio < noticePrio) return;
  noticeKey = key; noticePrio = prio; noticeAction = onAction || null;
  const n = $('notice');
  $('noticeTitle').textContent = title; $('noticeText').textContent = text || '';
  $('noticeAction').textContent = action || ''; $('noticeAction').style.display = action ? '' : 'none';
  n.classList.toggle('warn', warn); n.classList.remove('busy'); n.classList.add('show');
}
function hideNotice(key) {
  if (key && key !== noticeKey) return;
  $('notice').classList.remove('show', 'busy'); noticeKey = null; noticePrio = 0; noticeAction = null;
}
const dismissed = new Set();
$('noticeAction').addEventListener('click', () => noticeAction && noticeAction());
$('noticeClose').addEventListener('click', () => { if (noticeKey) dismissed.add(noticeKey); hideNotice(); });

// ── audio + visuals ──────────────────────────────────────────────────
const audio = new AudioData(72);
const liquid = new LiquidBackground($('liquidBg'), audio);
const viz = new Visualizer($('vizCanvas'), audio);
const ring = new KnobRing($('ringCanvas'), audio);
audio.onBeat = (p) => viz.burst(p);

// ── boost ────────────────────────────────────────────────────────────
function showBoost(v) {
  typeof showWinVol === 'function' && requestAnimationFrame(showWinVol);
  const db = v <= 0 ? '−∞ dB' : `${v >= 1 ? '+' : ''}${(20 * Math.log10(v)).toFixed(1)} dB`;
  $('boostDb').textContent = db;
  document.querySelectorAll('[data-boost]').forEach((b) => b.classList.toggle('active', Math.abs(+b.dataset.boost - v) < 0.01));
}
const knob = new Knob($('knob'), {
  min: 0, max: 6, value: 1,
  onInput: (v) => { S.boost = v; invoke('set_boost', { value: v }); showBoost(v); markCustom(); },
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
  if (changed) markCustom();
}
const pitch = new PitchSlider($('pitchSlider'), { onInput: (v) => setPitch(v, S.pitch_cents, true), onCommit: save });
$('inSt').addEventListener('change', (e) => { setPitch(e.target.value, S.pitch_cents); save(); });
$('inCt').addEventListener('change', (e) => { setPitch(S.pitch_semitones, e.target.value); save(); });
for (const id of ['inSt', 'inCt']) $(id).addEventListener('keydown', (e) => { if (e.key === 'Enter') e.target.blur(); });
document.querySelectorAll('[data-st]').forEach((b) => b.addEventListener('click', () => { setPitch(S.pitch_semitones + +b.dataset.st, S.pitch_cents); save(); }));
$('pitchReset').addEventListener('click', () => { setPitch(0, 0); save(); });

// ── equalizer + presets ──────────────────────────────────────────────
const eq = new Equalizer($('eqBands'), $('eqCurve'), {
  onBand: (i, g) => { S.eq[i] = g; S.preset = 'Custom'; invoke('set_eq_band', { index: i, gain: g }); renderPresets(); markCustom(); },
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
  requestAnimationFrame(() => {
    box.classList.toggle('overflow', box.scrollHeight > box.clientHeight + 1);
    window.__revealActivePreset && window.__revealActivePreset();
  });
}

// Compact view: the preset row scrolls with the mouse wheel, by dragging, and with arrow buttons.
(function presetScroller() {
  const box = $('presets'), wrap = box.parentElement;
  const update = () => {
    const max = box.scrollWidth - box.clientWidth;
    wrap.classList.toggle('can-left', box.scrollLeft > 2);
    wrap.classList.toggle('can-right', box.scrollLeft < max - 2);
  };
  box.addEventListener('scroll', update, { passive: true });
  new ResizeObserver(update).observe(box);
  new MutationObserver(() => requestAnimationFrame(update)).observe(box, { childList: true });
  box.addEventListener('wheel', (e) => {
    if (S && S.mode !== 'compact') return;
    const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (!d) return;
    e.preventDefault();
    box.scrollBy({ left: d * 1.2, behavior: 'smooth' });
  }, { passive: false });
  let down = null, moved = false;
  box.addEventListener('pointerdown', (e) => {
    if (S && S.mode !== 'compact') return;
    if (e.pointerType === 'mouse' && e.button !== 0) return;
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
  // swallow the click that ends a drag so it doesn't pick a preset
  box.addEventListener('click', (e) => { if (moved) { e.stopPropagation(); e.preventDefault(); moved = false; } }, true);
  $('presetsLeft').addEventListener('click', () => box.scrollBy({ left: -box.clientWidth * 0.7, behavior: 'smooth' }));
  $('presetsRight').addEventListener('click', () => box.scrollBy({ left: box.clientWidth * 0.7, behavior: 'smooth' }));
  window.__revealActivePreset = () => {
    const a = box.querySelector('.chip.active');
    // scroll only the preset row (scrollIntoView would also shift the whole window)
    if (a && S && S.mode === 'compact') box.scrollTo({ left: a.offsetLeft - (box.clientWidth - a.offsetWidth) / 2, behavior: 'smooth' });
    update();
  };
})();

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

// ── parametric EQ (Equalizer APO style) ───────────────────────────────
const peqEd = new ParametricEditor({
  list: $('peqList'), canvas: $('peqCurve'), preRange: $('peqPre'), preInput: $('peqPreVal'),
  onChange: (peq) => { S.peq = peq; invoke('set_peq', { peq }); markCustom(); save(); },
});
$('peqOn').addEventListener('change', (e) => { peqEd.setEnabled(e.target.checked); toast(e.target.checked ? 'Parametric EQ on' : 'Parametric EQ off'); });
$('peqAdd').addEventListener('click', () => { if (!peqEd.add()) toast('Up to 20 filters'); });
$('peqAuto').addEventListener('click', () => { const v = peqEd.autoPreamp(); toast(`Preamp ${v.toFixed(1)} dB: just enough headroom`); });
$('peqExport').addEventListener('click', async () => {
  const text = toApo(peqEd.peq);
  try { await navigator.clipboard.writeText(text); toast('Copied as Equalizer APO config'); }
  catch { openApo(text); toast('Copy the text below'); }
});
function openApo(text = '') { $('apoText').value = text; $('apoErr').textContent = ''; $('apoModal').classList.add('show'); setTimeout(() => $('apoText').focus(), 50); }
$('peqImport').addEventListener('click', () => openApo(''));
$('apoCancel').addEventListener('click', () => $('apoModal').classList.remove('show'));
$('apoFile').addEventListener('click', () => $('apoFileInput').click());
$('apoFileInput').addEventListener('change', async (e) => { const f = e.target.files[0]; if (f) $('apoText').value = await f.text(); e.target.value = ''; });
$('apoApply').addEventListener('click', () => {
  try {
    const r = parseApo($('apoText').value);
    peqEd.load(r); peqEd.emit();
    $('peqOn').checked = true;
    if (r.graphic) applyPreset('Custom', r.graphic);
    if (r.ambience) setAmbience(r.ambience, true);
    $('apoModal').classList.remove('show');
    const parts = [`preamp ${r.preamp.toFixed(1)} dB`, `${r.filters.length} filters`];
    if (r.graphic) parts.push('GraphicEQ → 10-band');
    if (r.ambience) parts.push('Ambience on');
    toast(`Imported ${parts.join(' · ')}`);
    if (r.warnings.length) showNotice({ key: 'import', prio: 2, title: 'Imported with notes', text: r.warnings.join(' '), action: 'OK', onAction: () => hideNotice('import') });
  } catch (err) { $('apoErr').textContent = err.message; }
});

// EQ view tabs (remembered)
function setEqTab(tab) {
  document.querySelector('.eq-panel').classList.toggle('parametric', tab === 'parametric');
  document.querySelectorAll('[data-eqtab]').forEach((b) => b.classList.toggle('active', b.dataset.eqtab === tab));
  try { localStorage.setItem('eqTab', tab); } catch {}
  requestAnimationFrame(() => { peqEd.draw(); eq.dirty = true; });
}
document.querySelectorAll('[data-eqtab]').forEach((b) => b.addEventListener('click', () => setEqTab(b.dataset.eqtab)));
setEqTab((() => { try { return localStorage.getItem('eqTab'); } catch { return null; } })() || 'graphic');

// ── sound profiles (system-wide) ──────────────────────────────────────
const CHECK_SVG = '<svg class="pi-check" viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
const X_SVG = '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>';
let profiles = { profiles: [], active: '' };
function markCustom() { if (profiles.active) { profiles.active = ''; renderProfiles(); } }
function renderProfiles() {
  $('profileName').textContent = profiles.active || 'Custom';
  $('btnProfiles').title = `Sound profile: ${profiles.active || 'Custom (unsaved changes)'} · Ctrl+Alt+1–9`;
  const list = $('profileList');
  list.innerHTML = profiles.profiles.length ? '' : '<div class="pop-empty">No profiles yet. Save your current sound below.</div>';
  profiles.profiles.forEach((name, i) => {
    const el = document.createElement('div');
    el.className = 'pop-item' + (name === profiles.active ? ' active' : '');
    el.innerHTML = `<span class="pi-key">${i < 9 ? `<kbd title="Ctrl+Alt+${i + 1}">${i + 1}</kbd>` : ''}</span><span class="pi-name"></span>${CHECK_SVG}<button class="pi-del" title="Delete profile">${X_SVG}</button>`;
    el.querySelector('.pi-name').textContent = name;
    el.addEventListener('click', async (e) => {
      if (e.target.closest('.pi-del')) {
        profiles = await invoke('delete_profile', { name }); renderProfiles(); toast(`Deleted “${name}”`); return;
      }
      try { await invoke('apply_profile', { name }); closeProfiles(); } catch (err) { toast(String(err)); }
    });
    list.appendChild(el);
  });
}
function openProfiles() {
  $('profilesPop').classList.add('open'); $('btnProfiles').classList.add('open');
  invoke('list_profiles').then((p) => { if (p) { profiles = p; renderProfiles(); } });
  setTimeout(() => document.addEventListener('pointerdown', outsideProfiles, true));
}
function closeProfiles() {
  $('profilesPop').classList.remove('open'); $('btnProfiles').classList.remove('open');
  document.removeEventListener('pointerdown', outsideProfiles, true);
}
function outsideProfiles(e) { if (!e.target.closest('#profilesPop, #btnProfiles')) closeProfiles(); }
$('btnProfiles').addEventListener('click', () => ($('profilesPop').classList.contains('open') ? closeProfiles() : openProfiles()));
async function saveProfile() {
  const name = $('profileNew').value.trim();
  if (!name) { $('profileNew').focus(); return; }
  try { profiles = await invoke('save_profile', { name }); $('profileNew').value = ''; renderProfiles(); toast(`Saved profile “${name}”`); }
  catch (err) { toast(String(err)); }
}
$('profileSave').addEventListener('click', saveProfile);
$('profileImport').addEventListener('click', () => $('profileImportInput').click());
$('profileImportInput').addEventListener('change', async (e) => {
  const files = [...e.target.files];
  e.target.value = '';
  if (!files.length) return;
  const list = [], notes = [];
  for (const f of files) {
    const name = profileNameFromFile(f.name);
    try {
      const r = parseApo(await f.text());
      list.push({
        name, boost: S.boost, eq: r.graphic || Array(10).fill(0), preset: r.graphic ? 'Custom' : 'Flat',
        peq: { enabled: true, preamp: r.preamp, filters: r.filters },
        ambience: r.ambience || { enabled: false, mix: 0.45, size: 1 },
        pitch_semitones: 0, pitch_cents: 0, width: 1, balance: 0, limiter: true,
      });
      r.warnings.forEach((w) => notes.push(`${name}: ${w}`));
    } catch (err) { notes.push(`${name}: ${err.message}`); }
  }
  if (list.length) {
    try { profiles = await invoke('import_profiles', { profiles: list }); renderProfiles(); }
    catch (err) { toast(String(err)); return; }
  }
  toast(`Imported ${list.length} of ${files.length} file${files.length > 1 ? 's' : ''} as profiles`);
  if (notes.length) showNotice({ key: 'import', prio: 2, title: 'Imported with notes', text: notes.join('  ·  '), action: 'OK', onAction: () => hideNotice('import') });
});
$('profileNew').addEventListener('keydown', (e) => { if (e.key === 'Enter') saveProfile(); if (e.key === 'Escape') closeProfiles(); });

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
// ── mic view ─────────────────────────────────────────────────────────
const mic = new MicView({ app, toast });
let micOn = false;
function setMicView(on, user = true) {
  if (user) {
    app.classList.add('morphing');
    setTimeout(() => { apply(); requestAnimationFrame(() => app.classList.remove('morphing')); }, 200);
  } else apply();
  function apply() {
    app.classList.toggle('mic', on);
    micOn = on;
    app.classList.remove('sheet'); $('btnSettings').classList.remove('active');
    $('btnMicView').title = on ? 'Back to the sound booster' : 'Mic: voice processing, voice modes, soundboard';
    mic.setVisible(on);
    if (!on) eq.dirty = true;
  }
  try { localStorage.setItem('micView', on ? '1' : ''); } catch {}
}
$('btnMicView').addEventListener('click', () => setMicView(!app.classList.contains('mic')));
$('btnSettings').addEventListener('click', () => {
  if (S.mode === 'compact') app.classList.toggle('sheet');
  else document.querySelector('.settings-panel').animate([{ transform: 'scale(1)' }, { transform: 'scale(1.02)' }, { transform: 'scale(1)' }], { duration: 450, easing: 'cubic-bezier(.34,1.56,.64,1)' });
  $('btnSettings').classList.toggle('active', app.classList.contains('sheet'));
});
$('closeSettings').addEventListener('click', () => { app.classList.remove('sheet'); $('btnSettings').classList.remove('active'); });
$('btnMin').addEventListener('click', () => invoke('win_minimize'));

// ── maximize / full screen ───────────────────────────────────────────
let win = { maximized: false, fullscreen: false };
function applyWin(w) {
  if (!w) return;
  win = w;
  const big = w.maximized || w.fullscreen;
  app.classList.toggle('maxed', big);
  $('btnMax').title = big ? 'Restore down' : 'Maximize (double-click title bar · F11 for full screen)';
  // a maximized window always uses the extended layout
  if (big && S && S.mode === 'compact') setMode('extended');
  requestAnimationFrame(() => { eq.dirty = true; peqEd.draw(); });
}
async function toggleMax() {
  if (S && S.mode === 'compact' && !win.maximized && !win.fullscreen) await setMode('extended');
  applyWin(await invoke('win_toggle_maximize'));
}
$('btnMax').addEventListener('click', toggleMax);
window.addEventListener('keydown', async (e) => {
  if (e.key === 'F11') {
    e.preventDefault();
    if (S && S.mode === 'compact') await setMode('extended');
    applyWin(await invoke('win_toggle_fullscreen'));
  } else if (e.key === 'Escape' && win.fullscreen) {
    applyWin(await invoke('win_toggle_fullscreen'));
  }
});
// double-clicking the title bar (handled by Tauri) or Win+Up also change the state
let winTimer = 0;
window.addEventListener('resize', () => {
  clearTimeout(winTimer);
  winTimer = setTimeout(async () => { try { applyWin(await invoke('win_state')); } catch {} }, 120);
});
$('btnClose').addEventListener('click', () => { invoke('win_hide'); if (!isTauri) toast('Hidden to tray (keeps boosting)'); });

// ── settings panel ───────────────────────────────────────────────────
const selects = [];
function fillDevices(d) {
  if (!selects.length) selects.push(new GlassSelect($('selInput')), new GlassSelect($('selOutput')));
  const fill = (sel, list, cur, filter) => {
    sel.innerHTML = '';
    const auto = document.createElement('option'); auto.value = ''; auto.textContent = 'Automatic'; sel.appendChild(auto);
    list.filter(filter).forEach((n) => { const o = document.createElement('option'); o.value = o.textContent = n; sel.appendChild(o); });
    sel.value = cur && list.includes(cur) ? cur : '';
  };
  fill($('selInput'), d.inputs, S.input_device, () => true);
  fill($('selOutput'), d.outputs, S.output_device, (n) => !/cable|virtual|voicemeeter/i.test(n));
  selects.forEach((s) => s.refresh());
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

const darkMq = window.matchMedia ? matchMedia('(prefers-color-scheme: dark)') : null;
function applyTheme(theme) {
  // auto follows Windows light/dark; glass is a see-through dark window
  const light = theme === 'light' || (theme === 'auto' && darkMq && !darkMq.matches);
  app.classList.toggle('light', light);
  app.classList.toggle('glass', theme === 'glass');
  document.querySelectorAll('[data-theme]').forEach((b) => b.classList.toggle('active', b.dataset.theme === theme));
  liquid.light = light;
  const ink = light ? '0,0,0' : '255,255,255';
  viz.ink = ring.ink = eq.ink = peqEd.ink = ink; viz.bg = light ? '217,217,217' : '4,4,4';
  peqEd.draw();
  eq.dirty = true;
}
document.querySelectorAll('[data-theme]').forEach((b) => b.addEventListener('click', () => {
  S.theme = b.dataset.theme; applyTheme(S.theme);
  invoke('set_window_effect', { theme: S.theme }).catch(() => {});
  invoke('set_ui', { theme: S.theme }); save();
  toast({ dark: 'Dark theme', light: 'Light theme', auto: 'Theme follows Windows', glass: 'Glass: see-through window' }[S.theme]);
}));
darkMq && darkMq.addEventListener('change', () => { if (S && S.theme === 'auto') applyTheme('auto'); });

// Graphics quality. The biggest cost is the moving light field behind the
// frosted panels: every redraw makes each blurred panel re-blur. So
// "balanced" redraws it at 15 fps (it moves slowly, the eye can't tell).
const QUALITY = {
  low: { bgFps: 0, vizDpr: 1, glow: false, refract: false, fpsCap: 30 },
  balanced: { bgFps: 15, vizDpr: 1.5, glow: true, refract: false, fpsCap: 60 },
  high: { bgFps: 60, vizDpr: 2, glow: true, refract: true, fpsCap: 144 },
};
let Q = QUALITY.balanced;
function applyQuality(q) {
  Q = QUALITY[q] || QUALITY.balanced;
  ['low', 'balanced', 'high'].forEach((k) => app.classList.toggle('q-' + k, k === q));
  app.classList.toggle('refract', Q.refract);
  setRenderQuality(Q.vizDpr, Q.glow);
  document.querySelectorAll('[data-quality]').forEach((b) => b.classList.toggle('active', b.dataset.quality === q));
  liquid.draw(0.016); // keep a still frame in Fast mode
}
document.querySelectorAll('[data-quality]').forEach((b) => b.addEventListener('click', () => {
  S.quality = b.dataset.quality; applyQuality(S.quality); invoke('set_ui', { quality: S.quality }); save();
}));
window.__setQuality = applyQuality;

// ── ambience (echo & room) ───────────────────────────────────────────
function showAmbience() {
  const a = S.ambience || { enabled: false, mix: 0.45, size: 1 };
  $('tglAmb').checked = !!a.enabled;
  $('rngAmbMix').value = a.mix; $('rngAmbSize').value = a.size;
  rangeFill($('rngAmbMix')); rangeFill($('rngAmbSize'));
  $('ambMixVal').textContent = `${Math.round(a.mix * 100)}%`;
  $('ambSizeVal').textContent = a.size < 0.8 ? 'Small' : a.size > 1.4 ? 'Hall' : 'Room';
}
function setAmbience(a, send = true) {
  S.ambience = { enabled: !!a.enabled, mix: +a.mix, size: +a.size };
  showAmbience();
  if (send) { invoke('set_ambience', { ambience: S.ambience }); markCustom(); save(); }
}
$('tglAmb').addEventListener('change', (e) => setAmbience({ ...S.ambience, enabled: e.target.checked }));
$('rngAmbMix').addEventListener('input', (e) => setAmbience({ ...S.ambience, mix: +e.target.value, enabled: true }));
$('rngAmbSize').addEventListener('input', (e) => setAmbience({ ...S.ambience, size: +e.target.value, enabled: true }));

// ── Windows volume slider (applied by SonicBoost while output is the cable)
let winVol = { percent: 100, muted: false, active: false };
function showWinVol() {
  const el = $('winVol');
  if (!winVol.active) { el.textContent = ''; return; }
  const out = Math.round(((S ? S.boost : 1) * winVol.percent));
  el.textContent = winVol.muted ? 'Windows muted' : `Windows ${winVol.percent}% · Output ${out}%`;
  el.classList.toggle('muted', winVol.muted);
}
$('tglFollowVol').addEventListener('change', (e) => {
  invoke('set_follow_volume', { enabled: e.target.checked });
  toast(e.target.checked ? 'Windows volume slider controls SonicBoost' : 'Windows volume slider ignored');
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
  requestAnimationFrame(() => { pill.title = txt.textContent; });
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

// ── routing diagnostics: tell the user when nothing is being boosted ──
const isVirtual = (n) => /cable|virtual|voicemeeter/i.test(n || '');
let silentSince = 0, lastDefault = null;
async function checkRouting() {
  if (!status.running) { hideNotice('route'); hideNotice('silent'); return; }
  let d;
  try { d = await invoke('list_devices'); } catch { return; }
  if (d.default_output !== lastDefault) { lastDefault = d.default_output; dismissed.delete('route'); }
  if (d.default_output && !isVirtual(d.default_output)) {
    if (!dismissed.has('route')) showNotice({
      key: 'route', prio: 3, warn: true,
      title: 'Not boosting yet',
      text: `Windows is playing straight to “${d.default_output}”. Set the output to CABLE Input so sound goes through SonicBoost.`,
      action: 'Open Sound settings', onAction: () => invoke('open_link', { target: 'sound-settings' }),
    });
  } else hideNotice('route');
}
function checkSilence() {
  // music is playing but no audio reaches the engine → routing problem
  const playing = document.querySelector('.np-panel.playing');
  if (status.running && playing && !(S && S.bypass) && audio.energy < 0.004) {
    if (!silentSince) silentSince = performance.now();
    if (performance.now() - silentSince > 5000 && noticeKey !== 'route' && !dismissed.has('silent')) showNotice({
      key: 'silent', prio: 2, warn: true,
      title: 'No sound is reaching SonicBoost',
      text: 'Music is playing, but SonicBoost hears silence. Check that Windows output is CABLE Input and the app is not muted in Volume mixer.',
      action: 'Open Sound settings', onAction: () => invoke('open_link', { target: 'sound-settings' }),
    });
  } else { silentSince = 0; hideNotice('silent'); }
}

// ── updates ──────────────────────────────────────────────────────────
let appVersion = '1.0.0';
const fmtMB = (b) => (b / 1048576).toFixed(1) + ' MB';
async function checkForUpdates(manual) {
  const st = $('updateStatus'), btn = $('btnCheckUpdate');
  btn.disabled = true; st.textContent = 'Checking GitHub…';
  try {
    const u = await invoke('check_update');
    if (u && u.available) {
      st.textContent = `Version ${u.latest} is available${u.size ? ` (${fmtMB(u.size)})` : ''}.`;
      btn.textContent = `Update to ${u.latest}`;
      btn.onclick = () => installUpdate(u);
      showNotice({
        key: 'update', prio: 1,
        title: `Update available: SonicBoost ${u.latest}`,
        text: `You have ${u.current}. Installs in about a minute and keeps all your settings.`,
        action: 'Update now', onAction: () => installUpdate(u),
      });
    } else {
      st.textContent = `You're up to date (v${u ? u.current : appVersion}).`;
      if (manual) toast("You're on the latest version");
    }
  } catch (e) {
    st.textContent = 'Could not check for updates. Are you online?';
    if (manual) toast('Update check failed');
  } finally { btn.disabled = false; }
}
async function installUpdate(u) {
  showNotice({ key: 'update', prio: 4, title: `Downloading SonicBoost ${u.latest}…`, text: 'SonicBoost will restart by itself when the update is installed.' });
  $('notice').classList.add('busy'); $('noticeAction').style.display = 'none';
  try {
    await invoke('install_update', { url: u.url });
  } catch (e) {
    showNotice({ key: 'update', prio: 4, warn: true, title: 'Update failed', text: String(e), action: 'Try again', onAction: () => installUpdate(u) });
  }
}
$('btnCheckUpdate').addEventListener('click', () => checkForUpdates(true));
$('tglAutoUpdate').addEventListener('change', (e) => { S.auto_update = e.target.checked; invoke('set_auto_update', { enabled: e.target.checked }); });

// ── cursor-following specular light on the glass ─────────────────────
// Only the panel under the cursor is updated (reading every panel's rect each
// frame forced a full layout).
let mouse = null, sheenPanel = null;
app.addEventListener('pointermove', (e) => { mouse = e; });
function updateSheen() {
  if (!mouse) return;
  const p = mouse.target.closest && mouse.target.closest('.panel');
  if (p) {
    const r = p.getBoundingClientRect();
    p.style.setProperty('--mx', `${mouse.clientX - r.left}px`);
    p.style.setProperty('--my', `${mouse.clientY - r.top}px`);
  }
  sheenPanel = p;
  mouse = null;
}

// cached sizes (no layout reads inside the animation loop)
let knobRadius = 76;
new ResizeObserver(() => { knobRadius = $('knob').getBoundingClientRect().width / 2; }).observe($('knob'));
const energyEls = [document.querySelector('.logo'), null];
let lastEnergy = -1, energyT = 0, bgT = 0;

// ── render loop ──────────────────────────────────────────────────────
let last = performance.now(), running = true;
let frameAcc = 0;
const meterL = $('meterL'), meterR = $('meterR');
function loop(now) {
  if (!running) return;
  requestAnimationFrame(loop);
  const raw = (now - last) / 1000;
  // frame cap (Fast: 30 fps; unfocused window: 30 fps)
  const cap = document.hasFocus() ? Q.fpsCap : Math.min(30, Q.fpsCap);
  frameAcc += raw; last = now;
  if (frameAcc < 1 / cap - 0.002) return;
  const dt = Math.min(0.05, frameAcc); frameAcc = 0;
  audio.update(dt);
  // the glow variable only feeds the logo and the knob: set it on them (not
  // on #app, which restyled the whole page every frame), at most 30x/s
  energyT += dt;
  if (energyT > 1 / 30) {
    energyT = 0;
    const e = Math.round((S && S.bypass ? audio.energy * 0.4 : audio.energy) * 100) / 100;
    if (e !== lastEnergy) {
      lastEnergy = e;
      energyEls[1] ||= document.querySelector('.knob .orb');
      for (const el of energyEls) el && el.style.setProperty('--energy', e);
    }
  }
  if (Q.bgFps) { bgT += dt; if (bgT >= 1 / Q.bgFps) { liquid.draw(bgT); bgT = 0; } }
  if (micOn) { mic.tick(); updateSheen(); return; } // booster panels are hidden: draw nothing for them
  viz.draw(dt);
  ring.draw(dt, knobRadius, !(S && S.bypass));
  knob.tick(dt); pitch.tick(dt); eq.tick(dt); np.tick(dt);
  meterL.style.setProperty('--v', Math.min(1, audio.peakL).toFixed(3));
  meterR.style.setProperty('--v', Math.min(1, audio.peakR).toFixed(3));
  updateSheen();
}
document.addEventListener('visibilitychange', () => {
  const vis = !document.hidden;
  invoke('set_viz_active', { enabled: vis }).catch(() => {});
  if (vis && !running) { running = true; last = performance.now(); requestAnimationFrame(loop); }
  if (!vis) running = false;
});

// ── boot ─────────────────────────────────────────────────────────────
/** Puts the backend state into every control without sending anything back. */
function applySound() {
  knob.set(S.boost); showBoost(S.boost);
  $('btnLimiter').classList.toggle('on', S.limiter);
  pitch.set(S.pitch_semitones, false); showPitch(false);
  eq.setAll(S.eq);
  renderPresets();
  S.peq ||= SIGNATURE();
  peqEd.load(S.peq); $('peqOn').checked = !!S.peq.enabled;
  $('rngWidth').value = S.width ?? 1; $('rngBalance').value = S.balance ?? 0;
  ['rngWidth', 'rngBalance'].forEach((id) => rangeFill($(id)));
  showStereo();
  showAmbience();
  profiles = { profiles: (S.profiles || []).map((p) => p.name), active: S.active_profile || '' };
  renderProfiles();
}

async function boot() {
  const st = await invoke('get_state');
  S = st.settings;
  S.custom_presets ||= [];
  applyTheme(S.theme);
  applyQuality(S.quality || 'balanced');
  if (S.theme === 'glass') invoke('set_window_effect', { theme: 'glass' }).catch(() => {});
  $('tglFollowVol').checked = S.follow_windows_volume !== false;
  await setMode(S.mode, false);
  applySound();
  setBypass(S.bypass, false);
  setViz(S.viz_mode || 'radial', false);
  $('rngIntensity').value = S.intensity ?? 1; audio.intensity = S.intensity ?? 1;
  rangeFill($('rngIntensity'));
  $('intVal').textContent = `${Math.round(audio.intensity * 100)}%`;
  appVersion = st.version || appVersion;
  $('appVersion').textContent = appVersion;
  $('tglAutoUpdate').checked = S.auto_update !== false;
  $('tglAutostart').checked = st.autostart;
  $('tglMinimized').checked = S.start_minimized;
  fillDevices(st.devices);
  setStatus(st.status);

  listen('audio-frame', (f) => audio.feed(f));
  await listen('media-status', (m) => np.update(m));
  invoke('media_control', { action: 'refresh' }).catch(() => {});
  listen('engine-ready', (s) => setStatus(s)); // devices opened in the background at startup
  listen('engine-status', (s) => { setStatus(s); toast(s.running ? 'Audio device changed — reconnected' : 'Audio device lost'); });
  listen('bypass-changed', (on) => setBypass(on, false));
  listen('windows-volume', (v) => { winVol = v; showWinVol(); });
  listen('state-reload', async (name) => {
    const st2 = await invoke('get_state');
    S = st2.settings; applySound();
    toast(`Profile: ${name}`);
  });
  listen('boost-changed', (v) => { S.boost = v; knob.set(v); showBoost(v); toast(`Boost ${Math.round(v * 100)}%`); });
  listen('update-progress', (p) => {
    const pct = p.total ? Math.round((p.done / p.total) * 100) : 0;
    $('noticeProgress').firstElementChild.style.transform = `scaleX(${(pct / 100).toFixed(3)})`;
    $('noticeText').textContent = p.total ? `${fmtMB(p.done)} of ${fmtMB(p.total)} · ${pct}%` : fmtMB(p.done);
  });

  mic.listen();
  mic.load();
  setMicView((() => { try { return localStorage.getItem('micView') === '1'; } catch { return false; } })() || /mic/.test(location.search), false);
  checkRouting();
  setInterval(checkRouting, 5000);
  setInterval(checkSilence, 1000);
  if (S.auto_update !== false) setTimeout(() => checkForUpdates(false), 4000);

  requestAnimationFrame(() => app.classList.remove('booting'));
  requestAnimationFrame(loop);
}
boot().catch((e) => { console.error(e); app.classList.remove('booting'); toast('Failed to load: ' + e); });
