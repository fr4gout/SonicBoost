//! SonicBoost Mic engine.
//!
//!   your mic ──WASAPI──▶ resample → 48 kHz ─▶ RNNoise (10 ms frames, optional)
//!            ─▶ MicChain (gate, EQ, compressor, de-esser, voice mode, soundboard, limiter)
//!            ─▶ ring ─▶ virtual cable ("CABLE In 16ch")  → apps use "CABLE Out 16ch" as their mic
//!            ─▶ ring ─▶ monitor headphones (optional, "hear yourself")
//!
//! Runs on its own threads, completely independent from the speaker booster.

use crate::audio::{is_cable, same_cable, AtomicF32};
use crate::dsp::Resampler;
use crate::micdsp::{MicChain, MicParams};
use crate::ring::Ring;
use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample, StreamConfig};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const FRAME: usize = 480; // RNNoise frame: 10 ms at 48 kHz
const TARGET_MS: usize = 20;
const REC_SECONDS: usize = 5;

pub struct MicShared {
    pub params: Mutex<MicParams>,
    pub version: AtomicU64,
    pub ns_on: AtomicBool,
    pub running: AtomicBool,
    pub device_error: AtomicBool,
    pub monitor_vol: AtomicF32,
    /// Hard switch for the monitor: when false nothing reaches the headphones,
    /// even before the engine has finished restarting.
    pub monitor_on: AtomicBool,
    pub in_level: AtomicF32,
    pub out_level: AtomicF32,
    pub gr_db: AtomicF32,
    pub vad: AtomicF32,
    pub gate_open: AtomicBool,
    pub wave: Arc<Ring>,
    pub sb_queue: Mutex<Vec<Arc<Vec<f32>>>>,
    pub sb_stop: AtomicBool,
    pub rec: Mutex<Vec<f32>>,
    pub recording: AtomicBool,
}

impl MicShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            params: Mutex::new(MicParams::default()),
            version: AtomicU64::new(1),
            ns_on: AtomicBool::new(true),
            running: AtomicBool::new(false),
            device_error: AtomicBool::new(false),
            monitor_vol: AtomicF32::new(0.8),
            monitor_on: AtomicBool::new(false),
            in_level: AtomicF32::new(0.0),
            out_level: AtomicF32::new(0.0),
            gr_db: AtomicF32::new(0.0),
            vad: AtomicF32::new(0.0),
            gate_open: AtomicBool::new(false),
            wave: Ring::new(8192),
            sb_queue: Mutex::new(Vec::with_capacity(16)),
            sb_stop: AtomicBool::new(false),
            rec: Mutex::new(Vec::new()),
            recording: AtomicBool::new(false),
        })
    }

    pub fn set_params(&self, p: MicParams) {
        if let Ok(mut g) = self.params.lock() {
            *g = p;
        }
        self.version.fetch_add(1, Ordering::Release);
    }

    pub fn play_clip(&self, clip: Arc<Vec<f32>>) {
        if let Ok(mut q) = self.sb_queue.lock() {
            if q.len() < 16 {
                q.push(clip);
            }
        }
    }

    pub fn start_recording(&self) {
        if let Ok(mut r) = self.rec.lock() {
            r.clear();
            r.reserve(48_000 * REC_SECONDS);
        }
        self.recording.store(true, Ordering::Release);
    }
}

#[derive(Serialize, Clone, Default)]
pub struct MicStatus {
    pub running: bool,
    pub input: Option<String>,
    pub output: Option<String>,
    pub monitor: Option<String>,
    pub latency_ms: u32,
    pub error: Option<String>,
    /// Recording devices that Windows itself plays to the speakers
    /// ("Listen to this device"): the usual cause of hearing yourself.
    pub listening: Vec<String>,
}

#[derive(Serialize, Clone, Default)]
pub struct MicDevices {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub suggested_output: Option<String>,
    pub default_input: Option<String>,
}

/// "CABLE Output (…)" (what the booster listens to) → "CABLE Input (…)":
/// the mic must never be written into the booster's own cable.
/// Names of recording devices with Windows' "Listen to this device" switched on.
/// Read from the registry (MMDevices), so no audio APIs are touched.
#[cfg(windows)]
pub fn listen_enabled() -> Vec<String> {
    use std::collections::HashMap;
    use std::os::windows::process::CommandExt;
    const ROOT: &str = r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture";
    const LISTEN: &str = "{24dbb0fc-9311-4b3d-9cf0-18ff155639d4},1";
    const DESC: &str = "{a45c254e-df1c-4efd-8020-67d146a850e0},2";
    const IFACE: &str = "{b3f8fa53-0004-438e-9003-51a46e139bfc},6";
    // value name → (device key → data)
    let query = |value: &str| -> HashMap<String, String> {
        let mut out = HashMap::new();
        let Ok(o) = std::process::Command::new("reg")
            .args(["query", ROOT, "/s", "/v", value])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
        else {
            return out;
        };
        let text = String::from_utf8_lossy(&o.stdout);
        let mut key = String::new();
        for line in text.lines() {
            if line.starts_with("HKEY_") {
                key = line.trim().trim_end_matches("\\Properties").to_string();
            } else if let Some(rest) = line.trim().strip_prefix(value) {
                let mut parts = rest.split_whitespace();
                let _kind = parts.next();
                let data: Vec<&str> = parts.collect();
                out.insert(key.clone(), data.join(" "));
            }
        }
        out
    };
    // one pass over all recording devices for the Listen flag only
    let on: Vec<String> = query(LISTEN)
        .into_iter()
        // REG_BINARY PROPVARIANT: VT_BOOL (0B00) with boolVal at byte 8; TRUE = FFFF
        .filter(|(_, hex)| hex.len() >= 20 && hex[..4].eq_ignore_ascii_case("0B00") && hex[16..20].eq_ignore_ascii_case("FFFF"))
        .map(|(k, _)| k)
        .collect();
    // names only for the few devices that have it switched on
    let read = |key: &str, value: &str| -> Option<String> {
        let props = format!("{key}\\Properties");
        let o = std::process::Command::new("reg")
            .args(["query", props.as_str(), "/v", value])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        text.lines().find_map(|l| {
            let rest = l.trim().strip_prefix(value)?;
            let mut parts = rest.split_whitespace();
            let _kind = parts.next();
            Some(parts.collect::<Vec<_>>().join(" "))
        })
    };
    on.iter()
        .filter_map(|k| {
            let d = read(k, DESC)?;
            Some(match read(k, IFACE) {
                Some(i) if !i.is_empty() => format!("{d} ({i})"),
                _ => d,
            })
        })
        .collect()
}
#[cfg(not(windows))]
pub fn listen_enabled() -> Vec<String> {
    Vec::new()
}

pub fn render_side_of(capture_name: &str) -> String {
    capture_name.replacen("Output", "Input", 1).replacen("CABLE Out ", "CABLE In ", 1)
}

pub fn list_devices(forbid: Option<&str>) -> MicDevices {
    let host = cpal::default_host();
    let inputs: Vec<String> = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).filter(|n| !is_cable(n)).collect())
        .unwrap_or_default();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    // any cable except the booster's own (both of its inputs count)
    let ok = |n: &String| is_cable(n) && !forbid.is_some_and(|f| same_cable(n, f));
    let suggested_output = outputs.iter().find(|n| ok(n)).cloned();
    let default_input = host.default_input_device().and_then(|d| d.name().ok()).filter(|n| !is_cable(n));
    MicDevices { inputs, outputs, suggested_output, default_input }
}

enum Cmd {
    Start { input: Option<String>, output: Option<String>, monitor: Option<String>, forbid: Option<String>, reply: Sender<MicStatus> },
    Stop,
}

pub struct MicEngine {
    tx: Sender<Cmd>,
}

impl MicEngine {
    pub fn spawn(shared: Arc<MicShared>) -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("mic-engine".into())
            .spawn(move || {
                let mut streams: Vec<cpal::Stream> = Vec::new();
                for cmd in rx {
                    match cmd {
                        Cmd::Start { input, output, monitor, forbid, reply } => {
                            streams.clear();
                            shared.running.store(false, Ordering::Release);
                            let st = match build(&shared, input.as_deref(), output.as_deref(), monitor.as_deref(), forbid.as_deref()) {
                                Ok((s, st)) => {
                                    streams = s;
                                    shared.running.store(true, Ordering::Release);
                                    st
                                }
                                Err(e) => MicStatus { error: Some(format!("{e:#}")), ..Default::default() },
                            };
                            let _ = reply.send(st);
                        }
                        Cmd::Stop => {
                            streams.clear();
                            shared.running.store(false, Ordering::Release);
                        }
                    }
                }
            })
            .expect("spawn mic thread");
        Self { tx }
    }

    pub fn start(&self, input: Option<String>, output: Option<String>, monitor: Option<String>, forbid: Option<String>) -> MicStatus {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::Start { input, output, monitor, forbid, reply }).is_err() {
            return MicStatus { error: Some("Mic thread stopped".into()), ..Default::default() };
        }
        rx.recv_timeout(Duration::from_secs(8))
            .unwrap_or_else(|_| MicStatus { error: Some("Mic device timed out".into()), ..Default::default() })
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

fn find(host: &cpal::Host, name: Option<&str>, input: bool) -> Result<Option<cpal::Device>> {
    let Some(want) = name else { return Ok(None) };
    let mut it: Box<dyn Iterator<Item = cpal::Device>> =
        if input { Box::new(host.input_devices()?) } else { Box::new(host.output_devices()?) };
    Ok(it.find(|d| d.name().map(|n| n == want).unwrap_or(false)))
}

fn build(
    shared: &Arc<MicShared>,
    input: Option<&str>,
    output: Option<&str>,
    monitor: Option<&str>,
    forbid: Option<&str>,
) -> Result<(Vec<cpal::Stream>, MicStatus)> {
    let host = cpal::default_host();
    let in_dev = match find(&host, input, true)? {
        Some(d) => d,
        None => host
            .default_input_device()
            .filter(|d| d.name().map(|n| !is_cable(&n)).unwrap_or(false))
            .or_else(|| host.input_devices().ok()?.find(|d| d.name().map(|n| !is_cable(&n)).unwrap_or(false)))
            .ok_or_else(|| anyhow!("No microphone found."))?,
    };
    let out_name = match output {
        Some(n) => n.to_string(),
        None => list_devices(forbid)
            .suggested_output
            .ok_or_else(|| anyhow!("MIC_CABLE_NOT_FOUND: The mic needs a second, separate virtual cable (VB-Audio Hi-Fi Cable or CABLE A+B). CABLE In 16ch is the other end of the booster's own cable, so it can't be used."))?,
    };
    if forbid.is_some_and(|f| same_cable(&out_name, f)) {
        bail!("\"{out_name}\" is part of the cable the speaker booster records, so your voice would play through your speakers. Install a second cable (VB-Audio Hi-Fi Cable or CABLE A+B) and pick it here.");
    }
    let out_dev = find(&host, Some(&out_name), false)?.ok_or_else(|| anyhow!("Mic output \"{out_name}\" not found."))?;
    let mon_dev = match monitor {
        Some("default") | Some("") => host
            .default_output_device()
            .filter(|d| d.name().map(|n| !is_cable(&n)).unwrap_or(false))
            .or_else(|| host.output_devices().ok()?.find(|d| d.name().map(|n| !is_cable(&n)).unwrap_or(false))),
        Some(n) => find(&host, Some(n), false)?,
        None => None,
    };

    let in_sup = in_dev.default_input_config().context("reading mic format")?;
    let in_cfg: StreamConfig = in_sup.config();
    let out_sup = out_dev.default_output_config().context("reading cable format")?;
    let out_cfg: StreamConfig = out_sup.config();

    let cable = Ring::new(out_cfg.sample_rate.0 as usize);
    let mut streams = Vec::new();
    let (mon_ring, mon_rate, mon_parts) = match &mon_dev {
        Some(d) => {
            let sup = d.default_output_config().context("reading headphone format")?;
            let cfg: StreamConfig = sup.config();
            (Some(Ring::new(cfg.sample_rate.0 as usize)), cfg.sample_rate.0, Some((d.clone(), sup, cfg)))
        }
        None => (None, 48_000, None),
    };

    let ctx = CaptureCtx {
        shared: shared.clone(),
        cable: cable.clone(),
        mon: mon_ring.clone(),
        in_ch: in_cfg.channels as usize,
        to48: Resampler::new(in_cfg.sample_rate.0, 48_000),
        to_cable: Resampler::new(48_000, out_cfg.sample_rate.0),
        to_mon: Resampler::new(48_000, mon_rate),
    };
    let cap = match in_sup.sample_format() {
        cpal::SampleFormat::F32 => capture::<f32>(&in_dev, &in_cfg, ctx)?,
        cpal::SampleFormat::I16 => capture::<i16>(&in_dev, &in_cfg, ctx)?,
        cpal::SampleFormat::I32 => capture::<i32>(&in_dev, &in_cfg, ctx)?,
        cpal::SampleFormat::U16 => capture::<u16>(&in_dev, &in_cfg, ctx)?,
        f => bail!("Unsupported mic sample format {f:?}"),
    };
    let out = playback_any(&out_dev, &out_sup, &out_cfg, cable, shared.clone(), false)?;
    cap.play().context("starting mic")?;
    out.play().context("starting mic output")?;
    streams.push(cap);
    streams.push(out);
    let mut mon_name = None;
    if let (Some((d, sup, cfg)), Some(ring)) = (mon_parts, mon_ring) {
        let s = playback_any(&d, &sup, &cfg, ring, shared.clone(), true)?;
        s.play().context("starting monitor")?;
        mon_name = d.name().ok();
        streams.push(s);
    }
    let st = MicStatus {
        running: true,
        input: in_dev.name().ok(),
        output: Some(out_name),
        monitor: mon_name,
        latency_ms: (FRAME as u32 * 1000 / 48_000) + TARGET_MS as u32 + 5,
        error: None,
        listening: Vec::new(),
    };
    Ok((streams, st))
}

struct CaptureCtx {
    shared: Arc<MicShared>,
    cable: Arc<Ring>,
    mon: Option<Arc<Ring>>,
    in_ch: usize,
    to48: Resampler,
    to_cable: Resampler,
    to_mon: Resampler,
}

fn capture<T>(dev: &cpal::Device, cfg: &StreamConfig, mut c: CaptureCtx) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut chain = MicChain::new();
    let mut denoise = nnnoiseless::DenoiseState::new();
    let mut inb = [0f32; FRAME];
    let mut outb = [0f32; FRAME];
    let mut fill = 0usize;
    let mut seen = 0u64;
    let err = c.shared.clone();
    let stream = dev.build_input_stream(
        cfg,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let sh = &c.shared;
            // parameter changes (try_lock: never waits in the audio thread)
            let v = sh.version.load(Ordering::Acquire);
            if v != seen {
                if let Ok(p) = sh.params.try_lock() {
                    chain.set(*p);
                    seen = v;
                }
            }
            if let Ok(mut q) = sh.sb_queue.try_lock() {
                while let Some(clip) = q.pop() {
                    chain.soundboard.play(clip);
                }
            }
            if sh.sb_stop.swap(false, Ordering::AcqRel) {
                chain.soundboard.stop();
            }
            let ns = sh.ns_on.load(Ordering::Relaxed);
            let (mut pin, mut pout) = (0f32, 0f32);
            for frame in data.chunks(c.in_ch) {
                let m = frame.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / c.in_ch as f32;
                // resampling can yield 0, 1 or 2 samples per input sample: keep them all
                let mut tmp = [0f32; 4];
                let mut tn = 0usize;
                c.to48.push(m, m, |a, _| {
                    if tn < 4 {
                        tmp[tn] = a;
                        tn += 1;
                    }
                });
                for &a in &tmp[..tn] {
                    inb[fill] = a;
                    fill += 1;
                    if fill < FRAME {
                        continue;
                    }
                    fill = 0;
                    // one 10 ms block: noise suppression, then the voice chain
                    let vad = if ns {
                        let scaled: [f32; FRAME] = std::array::from_fn(|i| inb[i] * 32768.0);
                        let v = denoise.process_frame(&mut outb, &scaled);
                        for o in outb.iter_mut() {
                            *o /= 32768.0;
                        }
                        v
                    } else {
                        outb.copy_from_slice(&inb);
                        1.0
                    };
                    sh.vad.set(vad);
                    let rec = sh.recording.load(Ordering::Relaxed);
                    let mut rec_buf = if rec { sh.rec.try_lock().ok() } else { None };
                    for i in 0..FRAME {
                        pin = pin.max(inb[i].abs());
                        let y = chain.process(outb[i]);
                        pout = pout.max(y.abs());
                        sh.wave.push(y);
                        c.to_cable.push(y, y, |a, _| {
                            c.cable.push(a);
                        });
                        if let (Some(r), true) = (&c.mon, sh.monitor_on.load(Ordering::Relaxed)) {
                            c.to_mon.push(y, y, |a, _| {
                                r.push(a);
                            });
                        }
                        if let Some(buf) = rec_buf.as_mut() {
                            if buf.len() < buf.capacity() {
                                buf.push(y);
                            } else {
                                sh.recording.store(false, Ordering::Release);
                            }
                        }
                    }
                }
            }
            sh.in_level.set(pin.max(sh.in_level.get()));
            sh.out_level.set(pout.max(sh.out_level.get()));
            sh.gr_db.set(chain.comp.gr_db + chain.deess.gr_db);
            sh.gate_open.store(chain.gate.is_open(), Ordering::Relaxed);
        },
        move |e| {
            eprintln!("mic capture error: {e}");
            err.device_error.store(true, Ordering::Release);
        },
        None,
    )?;
    Ok(stream)
}

fn playback_any(
    dev: &cpal::Device,
    sup: &cpal::SupportedStreamConfig,
    cfg: &StreamConfig,
    ring: Arc<Ring>,
    shared: Arc<MicShared>,
    monitor: bool,
) -> Result<cpal::Stream> {
    Ok(match sup.sample_format() {
        cpal::SampleFormat::F32 => playback::<f32>(dev, cfg, ring, shared, monitor)?,
        cpal::SampleFormat::I16 => playback::<i16>(dev, cfg, ring, shared, monitor)?,
        cpal::SampleFormat::I32 => playback::<i32>(dev, cfg, ring, shared, monitor)?,
        cpal::SampleFormat::U16 => playback::<u16>(dev, cfg, ring, shared, monitor)?,
        f => bail!("Unsupported output sample format {f:?}"),
    })
}

/// Mono ring → device (first two channels; extra channels of e.g. a 16-ch cable get silence).
fn playback<T>(dev: &cpal::Device, cfg: &StreamConfig, ring: Arc<Ring>, shared: Arc<MicShared>, monitor: bool) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = cfg.channels as usize;
    let target = cfg.sample_rate.0 as usize * TARGET_MS / 1000;
    let mut primed = false;
    let err = shared.clone();
    let stream = dev.build_output_stream(
        cfg,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let avail = ring.len();
            if !primed && avail >= target {
                primed = true;
            }
            if primed && avail > target * 4 {
                ring.skip(avail - target);
            }
            let vol = match (monitor, shared.monitor_on.load(Ordering::Relaxed)) {
                (false, _) => 1.0,
                (true, true) => shared.monitor_vol.get(),
                (true, false) => {
                    ring.skip(ring.len());
                    0.0
                }
            };
            for frame in data.chunks_mut(ch) {
                let mut s = 0.0;
                if primed {
                    match ring.pop() {
                        Some(v) => s = v * vol,
                        None => primed = false,
                    }
                }
                for (i, o) in frame.iter_mut().enumerate() {
                    *o = T::from_sample(if i < 2 { s } else { 0.0f32 });
                }
            }
        },
        move |e| {
            eprintln!("mic output error: {e}");
            err.device_error.store(true, Ordering::Release);
        },
        None,
    )?;
    Ok(stream)
}

/// Plays the 5-second test recording on the headphones / default output.
pub fn play_test(device: Option<String>, samples: Vec<f32>) -> Result<(), String> {
    if samples.is_empty() {
        return Err("Nothing recorded yet".into());
    }
    thread::spawn(move || {
        let host = cpal::default_host();
        let dev = device
            .and_then(|n| host.output_devices().ok()?.find(|d| d.name().map(|x| x == n).unwrap_or(false)))
            .or_else(|| host.default_output_device());
        let Some(dev) = dev else { return };
        let Ok(sup) = dev.default_output_config() else { return };
        let cfg: StreamConfig = sup.config();
        let ring = Ring::new(samples.len() * 2 + 48_000);
        let mut rs = Resampler::new(48_000, cfg.sample_rate.0);
        for &s in &samples {
            rs.push(s, s, |a, _| {
                ring.push(a);
            });
        }
        let secs = samples.len() as f32 / 48_000.0;
        let dummy = MicShared::new();
        dummy.monitor_vol.set(1.0);
        dummy.monitor_on.store(true, Ordering::Relaxed);
        if let Ok(stream) = playback_any(&dev, &sup, &cfg, ring, dummy, true) {
            if stream.play().is_ok() {
                thread::sleep(Duration::from_secs_f32(secs + 0.4));
            }
        }
    });
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct MicFrame {
    pub running: bool,
    pub input: f32,
    pub output: f32,
    pub gr_db: f32,
    pub gate: bool,
    pub voice: f32,
    pub recording: bool,
    pub wave: Vec<f32>,
}

/// Meters + waveform for the Mic view, 30 times a second while visible.
pub fn spawn_meters(app: AppHandle, shared: Arc<MicShared>, visible: Arc<AtomicBool>) {
    thread::Builder::new()
        .name("mic-meters".into())
        .spawn(move || {
            let mut hist: Vec<f32> = vec![0.0; 1024];
            loop {
                thread::sleep(Duration::from_millis(33));
                while let Some(s) = shared.wave.pop() {
                    hist.push(s);
                }
                if hist.len() > 1024 {
                    hist.drain(..hist.len() - 1024);
                }
                if !visible.load(Ordering::Relaxed) || crate::audio::ui_busy() {
                    continue;
                }
                let f = MicFrame {
                    running: shared.running.load(Ordering::Acquire),
                    input: shared.in_level.get(),
                    output: shared.out_level.get(),
                    gr_db: shared.gr_db.get(),
                    gate: shared.gate_open.load(Ordering::Relaxed),
                    voice: shared.vad.get(),
                    recording: shared.recording.load(Ordering::Relaxed),
                    wave: hist.iter().step_by(8).cloned().collect(),
                };
                shared.in_level.set(0.0);
                shared.out_level.set(0.0);
                let _ = app.emit("mic-frame", f);
            }
        })
        .expect("spawn mic meters");
}
