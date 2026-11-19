use crate::dsp::EQ_BANDS;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    pub bypass: bool,
    pub limiter: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub mode: String,     // "compact" | "extended"
    pub viz_mode: String, // "bars" | "radial" | "wave" | "galaxy"
    pub theme: String, // "dark" | "light" (both monochrome)
    pub intensity: f32,
    pub start_minimized: bool,
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
            bypass: false,
            limiter: true,
            input_device: None,
            output_device: None,
            mode: "compact".into(),
            viz_mode: "radial".into(),
            theme: "dark".into(),
            intensity: 1.0,
            start_minimized: true,
            first_run: true,
        }
    }
}

pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, s: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}
