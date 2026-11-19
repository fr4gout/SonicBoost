use crate::dsp::EQ_BANDS;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One parametric filter, using Equalizer APO's type codes:
/// PK (peaking), LSC / HSC (shelves), LP / HP (pass), NO (notch), BP (band pass).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct PeqFilter {
    pub enabled: bool,
    pub kind: String,
    pub freq: f32,
    pub gain: f32,
    pub q: f32,
}

impl Default for PeqFilter {
    fn default() -> Self {
        Self { enabled: true, kind: "PK".into(), freq: 1000.0, gain: 0.0, q: 1.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ParamEq {
    pub enabled: bool,
    pub preamp: f32, // dB
    pub filters: Vec<PeqFilter>,
}

impl Default for ParamEq {
    /// Ships with the "SYNC Signature" curve (Equalizer APO):
    /// Preamp -5 dB; PK 80 Hz -5 dB Q10; PK 1.8 kHz +3 dB Q1; PK 3.2 kHz +3.5 dB Q1.2; PK 8 kHz -2 dB Q1.
    fn default() -> Self {
        let pk = |freq, gain, q| PeqFilter { enabled: true, kind: "PK".into(), freq, gain, q };
        Self {
            enabled: false,
            preamp: -5.0,
            filters: vec![pk(80.0, -5.0, 10.0), pk(1800.0, 3.0, 1.0), pk(3200.0, 3.5, 1.2), pk(8000.0, -2.0, 1.0)],
        }
    }
}

/// A complete, named sound: everything that shapes the audio.
/// Applying one changes the sound for every app at once (it's system-wide).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub boost: f32,
    pub eq: [f32; EQ_BANDS],
    pub preset: String,
    pub peq: ParamEq,
    pub pitch_semitones: f32,
    pub pitch_cents: f32,
    pub width: f32,
    pub balance: f32,
    pub limiter: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self::capture("Profile", &Settings::default())
    }
}

impl Profile {
    pub fn capture(name: &str, s: &Settings) -> Self {
        Self {
            name: name.to_string(),
            boost: s.boost,
            eq: s.eq,
            preset: s.preset.clone(),
            peq: s.peq.clone(),
            pitch_semitones: s.pitch_semitones,
            pitch_cents: s.pitch_cents,
            width: s.width,
            balance: s.balance,
            limiter: s.limiter,
        }
    }

    pub fn apply_to(&self, s: &mut Settings) {
        s.boost = self.boost;
        s.eq = self.eq;
        s.preset = self.preset.clone();
        s.peq = self.peq.clone();
        s.pitch_semitones = self.pitch_semitones;
        s.pitch_cents = self.pitch_cents;
        s.width = self.width;
        s.balance = self.balance;
        s.limiter = self.limiter;
        s.active_profile = self.name.clone();
    }

    /// Built-in profile with the Equalizer APO "SYNC Signature" curve switched on.
    pub fn signature() -> Self {
        let mut s = Settings::default();
        s.boost = 1.5;
        s.peq.enabled = true;
        s.preset = "Flat".into();
        Self::capture("SYNC Signature", &s)
    }

    pub fn flat() -> Self {
        let mut s = Settings::default();
        s.boost = 1.0;
        s.peq.enabled = false;
        Self::capture("Flat (no processing)", &s)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomPreset {
    pub name: String,
    pub eq: [f32; EQ_BANDS],
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub boost: f32,          // 1.0 = 100 %
    pub eq: [f32; EQ_BANDS], // dB
    pub preset: String,
    pub pitch_semitones: f32,
    pub pitch_cents: f32,
    pub width: f32,   // 1.0 = normal stereo
    pub balance: f32, // -1..1
    pub custom_presets: Vec<CustomPreset>,
    pub peq: ParamEq,
    pub profiles: Vec<Profile>,
    pub active_profile: String,
    pub bypass: bool,
    pub limiter: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub mode: String,     // "compact" | "extended"
    pub viz_mode: String, // "bars" | "radial" | "wave" | "galaxy"
    pub theme: String, // "dark" | "light" (both monochrome)
    pub intensity: f32,
    pub start_minimized: bool,
    pub auto_update: bool, // check GitHub Releases on startup
    pub first_run: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            boost: 1.5,
            eq: [0.0; EQ_BANDS],
            preset: "Flat".into(),
            pitch_semitones: 0.0,
            pitch_cents: 0.0,
            width: 1.0,
            balance: 0.0,
            custom_presets: Vec::new(),
            peq: ParamEq::default(),
            profiles: Vec::new(),
            active_profile: String::new(),
            bypass: false,
            limiter: true,
            input_device: None,
            output_device: None,
            mode: "compact".into(),
            viz_mode: "radial".into(),
            theme: "dark".into(),
            intensity: 1.0,
            start_minimized: true,
            auto_update: true,
            first_run: true,
        }
    }
}

pub fn load(path: &Path) -> Settings {
    let mut s: Settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    // first run / upgrade from 1.0.x: provide the built-in profiles
    if s.profiles.is_empty() {
        s.profiles = vec![Profile::signature(), Profile::flat()];
    }
    s
}

pub fn save(path: &Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}
