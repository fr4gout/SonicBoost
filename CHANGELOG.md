# Changelog

All notable changes to SonicBoost are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- **Boost is now much louder.** The old limiter cancelled most of the boost on music that was already loud. A new look-ahead limiter plus a loudness compressor that scales with the boost add up to **+14 dB on streamed music and +20 dB on quiet audio** at 600 % (previously about +12.6 dB and +15 dB), with peaks still safely below full scale. Tests now check that a higher boost is never quieter.
- Preset row in the compact view can now be scrolled with the mouse wheel, by dragging, or with the new ‹ › arrow buttons. The selected preset scrolls into view automatically.
- Device drop-downs now use a themed glass menu instead of the Windows-style list.
- The level meters no longer overlap the Bars visualizer.

### Added
- **Parametric EQ (Equalizer APO style).** Next to the 10-band EQ there is now a **Parametric** tab:
  - A preamp, plus up to 20 filters: Peaking, Low/High shelf, Low/High pass, Notch and Band pass.
  - Each filter has its own on/off switch, frequency, gain and Q.
  - On the live response curve, drag a point to set frequency and gain, scroll on it to change Q, or double-click to add a filter.
  - **Import** pastes or opens any Equalizer APO `config.txt` or AutoEQ `ParametricEQ.txt`; **Export** copies the curve as Equalizer APO text.
- **Sound profiles (system-wide).** Save your whole sound under a name: boost, 10-band EQ, parametric EQ with preamp, pitch, stereo width and balance, and limiter. Switch profiles from the new **Profiles** menu in the title bar, from the tray's **Profiles** submenu, or with **Ctrl+Alt+1–9**. A profile changes the sound for every app at once.
- **Built-in profiles:**
  - **SYNC Signature:** Preamp −5 dB · PK 80 Hz −5 dB Q 10 · PK 1.8 kHz +3 dB Q 1 · PK 3.2 kHz +3.5 dB Q 1.2 · PK 8 kHz −2 dB Q 1.
  - **Flat (no processing).**
- **Maximize and full screen.** The window now has a maximize button, a double-click on the title bar maximizes it, and **F11** toggles full screen. Window edges are resizable, and the extended layout scales up on large screens (album art, knob and EQ curve grow). Maximizing from compact view switches to the extended layout.
- **In-app updates.** SonicBoost checks GitHub Releases on startup, or on request from ⚙ Settings → Updates. One click downloads the new installer, installs it and restarts the app, keeping your settings. Only installers from this repository's releases are accepted.
- **"Not boosting yet" warning.** Shown when Windows is playing straight to your speakers instead of CABLE Input, with a button that opens Sound settings.
- **"No sound is reaching SonicBoost" warning.** Shown when music is playing but the engine hears silence.
- A setup tip to keep your speakers' own volume at 100 %.

## [1.0.0] - 2026-11-20

First public release.

### Audio
- System-wide boost up to 600 % (+15.6 dB), with a stereo-linked peak limiter and soft clipping.
- 10-band graphic equalizer (31 Hz – 16 kHz, ±12 dB) with a live response curve.
- 12 built-in presets, plus custom presets you can save and delete.
- Real-time pitch shifter: ±12 semitones and ±100 cents, with speed preserved.
- Stereo width (mono – 200 %) and left/right balance.
- Click-free glides on every parameter change.
- Automatic resampling between devices running at different sample rates.

### Interface
- Monochrome liquid-glass design in dark and light themes.
- Compact (400 × 740) and extended (1180 × 760) layouts with an animated switch.
- Now Playing panel: album art, title, artist, album, source app, progress and playback controls.
- Visualizers: Bars, Radial, Wave and Galaxy, plus a reactive ring around the boost knob.
- Spring-animated knob, sliders and EQ.

### System
- Starts with Windows and runs from the system tray.
- Global hotkeys for boost, bypass and media control.
- Reconnects automatically when an audio device disappears.
- The analyzer pauses while the window is hidden.
- NSIS (`-setup.exe`) and MSI installers.

[Unreleased]: https://github.com/fr4gout/SonicBoost/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/fr4gout/SonicBoost/releases/tag/v1.0.0
