# Changelog

All notable changes to SonicBoost are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [1.0.5] - 2026-11-20

### Fixed
- **Hearing yourself with the monitor off.** The booster can no longer record the cable the mic sends your voice into, even if it was picked by hand, and switches away from it the moment the mic starts. If Windows itself is playing your mic or a cable to the speakers (*Listen to this device*), the Mic view now says so and opens the setting for you.
- **Smoother, faster window.** Commands that open audio devices, list devices or save settings now run off the window's UI thread, so clicks answer at once and opening, closing and dragging the window no longer stall. Meter and spectrum updates pause while the window is moved, so it no longer stutters or freezes during a drag. The booster stops drawing its hidden panels while the Mic view is open.
- **Mic monitor really turns off.** A hard switch silences the monitor the moment you turn it (or the mic) off. *Windows default* monitoring now always uses your real speakers or headphones, never a virtual cable, which used to send your voice through the booster.
- With two virtual cables installed, the booster now listens to the cable that matches the Windows default output and never picks the mic's 16-channel cable.
- Compact view: the mic preset and voice-mode rows scroll with the mouse wheel or by dragging, the selected item scrolls into view, and the title bar buttons line up evenly.
- Every scrollbar now uses the app's theme instead of the Windows style.

## [1.0.2] - 2026-11-20

### Added: SonicBoost Mic
- **Mic view** (🎙 button in the title bar): a real-time microphone processor that sends your voice to a second virtual cable, so Discord, OBS, Zoom and games hear it as a normal mic. It's off until you turn it on, and every stage is optional.
- **Voice chain:** RNNoise noise suppression, mic boost (+24 dB), noise gate with a live level marker, voice EQ (Clean, Broadcast, Clarity, Bass, Warm), compressor, de-esser, limiter and output level.
- **Mic presets:** Discord / Gaming, Streaming, Podcast, Meetings / Zoom, Noisy room, Singing, Whisper / ASMR and Raw, plus your own (+ Save).
- **13 voice modes:** Natural, Broadcast, Deep, High, Chipmunk, Robot, Radio, Telephone, Megaphone, Monster, Alien, Cave and Custom (your own pitch and room). <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>1–9</kbd>.
- **Soundboard:** up to 16 `.wav` clips mixed into the mic, 8 at once, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F1–F8</kbd>, with its own level.
- **Control:** mute (<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>M</kbd>), push-to-talk or push-to-mute (hold <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>T</kbd>), monitor on any output, a 5-second mic test, meters with Gate / Voice / PTT lights and a waveform.
- The mic can never be routed into the booster's own cable, and the watchdog reopens it after a device change.

### Fixed
- **The Windows volume slider works again** while sound goes through the cable. Virtual cables ignore their own volume, so SonicBoost now reads the slider, the volume keys and mute, and applies them as the very last step: output = boosted sound × Windows volume. That means Windows 100 % × boost 500 % = 500 %, and every slider step scales it exactly. The boost panel shows both live, e.g. "Windows 40 % · Output 200 %". You can switch this off in Settings.
- **Equalizer APO imports:**
  - `GraphicEQ:` lines now work. They're fitted onto the 10-band EQ, accurate to within 0.06 dB of the APO curve.
  - Routing scripts (`Copy` / `Channel` / `Delay`) no longer add every `Preamp` line together into −30 dB. Their tone curve is imported, and their echo/reverb is recreated with the new Ambience effect.
  - Very low preamps are flagged, and a new **Auto** button sets just enough headroom.
- `release.ps1` now reads files as UTF-8 (Windows PowerShell 5.1 garbled characters like ‹ › in the changelog).
- The compact settings sheet is now fully opaque.

### Added
- **Import many .txt files as profiles** in one go (Profiles menu → *Import .txt files as profiles…*). Each Equalizer APO or AutoEQ file becomes a profile named after the file.
- **Built-in profile library:** Bass boost without the mud, Desk speakers & monitors, Bangla Rock, Hip-hop & rap, Movies & dialogue, Night listening, Rock & metal, Speaker, and Ohi Room (reverb + echo). Existing installs get any that are missing.
- **Ambience effect (echo & room):** a ping-pong multi-tap echo with softened reflections, with Amount and Room size controls. It is saved in profiles.
- **Themes:** **Auto** follows Windows light/dark mode. **Glass** makes the window truly see-through, with your desktop blurred behind it (Windows acrylic).
- **Graphics quality:** *Fast* (no blur effects, 30 fps), *Balanced* (default) and *Ultra* (liquid refraction, full frame rate).
- Mic guide: [docs/MIC_PLAN.md](docs/MIC_PLAN.md).

### Performance
- Per-frame style recalculation is down from about 98 to 26 per second, and style work from about 177 ms to under 30 ms per second.
- Layouts are halved: the visualizers no longer read their size every frame.
- The light field behind the glass redraws at 15 fps in Balanced mode, so the frosted panels stop re-blurring 60 times a second.
- The animation loop runs at 30 fps while the window isn't focused.

## [1.0.1] - 2026-11-20

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

[Unreleased]: https://github.com/fr4gout/SonicBoost/compare/v1.0.5...HEAD
[1.0.5]: https://github.com/fr4gout/SonicBoost/compare/v1.0.4...v1.0.5
[1.0.4]: https://github.com/fr4gout/SonicBoost/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/fr4gout/SonicBoost/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/fr4gout/SonicBoost/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/fr4gout/SonicBoost/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/fr4gout/SonicBoost/releases/tag/v1.0.0
