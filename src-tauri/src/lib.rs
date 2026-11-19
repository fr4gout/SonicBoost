mod analyzer;
mod audio;
mod dsp;
mod media;
mod ring;
mod settings;
mod updater;

use audio::{Engine, EngineStatus, Shared};
use media::{Media, MediaCmd};
use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub const COMPACT: (f64, f64) = (400.0, 740.0);
pub const EXTENDED: (f64, f64) = (1180.0, 760.0);
pub const COMPACT_MIN: (f64, f64) = (380.0, 680.0);
pub const EXTENDED_MIN: (f64, f64) = (1060.0, 700.0);

/// Window size + minimum size for a layout. When the window is maximized or
/// full screen it keeps covering the screen; only the minimum size changes.
fn size_window(w: &tauri::WebviewWindow, mode: &str, center: bool) {
    let ext = mode == "extended";
    let (mw, mh) = if ext { EXTENDED_MIN } else { COMPACT_MIN };
    let _ = w.set_min_size(Some(LogicalSize::new(mw, mh)));
    let big = w.is_maximized().unwrap_or(false) || w.is_fullscreen().unwrap_or(false);
    if !big {
        let (wd, ht) = if ext { EXTENDED } else { COMPACT };
        let _ = w.set_size(LogicalSize::new(wd, ht));
        if center {
            let _ = w.center();
        }
    }
}

#[derive(Serialize)]
struct WinState {
    maximized: bool,
    fullscreen: bool,
}

fn win_state_of(w: &tauri::WebviewWindow) -> WinState {
    WinState { maximized: w.is_maximized().unwrap_or(false), fullscreen: w.is_fullscreen().unwrap_or(false) }
}

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
        if let Ok(mut q) = self.shared.peq.lock() {
            *q = audio::PeqState::from_settings(&s.peq);
        }
        self.shared.peq_version.fetch_add(1, Ordering::Release);
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
    version: String,
    settings: Settings,
    status: EngineStatus,
    autostart: bool,
    devices: audio::DeviceList,
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_state(app: AppHandle, state: State<AppState>) -> FullState {
    FullState {
        version: app.package_info().version.to_string(),
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
    mark_modified(&state);
}

#[tauri::command]
fn set_eq_band(state: State<AppState>, index: usize, gain: f32) {
    if index < dsp::EQ_BANDS {
        state.update(|s| {
            s.eq[index] = gain;
            s.preset = "Custom".into();
        });
        mark_modified(&state);
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
    mark_modified(&state);
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

/// Parametric EQ (Equalizer APO style): preamp + up to 20 filters.
#[tauri::command]
fn set_peq(state: State<AppState>, peq: settings::ParamEq) {
    state.update(|s| {
        let mut p = peq;
        p.filters.truncate(dsp::PEQ_MAX);
        s.peq = p;
    });
    mark_modified(&state);
}

// ------------------------------------------------------------ profiles

#[derive(Serialize)]
struct ProfilesDto {
    profiles: Vec<String>,
    active: String,
}

fn profiles_dto(s: &Settings) -> ProfilesDto {
    ProfilesDto { profiles: s.profiles.iter().map(|p| p.name.clone()).collect(), active: s.active_profile.clone() }
}

/// Applies a profile system-wide and tells the UI to reload its controls.
fn apply_profile_at(app: &AppHandle, index: usize) -> Result<String, String> {
    let st = app.state::<AppState>();
    let name = {
        let mut s = st.settings.lock().unwrap();
        let p = s.profiles.get(index).cloned().ok_or("No profile in that slot")?;
        p.apply_to(&mut s);
        st.apply(&s);
        p.name
    };
    let _ = st.save();
    refresh_tray(app);
    let _ = app.emit("state-reload", &name);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(format!("SonicBoost · {name}")));
    }
    Ok(name)
}

#[tauri::command]
fn list_profiles(state: State<AppState>) -> ProfilesDto {
    profiles_dto(&state.settings.lock().unwrap())
}

/// Saves the current sound under `name` (overwrites a profile with that name).
#[tauri::command]
fn save_profile(app: AppHandle, state: State<AppState>, name: String) -> Result<ProfilesDto, String> {
    let name = name.trim().chars().take(32).collect::<String>();
    if name.is_empty() {
        return Err("Give the profile a name".into());
    }
    let dto = {
        let mut s = state.settings.lock().unwrap();
        let p = settings::Profile::capture(&name, &s);
        match s.profiles.iter().position(|x| x.name.eq_ignore_ascii_case(&name)) {
            Some(i) => s.profiles[i] = p,
            None => {
                if s.profiles.len() >= 30 {
                    return Err("You can keep up to 30 profiles".into());
                }
                s.profiles.push(p)
            }
        }
        s.active_profile = name;
        profiles_dto(&s)
    };
    state.save()?;
    refresh_tray(&app);
    Ok(dto)
}

#[tauri::command]
fn apply_profile(app: AppHandle, state: State<AppState>, name: String) -> Result<String, String> {
    let idx = state.settings.lock().unwrap().profiles.iter().position(|p| p.name == name).ok_or("Profile not found")?;
    apply_profile_at(&app, idx)
}

#[tauri::command]
fn delete_profile(app: AppHandle, state: State<AppState>, name: String) -> Result<ProfilesDto, String> {
    let dto = {
        let mut s = state.settings.lock().unwrap();
        s.profiles.retain(|p| p.name != name);
        if s.active_profile == name {
            s.active_profile.clear();
        }
        profiles_dto(&s)
    };
    state.save()?;
    refresh_tray(&app);
    Ok(dto)
}

/// Any manual change means the sound no longer matches the active profile.
fn mark_modified(state: &AppState) {
    state.settings.lock().unwrap().active_profile.clear();
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
    if mode == "compact" {
        // compact is a small floating panel: leave maximized / full screen first
        let _ = w.set_fullscreen(false);
        let _ = w.unmaximize();
    }
    size_window(&w, &mode, true);
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

/// Looks for a newer release on GitHub (runs off the main thread).
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<updater::UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || updater::check(&current))
        .await
        .map_err(|e| e.to_string())?
}

/// Downloads the new installer (emitting `update-progress`), starts it and
/// quits so it can replace the running app. SonicBoost relaunches afterwards.
#[tauri::command]
async fn install_update(app: AppHandle, url: String) -> Result<(), String> {
    let h = app.clone();
    let path = tauri::async_runtime::spawn_blocking(move || {
        updater::download(&url, |p| {
            let _ = h.emit("update-progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    {
        let st = app.state::<AppState>();
        let _ = st.save();
        st.engine.lock().unwrap().stop();
    }
    updater::launch_installer(&path)?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn set_auto_update(state: State<AppState>, enabled: bool) -> Result<(), String> {
    state.update(|s| s.auto_update = enabled);
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
fn win_toggle_maximize(app: AppHandle) -> Result<WinState, String> {
    let w = app.get_webview_window("main").ok_or("window missing")?;
    if w.is_fullscreen().unwrap_or(false) {
        let _ = w.set_fullscreen(false);
    } else if w.is_maximized().unwrap_or(false) {
        w.unmaximize().map_err(|e| e.to_string())?;
    } else {
        w.maximize().map_err(|e| e.to_string())?;
    }
    Ok(win_state_of(&w))
}

#[tauri::command]
fn win_toggle_fullscreen(app: AppHandle) -> Result<WinState, String> {
    let w = app.get_webview_window("main").ok_or("window missing")?;
    let f = w.is_fullscreen().unwrap_or(false);
    w.set_fullscreen(!f).map_err(|e| e.to_string())?;
    Ok(win_state_of(&w))
}

#[tauri::command]
fn win_state(app: AppHandle) -> Result<WinState, String> {
    let w = app.get_webview_window("main").ok_or("window missing")?;
    Ok(win_state_of(&w))
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

/// Ctrl+Alt+1 … Ctrl+Alt+9 switch to profile 1 … 9.
const PROFILE_KEYS: [Code; 9] = [
    Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4, Code::Digit5,
    Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9,
];

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
                } else if let Some(i) = PROFILE_KEYS.iter().position(|c| sc == &hotkey(*c)) {
                    if let Ok(name) = apply_profile_at(app, i) {
                        let _ = app.emit("profile-hotkey", name);
                    }
                }
            })
            .build(),
    )?;
    let base = [Code::ArrowUp, Code::ArrowDown, Code::KeyB, Code::KeyP, Code::ArrowRight, Code::ArrowLeft];
    for code in base.into_iter().chain(PROFILE_KEYS) {
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

/// Tray menu with a "Profiles" submenu (✓ marks the active one).
fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let (names, active) = {
        let s = app.state::<AppState>().settings.lock().unwrap().clone();
        (s.profiles.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), s.active_profile)
    };
    let show = MenuItem::with_id(app, "show", "Open SonicBoost", true, None::<&str>)?;
    let bypass = MenuItem::with_id(app, "bypass", "Toggle Boost On/Off", true, None::<&str>)?;
    let play = MenuItem::with_id(app, "playpause", "Play / Pause music", true, None::<&str>)?;
    let mut items: Vec<CheckMenuItem<tauri::Wry>> = Vec::new();
    for (i, n) in names.iter().enumerate() {
        let label = if i < 9 { format!("{n}\tCtrl+Alt+{}", i + 1) } else { n.clone() };
        items.push(CheckMenuItem::with_id(app, format!("profile:{i}"), label, true, *n == active, None::<&str>)?);
    }
    let empty = MenuItem::with_id(app, "noprofiles", "No profiles yet", false, None::<&str>)?;
    let refs: Vec<&dyn IsMenuItem<tauri::Wry>> = if items.is_empty() {
        vec![&empty as &dyn IsMenuItem<tauri::Wry>]
    } else {
        items.iter().map(|i| i as &dyn IsMenuItem<tauri::Wry>).collect()
    };
    let profiles = Submenu::with_items(app, "Profiles", true, &refs)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    Menu::with_items(app, &[&show, &sep, &profiles, &bypass, &play, &sep2, &quit])
}

fn refresh_tray(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), build_tray_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
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
            size_window(&win, &cfg.mode, true);
            let autostarted = std::env::args().any(|a| a == "--minimized");
            if !(autostarted && cfg.start_minimized) {
                let _ = win.show();
            }

            // System tray.
            let menu = build_tray_menu(app.handle())?;
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("SonicBoost")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, e| match e.id.as_ref() {
                    "show" => show_main(app),
                    "bypass" => toggle_bypass(app),
                    "playpause" => app.state::<AppState>().media.send(MediaCmd::PlayPause),
                    id if id.starts_with("profile:") => {
                        if let Ok(i) = id["profile:".len()..].parse::<usize>() {
                            let _ = apply_profile_at(app, i);
                        }
                    }
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
            set_peq,
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
            list_profiles,
            save_profile,
            apply_profile,
            delete_profile,
            check_update,
            install_update,
            set_auto_update,
            media_control,
            open_link,
            win_minimize,
            win_toggle_maximize,
            win_toggle_fullscreen,
            win_state,
            win_hide
        ])
        .run(tauri::generate_context!())
        .expect("error while running SonicBoost");
}
