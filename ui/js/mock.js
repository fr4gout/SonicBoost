// Demo backend used only when the UI is opened in a normal browser.
const PRESET_EQ = Array(10).fill(0);
const state = {
  settings: {
    boost: 1.5, eq: [...PRESET_EQ], preset: 'Flat', pitch_semitones: 0, pitch_cents: 0,
    width: 1, balance: 0, custom_presets: [], bypass: false, limiter: true,
    peq: { enabled: false, preamp: -5, filters: [
      { enabled: true, kind: 'PK', freq: 80, gain: -5, q: 10 }, { enabled: true, kind: 'PK', freq: 1800, gain: 3, q: 1 },
      { enabled: true, kind: 'PK', freq: 3200, gain: 3.5, q: 1.2 }, { enabled: true, kind: 'PK', freq: 8000, gain: -2, q: 1 }] },
    profiles: [], active_profile: '',
    ambience: { enabled: false, mix: 0.45, size: 1 }, quality: 'balanced', follow_windows_volume: true,
    input_device: null, output_device: null, mode: 'compact', viz_mode: 'radial',
    theme: 'dark', intensity: 1, start_minimized: true, auto_update: true, first_run: false,
  },
  version: '1.0.0',
  status: { running: true, input: 'CABLE Output (VB-Audio Virtual Cable)', output: 'Speakers (Realtek(R) Audio)', sample_rate: 48000, input_rate: 48000, latency_ms: 35, error: null, listening: /listen/.test(location.search) ? ['CABLE Out 16ch (VB-Audio Virtual Cable)'] : [] },
  autostart: true,
  devices: {
    inputs: ['CABLE Output (VB-Audio Virtual Cable)', 'Microphone (USB Audio)'],
    outputs: ['Speakers (Realtek(R) Audio)', 'Headphones (WH-1000XM5)', 'CABLE Input (VB-Audio Virtual Cable)'],
    default_output: 'CABLE Input (VB-Audio Virtual Cable)', cable_input: 'CABLE Output (VB-Audio Virtual Cable)', cable_found: true,
  },
};

state.settings.profiles = [
  { name: 'SYNC Signature', boost: 1.5, eq: Array(10).fill(0), preset: 'Flat', peq: { ...structuredClone(state.settings.peq), enabled: true }, pitch_semitones: 0, pitch_cents: 0, width: 1, balance: 0, limiter: true },
  { name: 'Flat (no processing)', boost: 1, eq: Array(10).fill(0), preset: 'Flat', peq: { ...structuredClone(state.settings.peq), enabled: false }, pitch_semitones: 0, pitch_cents: 0, width: 1, balance: 0, limiter: true },
];

// ── mic (mirrors settings.rs defaults and built-in presets) ─────────────
const micCfg = (o = {}) => ({ ns: true, boost_db: 0, gate_on: true, gate_db: -45, eq_preset: 1, comp_on: true, comp_amount: 0.5,
  deess_on: false, deess_amount: 0.5, mode: 'natural', custom_pitch: 0, custom_room: 0, limiter_on: true, out_db: 0, ...o });
const micBuiltin = [
  ['Discord / Gaming', {}], ['Streaming', { boost_db: 3, gate_db: -50, eq_preset: 2, comp_amount: 0.65, deess_on: true }],
  ['Podcast', { gate_db: -52, eq_preset: 2, comp_amount: 0.75, deess_on: true, deess_amount: 0.6 }],
  ['Meetings / Zoom', { gate_db: -48, eq_preset: 3, comp_amount: 0.4 }], ['Noisy room', { boost_db: 2, gate_db: -38 }],
  ['Singing', { ns: false, gate_on: false, eq_preset: 5, comp_amount: 0.35, deess_on: true, deess_amount: 0.4 }],
  ['Whisper / ASMR', { boost_db: 12, gate_db: -60, eq_preset: 4, comp_amount: 0.8 }],
  ['Raw (no processing)', { ns: false, gate_on: false, eq_preset: 0, comp_on: false }],
].map(([name, o]) => ({ name, cfg: micCfg(o) }));
const mic = {
  settings: { enabled: !/micoff/.test(location.search), input: null, output: null, monitor_on: false, monitor_device: null, monitor_volume: 0.8,
    ptt: 'off', muted: false, cfg: micCfg({ boost_db: 3, gate_db: -50, eq_preset: 2, comp_amount: 0.65, deess_on: true }), preset: 'Streaming',
    presets: [{ name: 'My stream voice', cfg: micCfg({ boost_db: 4, eq_preset: 2, comp_amount: 0.7, deess_on: true }) }],
    soundboard: [{ name: 'Airhorn', file: '' }, { name: 'Applause', file: '' }, { name: 'Drum roll', file: '' }, { name: 'Laugh track', file: '' }, { name: 'Bruh', file: '' }, { name: 'GG', file: '' }], sb_gain: 0.8 },
  devices: { inputs: ['Microphone (USB Audio)', 'Headset Microphone (WH-1000XM5)'], outputs: ['Speakers (Realtek(R) Audio)', 'Headphones (WH-1000XM5)', 'CABLE In 16ch (VB-Audio Virtual Cable)'],
    suggested_output: 'CABLE In 16ch (VB-Audio Virtual Cable)', default_input: 'Microphone (USB Audio)' },
  status: null,
};
const micStatus = () => (mic.settings.enabled
  ? { running: true, input: mic.settings.input || 'Microphone (USB Audio)', output: mic.settings.output || mic.devices.suggested_output, monitor: mic.settings.monitor_on ? 'Headphones (WH-1000XM5)' : null, latency_ms: 35, error: null, listening: /listen/.test(location.search) ? ['CABLE Out 16ch (VB-Audio Virtual Cable)'] : [] }
  : { running: false, input: null, output: null, monitor: null, latency_ms: 0, error: null, listening: [] });
let micVisible = false, micRec = 0;
function micFrame() {
  if (!micVisible) return;
  const t = performance.now() / 1000, on = mic.settings.enabled && !mic.settings.muted;
  const talk = on ? Math.max(0, Math.sin(t * 1.7) * 0.6 + Math.sin(t * 5.3) * 0.3 + 0.25) : 0;
  const lvl = 0.004 + talk * 0.32;
  const wave = [];
  for (let i = 0; i < 128; i++) wave.push(talk * (Math.sin(i * 0.9 + t * 40) * 0.3 + Math.sin(i * 0.23 + t * 13) * 0.25 + (Math.random() - 0.5) * 0.12) * Math.sin((i / 127) * Math.PI));
  emit('mic-frame', { running: on, input: lvl, output: lvl * 1.4, gr_db: talk * 9, gate: talk > 0.08, voice: talk > 0.15 ? 0.9 : 0.1, recording: micRec > performance.now(), wave });
}

const listeners = {};
const emit = (ev, data) => (listeners[ev] || []).forEach((cb) => cb(data));

// ── fake music ──────────────────────────────────────────────────────────
const tracks = [
  { title: 'Midnight Drive (Extended Mix)', artist: 'Neon Avenue', album: 'Afterglow', dur: 214, seed: 1 },
  { title: 'Glass Ocean', artist: 'Lumen & Vale', album: 'Refraction', dur: 187, seed: 2 },
  { title: 'Monochrome Heart', artist: 'Kairo', album: 'Greyscale Dreams', dur: 242, seed: 3 },
];
let ti = 0, playing = true, pos = 31, bpm = 124;

function artFor(seed) {
  const c = document.createElement('canvas');
  c.width = c.height = 300;
  const g = c.getContext('2d');
  const grd = g.createLinearGradient(0, 0, 300, 300);
  const hues = [[280, 200], [190, 330], [20, 260]][seed - 1];
  grd.addColorStop(0, `hsl(${hues[0]} 80% 55%)`);
  grd.addColorStop(1, `hsl(${hues[1]} 80% 25%)`);
  g.fillStyle = grd; g.fillRect(0, 0, 300, 300);
  for (let i = 0; i < 14; i++) {
    g.beginPath();
    g.arc(150, 150, 20 + i * 11, 0, Math.PI * 2);
    g.strokeStyle = `rgba(255,255,255,${0.04 + (i % 3) * 0.04})`;
    g.lineWidth = 3; g.stroke();
  }
  g.fillStyle = 'rgba(255,255,255,.9)';
  g.font = 'bold 34px Segoe UI'; g.fillText(tracks[seed - 1].artist.split(' ')[0].toUpperCase(), 24, 270);
  return c.toDataURL('image/jpeg', 0.85);
}

function mediaStatus(withArt) {
  const t = tracks[ti];
  const s = {
    available: true, title: t.title, artist: t.artist, album: t.album, app: 'Spotify',
    status: playing ? 'playing' : 'paused', position: pos, duration: t.dur, track_id: t.title,
  };
  if (withArt) s.art = artFor(t.seed);
  return s;
}

// ── fake spectrum that reacts to the EQ / boost / pitch controls ────────
let t0 = performance.now();
function frame() {
  const t = (performance.now() - t0) / 1000;
  const s = state.settings;
  const beat = playing ? Math.pow(1 - ((t * bpm / 60) % 1), 4) : 0;
  const snare = playing ? Math.pow(1 - (((t * bpm / 60) + 0.5) % 1), 6) * 0.6 : 0;
  const gain = s.bypass ? 1 : Math.min(s.boost, 3) / 1.5;
  const shift = s.bypass ? 0 : (s.pitch_semitones + s.pitch_cents / 100) * 0.9;
  const bands = [];
  for (let i = 0; i < 72; i++) {
    const x = i / 71;
    const fi = Math.max(0, Math.min(9, Math.round(x * 9)));
    const eq = s.bypass ? 0 : s.eq[fi];
    let v = 0.62 - x * 0.28 + Math.sin(i * 0.7 + t * 3) * 0.06 + Math.random() * 0.06;
    v += beat * Math.max(0, 0.5 - Math.abs(x - 0.06 - shift / 120) * 4);
    v += snare * Math.max(0, 0.35 - Math.abs(x - 0.55) * 1.2);
    v += Math.sin(t * 1.3 + x * 9) * 0.05 + eq / 40;
    v *= playing ? gain * 0.85 + 0.15 : 0.08;
    bands.push(Math.max(0, Math.min(1, v)));
  }
  const wave = [];
  for (let i = 0; i < 256; i++) {
    const p = i / 256;
    wave.push(playing ? (Math.sin(p * 18 + t * 9) * 0.35 + Math.sin(p * 51 - t * 4) * 0.15 + (Math.random() - 0.5) * 0.08) * (0.4 + beat * 0.6) * gain : 0);
  }
  const pk = playing ? Math.min(1, 0.45 + beat * 0.5) * gain : 0;
  emit('audio-frame', { bands, wave, peak_l: pk, peak_r: pk * 0.96, rms: pk * 0.5 });
}

let started = false;
function start() {
  if (started) return;
  started = true;
  setInterval(frame, 16);
  setInterval(micFrame, 33);
  setTimeout(() => emit('media-status', mediaStatus(true)), 300);
  setTimeout(() => emit('windows-volume', { percent: 40, muted: false, active: true }), 400);
  setInterval(() => {
    if (playing) pos += 1;
    if (pos >= tracks[ti].dur) { ti = (ti + 1) % tracks.length; pos = 0; emit('media-status', mediaStatus(true)); return; }
    emit('media-status', mediaStatus(false));
  }, 1000);
}

export const mock = {
  listen(ev, cb) {
    (listeners[ev] ||= []).push(cb);
    start();
    return Promise.resolve(() => {});
  },
  async invoke(cmd, a) {
    const s = state.settings;
    switch (cmd) {
      case 'get_state': return structuredClone(state);
      case 'list_devices': return /route/.test(location.search) ? { ...state.devices, default_output: 'Speakers (Realtek(R) Audio)' } : state.devices;
      case 'set_boost': s.boost = a.value; break;
      case 'set_eq_band': s.eq[a.index] = a.gain; s.preset = 'Custom'; break;
      case 'set_eq': s.eq = [...a.gains]; s.preset = a.preset; break;
      case 'set_pitch': s.pitch_semitones = a.semitones; s.pitch_cents = a.cents; break;
      case 'set_ambience': s.ambience = a.ambience; s.active_profile = ''; break;
      case 'set_follow_volume': s.follow_windows_volume = a.enabled; break;
      case 'set_window_effect': break;
      case 'import_profiles':
        for (const p of a.profiles) { const i = s.profiles.findIndex((x) => x.name.toLowerCase() === p.name.toLowerCase()); if (i >= 0) s.profiles[i] = p; else s.profiles.push(p); }
        return { profiles: s.profiles.map((p) => p.name), active: s.active_profile };
      case 'set_peq': s.peq = a.peq; s.active_profile = ''; break;
      case 'list_profiles': return { profiles: s.profiles.map((p) => p.name), active: s.active_profile };
      case 'save_profile': {
        const snap = { name: a.name, boost: s.boost, eq: [...s.eq], preset: s.preset, peq: structuredClone(s.peq), pitch_semitones: s.pitch_semitones, pitch_cents: s.pitch_cents, width: s.width, balance: s.balance, limiter: s.limiter };
        const i = s.profiles.findIndex((p) => p.name.toLowerCase() === a.name.toLowerCase());
        if (i >= 0) s.profiles[i] = snap; else s.profiles.push(snap);
        s.active_profile = a.name;
        return { profiles: s.profiles.map((p) => p.name), active: s.active_profile };
      }
      case 'apply_profile': {
        const p = s.profiles.find((x) => x.name === a.name);
        Object.assign(s, structuredClone(p)); delete s.name; s.active_profile = a.name;
        setTimeout(() => emit('state-reload', a.name), 30);
        return a.name;
      }
      case 'delete_profile':
        s.profiles = s.profiles.filter((p) => p.name !== a.name);
        if (s.active_profile === a.name) s.active_profile = '';
        return { profiles: s.profiles.map((p) => p.name), active: s.active_profile };
      case 'win_state': return { maximized: !!state.win?.max, fullscreen: !!state.win?.fs };
      case 'win_toggle_maximize': state.win = { ...state.win, max: !state.win?.max, fs: false }; return { maximized: state.win.max, fullscreen: false };
      case 'win_toggle_fullscreen': state.win = { ...state.win, fs: !state.win?.fs }; return { maximized: !!state.win.max, fullscreen: state.win.fs };
      case 'set_stereo': s.width = a.width; s.balance = a.balance; break;
      case 'set_custom_presets': s.custom_presets = a.presets; break;
      case 'set_bypass': s.bypass = a.enabled; break;
      case 'set_limiter': s.limiter = a.enabled; break;
      case 'set_ui': Object.assign(s, { viz_mode: a.viz ?? s.viz_mode, theme: a.theme ?? s.theme, intensity: a.intensity ?? s.intensity }); break;
      case 'set_mode': s.mode = a.mode; break;
      case 'set_devices': s.input_device = a.input; s.output_device = a.output; return state.status;
      case 'start_engine': return state.status;
      case 'stop_engine': return { ...state.status, running: false };
      case 'set_autostart': state.autostart = a.enabled; return a.enabled;
      case 'check_update': return /update/.test(location.search)
        ? { current: '1.0.0', latest: '1.0.1', available: true, notes: '', url: 'https://github.com/fr4gout/SonicBoost/releases/download/v1.0.1/SonicBoost_1.0.1_x64-setup.exe', page: '', size: 4718592 }
        : { current: '1.0.0', latest: '1.0.0', available: false, notes: '', url: null, page: '', size: 0 };
      case 'install_update': { let d = 0; const t = setInterval(() => { d += 400000; emit('update-progress', { done: Math.min(d, 4718592), total: 4718592 }); if (d >= 4718592) clearInterval(t); }, 150); return new Promise(() => {}); }
      case 'mic_get': return structuredClone({ settings: mic.settings, status: micStatus(), devices: mic.devices, builtin: micBuiltin, modes: ['natural', 'broadcast', 'deep', 'high', 'chipmunk', 'robot', 'radio', 'telephone', 'megaphone', 'monster', 'alien', 'cave', 'custom'] });
      case 'mic_set': { const sb = mic.settings.soundboard; mic.settings = structuredClone(a.mic); mic.settings.soundboard = sb; return micStatus(); }
      case 'mic_mute': mic.settings.muted = a.muted ?? !mic.settings.muted; return mic.settings.muted;
      case 'mic_visible': micVisible = a.enabled; break;
      case 'mic_test': if (!mic.settings.enabled) throw 'Turn the mic on first'; if (a.action === 'record') micRec = performance.now() + 5000; break;
      case 'sb_add': mic.settings.soundboard.push({ name: a.name, file: '' }); return structuredClone(mic.settings.soundboard);
      case 'sb_remove': mic.settings.soundboard.splice(a.index, 1); return structuredClone(mic.settings.soundboard);
      case 'sb_play': case 'sb_stop': break;
      case 'set_auto_update': state.settings.auto_update = a.enabled; break;
      case 'media_control':
        if (a.action === 'refresh') { setTimeout(() => emit('media-status', mediaStatus(true)), 50); break; }
        if (a.action === 'play_pause') playing = !playing;
        else { ti = (ti + (a.action === 'next' ? 1 : tracks.length - 1)) % tracks.length; pos = 0; }
        setTimeout(() => emit('media-status', mediaStatus(a.action !== 'play_pause')), 120);
        break;
      default: break;
    }
    return null;
  },
};
