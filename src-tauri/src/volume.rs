//! Makes the Windows volume slider work while the output is a virtual cable.
//!
//! VB-Cable (and most virtual cables) ignore their own endpoint volume: the
//! taskbar slider moves, but the sound reaching SonicBoost doesn't change.
//! This thread reads the default output's volume and mute (≈16 times a
//! second) and SonicBoost applies them as the very last gain stage, so the
//! slider, the keyboard volume keys and mute behave exactly as normal.
//! When Windows plays straight to real speakers, the gain stays at 1.0.

use crate::audio::{is_cable, Shared};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(serde::Serialize, Clone, PartialEq)]
pub struct WinVolume {
    pub percent: u32,  // Windows slider position
    pub muted: bool,
    pub active: bool,  // true while SonicBoost applies it (output is a virtual cable)
}

pub fn spawn(app: AppHandle, shared: Arc<Shared>) {
    std::thread::Builder::new()
        .name("volume-sync".into())
        .spawn(move || run(app, shared))
        .expect("spawn volume thread");
}

fn default_output_is_virtual() -> bool {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .default_output_device()
        .and_then(|d| d.name().ok())
        .map(|n| is_cable(&n))
        .unwrap_or(false)
}

fn publish(shared: &Shared, gain: f32) {
    if (shared.params.master.get() - gain).abs() > 1e-4 {
        shared.params.master.set(gain);
        shared.params.bump();
    }
}

#[cfg(windows)]
fn run(app: AppHandle, shared: Arc<Shared>) {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let open = || -> windows::core::Result<IAudioEndpointVolume> {
        unsafe {
            let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
            dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
        }
    };
    let mut vol: Option<IAudioEndpointVolume> = None;
    let mut virtual_out = false;
    let mut last_check = Instant::now() - Duration::from_secs(10);
    let mut last_sent: Option<WinVolume> = None;
    loop {
        std::thread::sleep(Duration::from_millis(60));
        // the default device can change at any time: re-resolve every 1.5 s
        if last_check.elapsed() > Duration::from_millis(1500) {
            last_check = Instant::now();
            virtual_out = default_output_is_virtual();
            vol = open().ok();
        }
        let follow = shared.follow_volume.load(Ordering::Relaxed);
        let active = follow && virtual_out && vol.is_some();
        let (percent, muted, db) = match &vol {
            Some(v) => unsafe {
                (
                    (v.GetMasterVolumeLevelScalar().unwrap_or(1.0) * 100.0).round() as u32,
                    v.GetMute().map(|b| b.as_bool()).unwrap_or(false),
                    v.GetMasterVolumeLevel().unwrap_or(0.0).clamp(-96.0, 0.0),
                )
            },
            None => (100, false, 0.0),
        };
        // dB follows the slider's audio taper, exactly like Windows itself
        let gain = if !active { 1.0 } else if muted || percent == 0 { 0.0 } else { 10f32.powf(db / 20.0) };
        publish(&shared, gain);
        let now = WinVolume { percent, muted, active };
        if last_sent.as_ref() != Some(&now) {
            let _ = app.emit("windows-volume", &now);
            last_sent = Some(now);
        }
    }
}

#[cfg(not(windows))]
fn run(app: AppHandle, shared: Arc<Shared>) {
    let _ = app.emit("windows-volume", WinVolume { percent: 100, muted: false, active: false });
    loop {
        std::thread::sleep(Duration::from_secs(1));
        publish(&shared, 1.0);
        let _ = default_output_is_virtual();
    }
}
