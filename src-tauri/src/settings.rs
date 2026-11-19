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

// ------------------------------------------------------------------ mic --

/// The voice chain, as stored in mic presets. Every stage has its own switch.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct MicConfig {
    pub ns: bool, // noise suppression (RNNoise)
    pub boost_db: f32,
    pub gate_on: bool,
    pub gate_db: f32,
    pub eq_preset: u8, // 0 off, 1 clean, 2 broadcast, 3 clarity, 4 bass voice, 5 warm
    pub comp_on: bool,
    pub comp_amount: f32,
    pub deess_on: bool,
    pub deess_amount: f32,
    pub mode: String, // voice mode code
    pub custom_pitch: f32,
    pub custom_room: f32,
    pub limiter_on: bool,
    pub out_db: f32,
}

impl Default for MicConfig {
    fn default() -> Self {
        Self {
            ns: true,
            boost_db: 0.0,
            gate_on: true,
            gate_db: -45.0,
            eq_preset: 1,
            comp_on: true,
            comp_amount: 0.5,
            deess_on: false,
            deess_amount: 0.5,
            mode: "natural".into(),
            custom_pitch: 0.0,
            custom_room: 0.0,
            limiter_on: true,
            out_db: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MicPreset {
    pub name: String,
    pub cfg: MicConfig,
}

/// Built-in mic presets: a complete chain for each use.
pub fn builtin_mic_presets() -> Vec<MicPreset> {
    let p = |name: &str, f: &dyn Fn(&mut MicConfig)| {
        let mut c = MicConfig::default();
        f(&mut c);
        MicPreset { name: name.into(), cfg: c }
    };
    vec![
        p("Discord / Gaming", &|c| {
            c.gate_db = -45.0;
            c.eq_preset = 1;
            c.comp_amount = 0.5;
        }),
        p("Streaming", &|c| {
            c.boost_db = 3.0;
            c.gate_db = -50.0;
            c.eq_preset = 2;
            c.comp_amount = 0.65;
            c.deess_on = true;
            c.deess_amount = 0.5;
        }),
        p("Podcast", &|c| {
            c.gate_db = -52.0;
            c.eq_preset = 2;
            c.comp_amount = 0.75;
            c.deess_on = true;
            c.deess_amount = 0.6;
        }),
        p("Meetings / Zoom", &|c| {
            c.gate_db = -48.0;
            c.eq_preset = 3;
            c.comp_amount = 0.4;
        }),
        p("Noisy room", &|c| {
            c.boost_db = 2.0;
            c.gate_db = -38.0;
            c.eq_preset = 1;
            c.comp_amount = 0.5;
        }),
        p("Singing", &|c| {
            c.ns = false;
            c.gate_on = false;
            c.eq_preset = 5;
            c.comp_amount = 0.35;
            c.deess_on = true;
            c.deess_amount = 0.4;
        }),
        p("Whisper / ASMR", &|c| {
            c.boost_db = 12.0;
            c.gate_db = -60.0;
            c.eq_preset = 4;
            c.comp_amount = 0.8;
        }),
        p("Raw (no processing)", &|c| {
            c.ns = false;
            c.gate_on = false;
            c.eq_preset = 0;
            c.comp_on = false;
            c.deess_on = false;
        }),
    ]
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct SoundClip {
    pub name: String,
    pub file: String, // stored copy in the app's "soundboard" folder
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct MicSettings {
    pub enabled: bool,
    pub input: Option<String>,
    pub output: Option<String>,
    pub monitor_on: bool,
    pub monitor_device: Option<String>,
    pub monitor_volume: f32,
    pub ptt: String, // "off" | "ptt" (hold to talk) | "ptm" (hold to mute)
    pub muted: bool,
    pub cfg: MicConfig,
    pub preset: String,
    pub presets: Vec<MicPreset>, // the user's own
    pub soundboard: Vec<SoundClip>,
    pub sb_gain: f32,
}

impl Default for MicSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            input: None,
            output: None,
            monitor_on: false,
            monitor_device: None,
            monitor_volume: 0.8,
            ptt: "off".into(),
            muted: false,
            cfg: MicConfig::default(),
            preset: "Discord / Gaming".into(),
            presets: Vec::new(),
            soundboard: Vec::new(),
            sb_gain: 0.8,
        }
    }
}

/// Echo / room effect (the APO "reverb + echo" scripts).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct AmbienceCfg {
    pub enabled: bool,
    pub mix: f32,  // 0..1
    pub size: f32, // 0.5..2
}

impl Default for AmbienceCfg {
    fn default() -> Self {
        Self { enabled: false, mix: 0.45, size: 1.0 }
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
    pub ambience: AmbienceCfg,
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
            ambience: s.ambience.clone(),
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
        s.ambience = self.ambience.clone();
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

    /// Built-in library (from the user's Equalizer APO configs).
    pub fn library() -> Vec<Profile> {
        fn f(kind: &str, freq: f32, gain: f32, q: f32) -> PeqFilter {
            PeqFilter { enabled: true, kind: kind.into(), freq, gain, q }
        }
        fn make(name: &str, preamp: f32, filters: Vec<PeqFilter>, eq: [f32; EQ_BANDS], amb: Option<(f32, f32)>) -> Profile {
            let mut s = Settings::default();
            s.boost = 1.5;
            s.eq = eq;
            s.preset = if eq.iter().any(|g| *g != 0.0) { "Custom".into() } else { "Flat".into() };
            s.peq = ParamEq { enabled: true, preamp, filters };
            if let Some((mix, size)) = amb {
                s.ambience = AmbienceCfg { enabled: true, mix, size };
            }
            Profile::capture(name, &s)
        }
        let flat = [0.0; EQ_BANDS];
        vec![
            make("Bass boost without the mud", 0.0, vec![f("PK", 285.78, -6.0, 1.6), f("PK", 60.0, -2.0, 2.0)], flat, None),
            make("Desk speakers & monitors", -0.3, vec![f("PK", 110.16, -3.0, 1.6), f("PK", 60.0, -2.0, 2.0)], flat, None),
            // GraphicEQ fitted onto the 10 bands (overlap-corrected, within 0.06 dB of the APO curve)
            make("Bangla Rock", 0.3, vec![], [0.5, 3.8, 0.5, -1.9, 0.1, 0.9, 2.0, 2.6, 1.5, 1.9], None),
            make("Hip-hop & rap", -5.5, vec![f("PK", 45.0, 5.0, 0.9), f("PK", 250.0, -2.5, 1.2), f("PK", 2500.0, 2.0, 1.0)], flat, None),
            make(
                "Movies & dialogue",
                -5.0,
                vec![f("PK", 80.0, -5.0, 10.0), f("PK", 1800.0, 3.0, 1.0), f("PK", 3200.0, 3.5, 1.2), f("PK", 8000.0, -2.0, 1.0)],
                flat,
                None,
            ),
            make("Night listening", 0.0, vec![f("LSC", 100.0, 3.5, 0.7), f("PK", 3000.0, 2.0, 1.0), f("HSC", 10000.0, 2.5, 0.7)], flat, None),
            // original preamp was -15.3 dB, which only makes it quieter; -1.5 dB covers its +1.5 dB boosts
            make(
                "Rock & metal",
                -1.5,
                vec![f("LSC", 80.0, 1.5, 0.7), f("PK", 400.0, -2.0, 1.2), f("PK", 4200.0, -3.0, 2.0), f("HSC", 9000.0, 1.5, 0.7)],
                flat,
                None,
            ),
            make("Speaker", -1.0, vec![f("PK", 120.0, -3.0, 1.6), f("PK", 60.0, -2.0, 2.0)], flat, None),
            // APO reverb/echo script: tone curve "1 -6; 160 0; 2500 1; 16000 -6" + ping-pong echo
            make(
                "Ohi Room (reverb + echo)",
                -1.0,
                vec![],
                [-2.1, -0.5, -0.2, 0.1, 0.2, 0.4, 1.0, -0.5, -2.4, -6.2],
                Some((0.55, 1.0)),
            ),
        ]
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
    pub ambience: AmbienceCfg,
    pub profiles: Vec<Profile>,
    pub profiles_version: u32,
    pub quality: String, // "low" | "balanced" | "high" graphics
    pub follow_windows_volume: bool,
    pub mic: MicSettings,
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
            ambience: AmbienceCfg::default(),
            profiles: Vec::new(),
            profiles_version: 0,
            quality: "balanced".into(),
            follow_windows_volume: true,
            mic: MicSettings::default(),
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
    // first run: the two basic profiles
    if s.profiles.is_empty() {
        s.profiles = vec![Profile::signature(), Profile::flat()];
    }
    // library v2: add any built-in profile the user doesn't have yet (by name)
    if s.profiles_version < 2 {
        for p in Profile::library() {
            if s.profiles.len() < 30 && !s.profiles.iter().any(|x| x.name.eq_ignore_ascii_case(&p.name)) {
                s.profiles.push(p);
            }
        }
        s.profiles_version = 2;
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
