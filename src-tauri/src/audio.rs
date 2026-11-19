//! Audio engine: captures system audio from a virtual cable (e.g. VB-Audio
//! "CABLE Output"), runs it through the DSP chain and plays it on your real
//! speakers/headphones through WASAPI (cpal).
//!
//!   Apps -> [Windows default: CABLE Input] -> CABLE Output -> SonicBoost -> Speakers

use crate::analyzer::{Analyzer, Frame};
use crate::dsp::{DspParams, FilterKind, PeqBand, Processor, Resampler, EQ_BANDS, PEQ_MAX};
use crate::ring::Ring;
use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SizedSample, StreamConfig};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

// ------------------------------------------------------------ atomics ---

pub struct AtomicF32(AtomicU32);
impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

/// Parameters shared between the UI thread and the audio callback.
pub struct Params {
    pub boost: AtomicF32,
    pub eq: [AtomicF32; EQ_BANDS],
    pub pitch: AtomicF32,
    pub width: AtomicF32,
    pub balance: AtomicF32,
    pub bypass: AtomicBool,
    pub limiter: AtomicBool,
    pub version: AtomicU64,
}

impl Params {
    fn new() -> Self {
        Self {
            boost: AtomicF32::new(1.0),
            eq: std::array::from_fn(|_| AtomicF32::new(0.0)),
            pitch: AtomicF32::new(0.0),
            width: AtomicF32::new(1.0),
            balance: AtomicF32::new(0.0),
            bypass: AtomicBool::new(false),
            limiter: AtomicBool::new(true),
            version: AtomicU64::new(1),
        }
    }
    pub fn bump(&self) {
        self.version.fetch_add(1, Ordering::Release);
    }
    pub fn snapshot(&self) -> DspParams {
        DspParams {
            boost: self.boost.get(),
            eq_db: std::array::from_fn(|i| self.eq[i].get()),
            pitch_st: self.pitch.get(),
            width: self.width.get(),
            balance: self.balance.get(),
            bypass: self.bypass.load(Ordering::Relaxed),
            limiter: self.limiter.load(Ordering::Relaxed),
        }
    }
}

/// Parametric EQ, already converted to DSP form on the UI thread so the
/// audio callback only copies plain values (it uses `try_lock`, never waits).
#[derive(Clone, Copy)]
pub struct PeqState {
    pub enabled: bool,
    pub preamp: f32,
    pub bands: [PeqBand; PEQ_MAX],
    pub n: usize,
}

impl PeqState {
    pub fn from_settings(p: &crate::settings::ParamEq) -> Self {
        let blank = PeqBand { enabled: false, kind: FilterKind::Peaking, freq: 1000.0, gain_db: 0.0, q: 1.0 };
        let mut bands = [blank; PEQ_MAX];
        let n = p.filters.len().min(PEQ_MAX);
        for (i, f) in p.filters.iter().take(n).enumerate() {
            bands[i] = PeqBand { enabled: f.enabled, kind: FilterKind::from_code(&f.kind), freq: f.freq, gain_db: f.gain, q: f.q };
        }
        Self { enabled: p.enabled, preamp: p.preamp, bands, n }
    }
}

pub struct Shared {
    pub params: Params,
    pub peq: Mutex<PeqState>,
    pub peq_version: AtomicU64,
    pub peak_l: AtomicF32,
    pub peak_r: AtomicF32,
    pub sample_rate: AtomicU32,
    pub running: AtomicBool,
    /// Set by stream error callbacks (e.g. headphones unplugged).
    pub device_error: AtomicBool,
    /// False while the window is hidden: analyzer stops to save CPU.
    pub viz_active: AtomicBool,
    pub analysis: Arc<Ring>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            params: Params::new(),
            peq: Mutex::new(PeqState::from_settings(&crate::settings::ParamEq { enabled: false, ..Default::default() })),
            peq_version: AtomicU64::new(1),
            peak_l: AtomicF32::new(0.0),
            peak_r: AtomicF32::new(0.0),
            sample_rate: AtomicU32::new(48_000),
            running: AtomicBool::new(false),
            device_error: AtomicBool::new(false),
            viz_active: AtomicBool::new(true),
            analysis: Ring::new(16_384),
        })
    }
}

// -------------------------------------------------------------- devices ---

#[derive(Serialize, Clone, Default)]
pub struct DeviceList {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_output: Option<String>,
    pub cable_input: Option<String>,
    pub cable_found: bool,
}

pub fn is_cable(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("cable") || n.contains("voicemeeter") || n.contains("virtual")
}

pub fn list_devices() -> DeviceList {
    let host = cpal::default_host();
    let inputs: Vec<String> = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    let outputs: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    let default_output = host.default_output_device().and_then(|d| d.name().ok());
    let cable_input = inputs.iter().find(|n| is_cable(n)).cloned();
    DeviceList { cable_found: cable_input.is_some(), inputs, outputs, default_output, cable_input }
}

fn find_input(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    let devices: Vec<cpal::Device> = host.input_devices()?.collect();
    if let Some(want) = name {
        if let Some(d) = devices.iter().find(|d| d.name().map(|n| n == want).unwrap_or(false)) {
            return Ok(d.clone());
        }
    }
    devices
        .into_iter()
        .find(|d| d.name().map(|n| is_cable(&n)).unwrap_or(false))
        .ok_or_else(|| anyhow!("CABLE_NOT_FOUND: No virtual audio cable found. Install VB-Audio Virtual Cable, then press Refresh."))
}

fn find_output(host: &cpal::Host, name: Option<&str>) -> Result<cpal::Device> {
    let devices: Vec<cpal::Device> = host.output_devices()?.collect();
    if let Some(want) = name {
        if let Some(d) = devices.iter().find(|d| d.name().map(|n| n == want).unwrap_or(false)) {
            return Ok(d.clone());
        }
    }
    // Prefer the Windows default if it's a real device, else first real device.
    if let Some(d) = host.default_output_device() {
        if d.name().map(|n| !is_cable(&n)).unwrap_or(false) {
            return Ok(d);
        }
    }
    devices
        .into_iter()
        .find(|d| d.name().map(|n| !is_cable(&n)).unwrap_or(false))
        .ok_or_else(|| anyhow!("No speakers/headphones found."))
}

// --------------------------------------------------------------- engine ---

#[derive(Serialize, Clone, Default)]
pub struct EngineStatus {
    pub running: bool,
    pub input: Option<String>,
    pub output: Option<String>,
    pub sample_rate: u32,
    pub input_rate: u32,
    pub latency_ms: u32,
    pub error: Option<String>,
}

enum Cmd {
    Start { input: Option<String>, output: Option<String>, reply: Sender<EngineStatus> },
    Stop,
}

pub struct Engine {
    tx: Sender<Cmd>,
}

impl Engine {
    /// cpal streams are not Send on every platform, so one dedicated thread
    /// owns them and takes commands over a channel.
    pub fn spawn(shared: Arc<Shared>) -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        thread::Builder::new()
            .name("audio-engine".into())
            .spawn(move || {
                let mut streams: Option<(cpal::Stream, cpal::Stream)> = None;
                for cmd in rx {
                    match cmd {
                        Cmd::Start { input, output, reply } => {
                            streams = None; // drop old streams first
                            shared.running.store(false, Ordering::Release);
                            let status = match build(&shared, input.as_deref(), output.as_deref()) {
                                Ok((s, st)) => {
                                    streams = Some(s);
                                    shared.running.store(true, Ordering::Release);
                                    st
                                }
                                Err(e) => EngineStatus { error: Some(format!("{e:#}")), ..Default::default() },
                            };
                            let _ = reply.send(status);
                        }
                        Cmd::Stop => {
                            streams = None;
                            shared.running.store(false, Ordering::Release);
                        }
                    }
                }
                drop(streams);
            })
            .expect("spawn audio thread");
        Self { tx }
    }

    pub fn start(&self, input: Option<String>, output: Option<String>) -> EngineStatus {
        let (reply, rx) = mpsc::channel();
        if self.tx.send(Cmd::Start { input, output, reply }).is_err() {
            return EngineStatus { error: Some("Audio thread stopped".into()), ..Default::default() };
        }
        rx.recv_timeout(Duration::from_secs(8))
            .unwrap_or_else(|_| EngineStatus { error: Some("Audio device timed out".into()), ..Default::default() })
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

const TARGET_LATENCY_MS: usize = 25;

fn build(
    shared: &Arc<Shared>,
    input: Option<&str>,
    output: Option<&str>,
) -> Result<((cpal::Stream, cpal::Stream), EngineStatus)> {
    let host = cpal::default_host();
    let in_dev = find_input(&host, input)?;
    let out_dev = find_output(&host, output)?;
    let in_name = in_dev.name().unwrap_or_default();
    let out_name = out_dev.name().unwrap_or_default();
    if is_cable(&out_name) {
        bail!("Output can't be a virtual cable (that would loop). Pick your real speakers or headphones.");
    }

    let in_sup = in_dev.default_input_config().context("reading input device format")?;
    let out_sup = out_dev.default_output_config().context("reading output device format")?;
    let in_cfg: StreamConfig = in_sup.config();
    let out_cfg: StreamConfig = out_sup.config();
    let in_rate = in_cfg.sample_rate.0;
    let out_rate = out_cfg.sample_rate.0;
    shared.sample_rate.store(out_rate, Ordering::Relaxed);

    // ~1 s of stereo audio between capture and playback.
    let ring = Ring::new(out_rate as usize * 2);

    let in_stream = match in_sup.sample_format() {
        cpal::SampleFormat::F32 => build_input::<f32>(&in_dev, &in_cfg, ring.clone(), in_rate, out_rate, shared.clone())?,
        cpal::SampleFormat::I16 => build_input::<i16>(&in_dev, &in_cfg, ring.clone(), in_rate, out_rate, shared.clone())?,
        cpal::SampleFormat::I32 => build_input::<i32>(&in_dev, &in_cfg, ring.clone(), in_rate, out_rate, shared.clone())?,
        cpal::SampleFormat::U16 => build_input::<u16>(&in_dev, &in_cfg, ring.clone(), in_rate, out_rate, shared.clone())?,
        f => bail!("Unsupported input sample format {f:?}"),
    };
    let out_stream = match out_sup.sample_format() {
        cpal::SampleFormat::F32 => build_output::<f32>(&out_dev, &out_cfg, ring, shared.clone())?,
        cpal::SampleFormat::I16 => build_output::<i16>(&out_dev, &out_cfg, ring, shared.clone())?,
        cpal::SampleFormat::I32 => build_output::<i32>(&out_dev, &out_cfg, ring, shared.clone())?,
        cpal::SampleFormat::U16 => build_output::<u16>(&out_dev, &out_cfg, ring, shared.clone())?,
        f => bail!("Unsupported output sample format {f:?}"),
    };
    in_stream.play().context("starting capture")?;
    out_stream.play().context("starting playback")?;

    let status = EngineStatus {
        running: true,
        input: Some(in_name),
        output: Some(out_name),
        sample_rate: out_rate,
        input_rate: in_rate,
        latency_ms: TARGET_LATENCY_MS as u32 + 10,
        error: None,
    };
    Ok(((in_stream, out_stream), status))
}

fn build_input<T>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    ring: Arc<Ring>,
    in_rate: u32,
    out_rate: u32,
    shared: Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = cfg.channels as usize;
    let mut rs = Resampler::new(in_rate, out_rate);
    let stream = dev.build_input_stream(
        cfg,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            for frame in data.chunks(ch) {
                let l: f32 = frame[0].to_sample::<f32>();
                let r: f32 = if ch > 1 { frame[1].to_sample::<f32>() } else { l };
                rs.push(l, r, |a, b| {
                    if ring.free() >= 2 {
                        ring.push(a);
                        ring.push(b);
                    }
                });
            }
        },
        move |e| {
            eprintln!("capture error: {e}");
            shared.device_error.store(true, Ordering::Release);
        },
        None,
    )?;
    Ok(stream)
}

fn build_output<T>(
    dev: &cpal::Device,
    cfg: &StreamConfig,
    ring: Arc<Ring>,
    shared: Arc<Shared>,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    let ch = cfg.channels as usize;
    let rate = cfg.sample_rate.0 as usize;
    let err_shared = shared.clone();
    let target = (rate * 2 * TARGET_LATENCY_MS / 1000) & !1;
    let mut proc = Processor::new(rate as f32);
    let mut seen_version = 0u64;
    let mut seen_peq = 0u64;
    let mut primed = false;

    let stream = dev.build_output_stream(
        cfg,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            let v = shared.params.version.load(Ordering::Acquire);
            if v != seen_version {
                seen_version = v;
                proc.set_params(shared.params.snapshot());
            }
            let pv = shared.peq_version.load(Ordering::Acquire);
            if pv != seen_peq {
                if let Ok(p) = shared.peq.try_lock() {
                    proc.set_peq(p.enabled, p.preamp, &p.bands[..p.n]);
                    seen_peq = pv;
                }
            }

            // Clock-drift control between the two devices.
            let avail = ring.len();
            if !primed && avail >= target {
                primed = true;
            }
            if primed && avail > target * 4 {
                ring.skip((avail - target) & !1);
            }

            let (mut pl, mut pr) = (0f32, 0f32);
            for frame in data.chunks_mut(ch) {
                let (mut l, mut r) = (0.0, 0.0);
                if primed {
                    if ring.len() >= 2 {
                        l = ring.pop().unwrap_or(0.0);
                        r = ring.pop().unwrap_or(0.0);
                    } else {
                        primed = false; // underrun: wait to refill
                    }
                }
                let (ol, or) = proc.process(l, r);
                pl = pl.max(ol.abs());
                pr = pr.max(or.abs());
                shared.analysis.push((ol + or) * 0.5);
                if ch == 1 {
                    frame[0] = T::from_sample((ol + or) * 0.5);
                } else {
                    frame[0] = T::from_sample(ol);
                    frame[1] = T::from_sample(or);
                    for s in frame.iter_mut().skip(2) {
                        *s = T::from_sample(0.0f32);
                    }
                }
            }
            shared.peak_l.set(pl.max(shared.peak_l.get()));
            shared.peak_r.set(pr.max(shared.peak_r.get()));
        },
        move |e| {
            eprintln!("playback error: {e}");
            err_shared.device_error.store(true, Ordering::Release);
        },
        None,
    )?;
    Ok(stream)
}

// ------------------------------------------------------------- analyzer ---

impl Serialize for Frame {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Frame", 5)?;
        st.serialize_field("bands", &self.bands)?;
        st.serialize_field("wave", &self.wave)?;
        st.serialize_field("peak_l", &self.peak_l)?;
        st.serialize_field("peak_r", &self.peak_r)?;
        st.serialize_field("rms", &self.rms)?;
        st.end()
    }
}

/// Emits an `audio-frame` event to the UI ~60 times per second.
pub fn spawn_analyzer(app: AppHandle, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("analyzer".into())
        .spawn(move || {
            let mut an = Analyzer::new();
            let mut fresh: Vec<f32> = Vec::with_capacity(8192);
            let mut idle_sent = false;
            loop {
                thread::sleep(Duration::from_millis(16));
                fresh.clear();
                while let Some(s) = shared.analysis.pop() {
                    fresh.push(s);
                }
                if !shared.viz_active.load(Ordering::Relaxed) {
                    continue; // window hidden: skip FFT work entirely
                }
                if !shared.running.load(Ordering::Acquire) {
                    if !idle_sent {
                        an.feed(&vec![0.0; crate::analyzer::FFT_SIZE]);
                        let _ = app.emit("audio-frame", an.frame(48_000.0, 0.0, 0.0));
                        idle_sent = true;
                    }
                    continue;
                }
                idle_sent = false;
                an.feed(&fresh);
                let pl = shared.peak_l.get();
                let pr = shared.peak_r.get();
                shared.peak_l.set(0.0);
                shared.peak_r.set(0.0);
                let fs = shared.sample_rate.load(Ordering::Relaxed) as f32;
                let _ = app.emit("audio-frame", an.frame(fs, pl, pr));
            }
        })
        .expect("spawn analyzer");
}
