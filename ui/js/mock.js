// Demo backend used only when the UI is opened in a normal browser.
const PRESET_EQ = Array(10).fill(0);
const state = {
  settings: {
    boost: 1.5, eq: [...PRESET_EQ], preset: 'Flat', pitch_semitones: 0, pitch_cents: 0,
    width: 1, balance: 0, custom_presets: [], bypass: false, limiter: true,
    input_device: null, output_device: null, mode: 'compact', viz_mode: 'radial',
    theme: 'dark', intensity: 1, start_minimized: true, first_run: false,
  },
  status: { running: true, input: 'CABLE Output (VB-Audio Virtual Cable)', output: 'Speakers (Realtek(R) Audio)', sample_rate: 48000, input_rate: 48000, latency_ms: 35, error: null },
  autostart: true,
  devices: {
    inputs: ['CABLE Output (VB-Audio Virtual Cable)', 'Microphone (USB Audio)'],
    outputs: ['Speakers (Realtek(R) Audio)', 'Headphones (WH-1000XM5)', 'CABLE Input (VB-Audio Virtual Cable)'],
    default_output: 'CABLE Input (VB-Audio Virtual Cable)', cable_input: 'CABLE Output (VB-Audio Virtual Cable)', cable_found: true,
  },
};

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
  setTimeout(() => emit('media-status', mediaStatus(true)), 300);
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
      case 'list_devices': return state.devices;
      case 'set_boost': s.boost = a.value; break;
      case 'set_eq_band': s.eq[a.index] = a.gain; s.preset = 'Custom'; break;
      case 'set_eq': s.eq = [...a.gains]; s.preset = a.preset; break;
      case 'set_pitch': s.pitch_semitones = a.semitones; s.pitch_cents = a.cents; break;
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
