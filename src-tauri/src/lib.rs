mod analyzer;
mod audio;
mod dsp;
mod media;
mod ring;
mod settings;

use audio::{Engine, EngineStatus, Shared};
use media::{Media, MediaCmd};
use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub const COMPACT: (f64, f64) = (400.0, 740.0);
pub const EXTENDED: (f64, f64) = (1180.0, 760.0);

struct AppState {
    shared: Arc<Shared>,
    engine: Mutex<Engine>,
    media: Media,
    settings: Mutex<Settings>,
    status: Mutex<EngineStatus>,
    path: PathBuf,
}

impl AppState {
    fn apply(&self, s: &Settings) {
        let p = &self.shared.params;
        p.boost.set(s.boost.clamp(0.0, dsp::BOOST_MAX));
        for (i, g) in s.eq.iter().enumerate() {
            p.eq[i].set(g.clamp(-dsp::EQ_RANGE_DB, dsp::EQ_RANGE_DB));
        }
        p.pitch.set((s.pitch_semitones + s.pitch_cents / 100.0).clamp(-12.0, 12.0));
        p.width.set(s.width.clamp(0.0, 2.0));
        p.balance.set(s.balance.clamp(-1.0, 1.0));
        p.bypass.store(s.bypass, Ordering::Relaxed);
        p.limiter.store(s.limiter, Ordering::Relaxed);
        p.bump();
    }

    fn update<F: FnOnce(&mut Settings)>(&self, f: F) {
        let mut s = self.settings.lock().unwrap();
        f(&mut s);
        self.apply(&s);
    }

    fn save(&self) -> Result<(), String> {
        settings::save(&self.path, &self.settings.lock().unwrap())
    }

    fn restart(&self) -> EngineStatus {
        let (i, o) = {
            let s = self.settings.lock().unwrap();
            (s.input_device.clone(), s.output_device.clone())
        };
        let st = self.engine.lock().unwrap().start(i, o);
        *self.status.lock().unwrap() = st.clone();
        st
    }
}

#[derive(Serialize)]
struct FullState {
    settings: Settings,
    status: EngineStatus,
    autostart: bool,
    devices: audio::DeviceList,
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_state(app: AppHandle, state: State<AppState>) -> FullState {
    FullState {
        settings: state.settings.lock().unwrap().clone(),
        status: state.status.lock().unwrap().clone(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        devices: audio::list_devices(),
    }
}

#[tauri::command]
fn list_devices() -> audio::DeviceList {
    audio::list_devices()
}

#[tauri::command]
fn set_boost(state: State<AppState>, value: f32) {
    state.update(|s| s.boost = value);
}

#[tauri::command]
fn set_eq_band(state: State<AppState>, index: usize, gain: f32) {
    if index < dsp::EQ_BANDS {
        state.update(|s| {
            s.eq[index] = gain;
            s.preset = "Custom".into();
        });
    }
}

#[tauri::command]
fn set_eq(state: State<AppState>, gains: Vec<f32>, preset: String) {
    state.update(|s| {
        for (i, g) in gains.iter().take(dsp::EQ_BANDS).enumerate() {
            s.eq[i] = *g;
        }
        s.preset = preset;
    });
}

#[tauri::command]
fn set_pitch(state: State<AppState>, semitones: f32, cents: f32) {
    state.update(|s| {
        s.pitch_semitones = semitones.round().clamp(-12.0, 12.0);
        s.pitch_cents = cents.round().clamp(-100.0, 100.0);
    });
}

#[tauri::command]
fn set_stereo(state: State<AppState>, width: f32, balance: f32) {
    state.update(|s| {
        s.width = width;
        s.balance = balance;
    });
}

#[tauri::command]
fn set_custom_presets(state: State<AppState>, presets: Vec<settings::CustomPreset>) -> Result<(), String> {
    state.update(|s| s.custom_presets = presets);
    state.save()
}

/// The UI calls this when it becomes hidden/visible so the analyzer can sleep.
#[tauri::command]
fn set_viz_active(state: State<AppState>, enabled: bool) {
    state.shared.viz_active.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
fn set_bypass(app: AppHandle, state: State<AppState>, enabled: bool) {
    state.update(|s| s.bypass = enabled);
    let _ = app.emit("bypass-changed", enabled);
}

#[tauri::command]
fn set_limiter(state: State<AppState>, enabled: bool) {
    state.update(|s| s.limiter = enabled);
}

#[tauri::command]
fn set_ui(state: State<AppState>, viz: Option<String>, theme: Option<String>, intensity: Option<f32>) {
    state.update(|s| {
        if let Some(v) = viz {
            s.viz_mode = v;
        }
        if let Some(t) = theme {
            s.theme = t;
        }
        if let Some(i) = intensity {
            s.intensity = i;
        }
    });
}

#[tauri::command]
fn set_mode(app: AppHandle, state: State<AppState>, mode: String) -> Result<(), String> {
    let w = app.get_webview_window("main").ok_or("window missing")?;
    let (wd, ht) = if mode == "extended" { EXTENDED } else { COMPACT };
    w.set_size(LogicalSize::new(wd, ht)).map_err(|e| e.to_string())?;
    let _ = w.center();
    state.update(|s| s.mode = mode);
    state.save()
}

#[tauri::command]
fn set_devices(state: State<AppState>, input: Option<String>, output: Option<String>) -> EngineStatus {
    state.update(|s| {
        s.input_device = input;
        s.output_device = output;
    });
    let st = state.restart();
    let _ = state.save();
    st
}

#[tauri::command]
fn start_engine(state: State<AppState>) -> EngineStatus {
    state.restart()
}

#[tauri::command]
fn stop_engine(state: State<AppState>) -> EngineStatus {
    state.engine.lock().unwrap().stop();
    let st = EngineStatus::default();
    *state.status.lock().unwrap() = st.clone();
    st
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    let res = if enabled { al.enable() } else { al.disable() };
    res.map_err(|e| e.to_string())?;
    al.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_start_minimized(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.update(|s| s.start_minimized = enabled);
    state.save()
}

#[tauri::command]
fn save_settings(state: State<AppState>) -> Result<(), String> {
    state.save()
}

#[tauri::command]
fn media_control(state: State<AppState>, action: String) {
    let cmd = match action.as_str() {
        "next" => MediaCmd::Next,
        "previous" => MediaCmd::Previous,
        "refresh" => MediaCmd::Refresh,
        _ => MediaCmd::PlayPause,
    };
    state.media.send(cmd);
}

#[tauri::command]
fn open_link(target: String) -> Result<(), String> {
    let url = match target.as_str() {
        "sound-settings" => "ms-settings:sound",
        "sound-panel" => "mmsys.cpl",
        "vbcable" => "https://vb-audio.com/Cable/",
        _ => return Err("unknown link".into()),
    };
    #[cfg(windows)]
    {
        let prog = if url.ends_with(".cpl") { "control" } else { "explorer" };
        std::process::Command::new(prog).arg(url).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    let _ = url;
    Ok(())
}

#[tauri::command]
fn win_minimize(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

#[tauri::command]
fn win_hide(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

// ------------------------------------------------------------------- app

fn toggle_bypass(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut on = false;
    st.update(|s| {
        s.bypass = !s.bypass;
        on = s.bypass;
    });
    let _ = app.emit("bypass-changed", on);
}

fn nudge_boost(app: &AppHandle, delta: f32) {
    let st = app.state::<AppState>();
    let mut v = 0.0;
    st.update(|s| {
        s.boost = ((s.boost + delta) * 10.0).round() / 10.0;
        s.boost = s.boost.clamp(0.0, dsp::BOOST_MAX);
        v = s.boost;
    });
    let _ = app.emit("boost-changed", v);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("SonicBoost · {:.0}%", v * 100.0)));
    }
}

fn hotkey(code: Code) -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), code)
}

/// Global hotkeys (work even when SonicBoost is hidden in the tray):
///   Ctrl+Alt+Up/Down  boost ±10 %      Ctrl+Alt+B  boost on/off
///   Ctrl+Alt+P        play / pause     Ctrl+Alt+Left/Right  previous / next track
fn register_hotkeys(app: &tauri::App) -> tauri::Result<()> {
    app.handle().plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, sc, event| {
                if !matches!(event.state(), ShortcutState::Pressed) {
                    return;
                }
                if sc == &hotkey(Code::ArrowUp) {
                    nudge_boost(app, 0.1);
                } else if sc == &hotkey(Code::ArrowDown) {
                    nudge_boost(app, -0.1);
                } else if sc == &hotkey(Code::KeyB) {
                    toggle_bypass(app);
                } else if sc == &hotkey(Code::KeyP) {
                    app.state::<AppState>().media.send(MediaCmd::PlayPause);
                } else if sc == &hotkey(Code::ArrowRight) {
                    app.state::<AppState>().media.send(MediaCmd::Next);
                } else if sc == &hotkey(Code::ArrowLeft) {
                    app.state::<AppState>().media.send(MediaCmd::Previous);
                }
            })
            .build(),
    )?;
    for code in [Code::ArrowUp, Code::ArrowDown, Code::KeyB, Code::KeyP, Code::ArrowRight, Code::ArrowLeft] {
        // Another app may already own a combo; skip it instead of failing.
        let _ = app.global_shortcut().register(hotkey(code));
    }
    Ok(())
}

/// Restarts the engine automatically if a device disappears (headphones
/// unplugged, Bluetooth disconnect, driver reset).
fn spawn_watchdog(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let st = app.state::<AppState>();
        if st.shared.device_error.swap(false, Ordering::AcqRel) {
            std::thread::sleep(std::time::Duration::from_millis(800));
            let mut status = st.restart();
            if status.error.is_some() {
                // The chosen output is gone: fall back to the Windows default.
                st.update(|s| s.output_device = None);
                status = st.restart();
            }
            let _ = app.emit("engine-status", &status);
        }
    });
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            let mut cfg = settings::load(&path);

            // First launch: register in Windows startup (user can turn it off).
            if cfg.first_run {
                let _ = app.autolaunch().enable();
                cfg.first_run = false;
                let _ = settings::save(&path, &cfg);
            }

            let shared = Shared::new();
            let engine = Engine::spawn(shared.clone());
            audio::spawn_analyzer(app.handle().clone(), shared.clone());
            let media = Media::spawn(app.handle().clone());

            let state = AppState {
                shared,
                engine: Mutex::new(engine),
                media,
                settings: Mutex::new(cfg.clone()),
                status: Mutex::new(EngineStatus::default()),
                path,
            };
            state.apply(&cfg);
            state.restart();
            app.manage(state);
            spawn_watchdog(app.handle().clone());
            if let Err(e) = register_hotkeys(app) {
                eprintln!("hotkeys unavailable: {e}");
            }

            // Window: size by mode; stay hidden in the tray when auto-started.
            let win = app.get_webview_window("main").expect("main window");
            let (wd, ht) = if cfg.mode == "extended" { EXTENDED } else { COMPACT };
            let _ = win.set_size(LogicalSize::new(wd, ht));
            let _ = win.center();
            let autostarted = std::env::args().any(|a| a == "--minimized");
            if !(autostarted && cfg.start_minimized) {
                let _ = win.show();
            }

            // System tray.
            let show = MenuItem::with_id(app, "show", "Open SonicBoost", true, None::<&str>)?;
            let bypass = MenuItem::with_id(app, "bypass", "Toggle Boost On/Off", true, None::<&str>)?;
            let play = MenuItem::with_id(app, "playpause", "Play / Pause music", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &bypass, &play, &sep, &quit])?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("SonicBoost")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show_main(app),
                    "bypass" => toggle_bypass(app),
                    "playpause" => app.state::<AppState>().media.send(MediaCmd::PlayPause),
                    "quit" => {
                        let st = app.state::<AppState>();
                        let _ = st.save();
                        st.engine.lock().unwrap().stop();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, e| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the booster running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            list_devices,
            set_boost,
            set_eq_band,
            set_eq,
            set_pitch,
            set_bypass,
            set_stereo,
            set_custom_presets,
            set_viz_active,
            set_limiter,
            set_ui,
            set_mode,
            set_devices,
            start_engine,
            stop_engine,
            set_autostart,
            set_start_minimized,
            save_settings,
            media_control,
            open_link,
            win_minimize,
            win_hide
        ])
        .run(tauri::generate_context!())
        .expect("error while running SonicBoost");
}
