//! "Now Playing" — reads the Windows media session (the same source as the
//! volume-flyout media overlay), so it works with Spotify, browsers
//! (YouTube, SoundCloud…), Windows Media Player, VLC, Apple Music, etc.

use serde::Serialize;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct MediaStatus {
    pub available: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub status: String, // "playing" | "paused" | "stopped" | "none"
    pub position: f64,  // seconds (already corrected for time since last update)
    pub duration: f64,  // seconds, 0 if unknown
    /// `data:` URL of the album art. Only sent when the track changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub art: Option<String>,
    pub track_id: String,
}

pub enum MediaCmd {
    PlayPause,
    Next,
    Previous,
    /// Re-send everything incl. album art (UI just opened).
    Refresh,
}

pub struct Media {
    tx: Sender<MediaCmd>,
}

impl Media {
    pub fn spawn(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("media-session".into())
            .spawn(move || run(app, rx))
            .expect("spawn media thread");
        Self { tx }
    }
    pub fn send(&self, c: MediaCmd) {
        let _ = self.tx.send(c);
    }
}

/// Turns an app id like "Spotify.exe" or "308046B0AF4A39CB" into a nice name.
pub fn pretty_app(aumid: &str) -> String {
    let l = aumid.to_lowercase();
    let known = [
        ("spotify", "Spotify"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("brave", "Brave"),
        ("zunemusic", "Media Player"),
        ("vlc", "VLC"),
        ("applemusic", "Apple Music"),
        ("itunes", "iTunes"),
        ("tidal", "TIDAL"),
        ("deezer", "Deezer"),
        ("youtube", "YouTube Music"),
        ("foobar", "foobar2000"),
        ("musicbee", "MusicBee"),
        ("discord", "Discord"),
        ("308046b0af4a39cb", "Firefox"),
    ];
    for (k, v) in known {
        if l.contains(k) {
            return v.to_string();
        }
    }
    let base = aumid.rsplit(['\\', '!']).next().unwrap_or(aumid);
    base.trim_end_matches(".exe").trim_end_matches(".EXE").to_string()
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(windows)]
fn run(app: AppHandle, rx: Receiver<MediaCmd>) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut last = MediaStatus::default();
    let mut last_track = String::new();
    let mut last_emit = std::time::Instant::now();
    loop {
        // Handle button presses immediately, otherwise poll every 500 ms.
        if let Ok(cmd) = rx.recv_timeout(Duration::from_millis(500)) {
            if matches!(cmd, MediaCmd::Refresh) {
                last_track.clear();
            } else {
                let _ = win::control(cmd);
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        let mut st = win::query().unwrap_or_default();
        let new_track = st.track_id != last_track;
        if new_track {
            last_track = st.track_id.clone();
            st.art = win::thumbnail().ok().flatten();
            if st.art.is_none() {
                st.art = Some(String::new()); // tells UI to clear old art
            }
        }
        let mut cmp = st.clone();
        cmp.art = None;
        cmp.position = (cmp.position).floor();
        let changed = new_track || cmp != last;
        if changed || last_emit.elapsed() > Duration::from_secs(2) {
            let _ = app.emit("media-status", &st);
            last = cmp;
            last_emit = std::time::Instant::now();
        }
    }
}

#[cfg(not(windows))]
fn run(app: AppHandle, rx: Receiver<MediaCmd>) {
    loop {
        let _ = rx.recv_timeout(Duration::from_secs(2));
        let _ = app.emit("media-status", &MediaStatus::default());
    }
}

#[cfg(windows)]
mod win {
    use super::{base64, pretty_app, MediaCmd, MediaStatus};
    use windows::core::{Interface, Result};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Ps,
    };
    use windows::Foundation::IAsyncOperation;
    use windows::Storage::Streams::{DataReader, IInputStream};

    fn session() -> Result<Session> {
        let mgr = Manager::RequestAsync()?.get()?;
        mgr.GetCurrentSession()
    }

    /// 100-ns ticks since 1601-01-01, same epoch as WinRT DateTime.
    fn now_ticks() -> i64 {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        (unix.as_nanos() / 100) as i64 + 116_444_736_000_000_000
    }

    pub fn query() -> Result<MediaStatus> {
        let s = session()?;
        let props = s.TryGetMediaPropertiesAsync()?.get()?;
        let title = props.Title()?.to_string();
        let artist = props.Artist()?.to_string();
        let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
        let app = pretty_app(&s.SourceAppUserModelId()?.to_string());

        let status = match s.GetPlaybackInfo()?.PlaybackStatus()? {
            Ps::Playing => "playing",
            Ps::Paused => "paused",
            _ => "stopped",
        };

        let (mut position, mut duration) = (0.0, 0.0);
        if let Ok(tl) = s.GetTimelineProperties() {
            let start = tl.StartTime().map(|t| t.Duration).unwrap_or(0);
            let end = tl.EndTime().map(|t| t.Duration).unwrap_or(0);
            let pos = tl.Position().map(|t| t.Duration).unwrap_or(0);
            let updated = tl.LastUpdatedTime().map(|t| t.UniversalTime).unwrap_or(0);
            duration = ((end - start).max(0)) as f64 / 1e7;
            position = (pos - start).max(0) as f64 / 1e7;
            if status == "playing" && updated > 0 {
                position += ((now_ticks() - updated).max(0)) as f64 / 1e7;
            }
            if duration > 0.0 {
                position = position.min(duration);
            }
        }

        Ok(MediaStatus {
            available: !title.is_empty(),
            track_id: format!("{app}|{title}|{artist}"),
            title,
            artist,
            album,
            app,
            status: status.into(),
            position,
            duration,
            art: None,
        })
    }

    pub fn thumbnail() -> Result<Option<String>> {
        let s = session()?;
        let props = s.TryGetMediaPropertiesAsync()?.get()?;
        let Ok(thumb) = props.Thumbnail() else { return Ok(None) };
        let stream = thumb.OpenReadAsync()?.get()?;
        let size = stream.Size()? as u32;
        if size == 0 || size > 8_000_000 {
            return Ok(None);
        }
        let mime = stream.ContentType().map(|h| h.to_string()).unwrap_or_default();
        let mime = if mime.starts_with("image/") { mime } else { "image/png".into() };
        let input: IInputStream = stream.cast()?;
        let reader = DataReader::CreateDataReader(&input)?;
        let load: IAsyncOperation<u32> = reader.LoadAsync(size)?.cast()?;
        load.get()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf)?;
        Ok(Some(format!("data:{mime};base64,{}", base64(&buf))))
    }

    pub fn control(cmd: MediaCmd) -> Result<()> {
        let s = session()?;
        match cmd {
            MediaCmd::PlayPause => s.TryTogglePlayPauseAsync()?.get()?,
            MediaCmd::Next => s.TrySkipNextAsync()?.get()?,
            MediaCmd::Previous => s.TrySkipPreviousAsync()?.get()?,
            MediaCmd::Refresh => true,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn b64() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(base64(b"hello world"), "aGVsbG8gd29ybGQ=");
    }
    #[test]
    fn app_names() {
        assert_eq!(pretty_app("Spotify.exe"), "Spotify");
        assert_eq!(pretty_app("MSEdge"), "Edge");
        assert_eq!(pretty_app("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Media Player");
        assert_eq!(pretty_app("C:\\Apps\\cool.exe"), "cool");
    }
}
