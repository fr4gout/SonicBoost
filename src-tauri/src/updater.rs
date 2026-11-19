//! In-app updates from GitHub Releases.
//!
//! 1. `check()` asks the GitHub API for the latest release of
//!    fr4gout/SonicBoost and compares its tag with the running version.
//! 2. `download()` fetches the `*-setup.exe` asset (only from this repo's
//!    release download URLs) into %TEMP%, reporting progress.
//! 3. `launch_installer()` starts the installer in passive mode and, once it
//!    finishes, relaunches SonicBoost. The caller then exits the app.
//!
//! Settings, presets and the Windows-startup entry are kept, because the
//! installer upgrades the existing installation in place.

use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const REPO: &str = "fr4gout/SonicBoost";

#[derive(Serialize, Clone, Debug, Default)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub available: bool,
    pub notes: String,
    pub url: Option<String>,
    pub page: String,
    pub size: u64,
}

#[derive(Serialize, Clone)]
pub struct Progress {
    pub done: u64,
    pub total: u64,
}

/// (major, minor, patch, is_prerelease)
pub fn parse_version(v: &str) -> (u64, u64, u64, bool) {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once('-') {
        Some((c, _)) => (c, true),
        None => (v, false),
    };
    let mut it = core.split('.').map(|x| x.trim().parse::<u64>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0), pre)
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    let a = parse_version(latest);
    let b = parse_version(current);
    let (ca, cb) = ((a.0, a.1, a.2), (b.0, b.1, b.2));
    ca > cb || (ca == cb && b.3 && !a.3)
}

/// Only this repository's release assets may be downloaded and run.
pub fn is_trusted_url(url: &str) -> bool {
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    url.to_lowercase().starts_with(&prefix.to_lowercase()) && url.to_lowercase().ends_with(".exe")
}

pub fn check(current: &str) -> Result<UpdateInfo, String> {
    let v: serde_json::Value = ureq::get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("User-Agent", "SonicBoost-Updater")
        .set("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(12))
        .call()
        .map_err(|e| format!("Could not reach GitHub: {e}"))?
        .into_json()
        .map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;

    let latest = v["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string();
    let asset = v["assets"].as_array().and_then(|a| {
        a.iter().find(|x| x["name"].as_str().map(|n| n.ends_with("-setup.exe")).unwrap_or(false))
    });
    let url = asset
        .and_then(|a| a["browser_download_url"].as_str())
        .filter(|u| is_trusted_url(u))
        .map(String::from);
    Ok(UpdateInfo {
        available: !latest.is_empty() && is_newer(&latest, current) && url.is_some(),
        current: current.to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        size: asset.and_then(|a| a["size"].as_u64()).unwrap_or(0),
        latest,
        url,
    })
}

pub fn download(url: &str, mut progress: impl FnMut(Progress)) -> Result<PathBuf, String> {
    if !is_trusted_url(url) {
        return Err("Refusing to download an update from an unknown address.".into());
    }
    let resp = ureq::get(url)
        .set("User-Agent", "SonicBoost-Updater")
        .timeout(Duration::from_secs(600))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total: u64 = resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0);
    let path = std::env::temp_dir().join("SonicBoost-update-setup.exe");
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let (mut done, mut last) = (0u64, Instant::now());
    let mut head = [0u8; 2];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        if done == 0 && n >= 2 {
            head = [buf[0], buf[1]];
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed() > Duration::from_millis(120) {
            progress(Progress { done, total });
            last = Instant::now();
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    progress(Progress { done, total });
    if &head != b"MZ" || done < 500_000 || (total > 0 && done != total) {
        let _ = std::fs::remove_file(&path);
        return Err("The downloaded installer looks incomplete. Please try again.".into());
    }
    Ok(path)
}

/// Runs the installer (passive: progress bar only, no questions), then
/// starts the freshly installed SonicBoost again.
pub fn launch_installer(installer: &std::path::Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let q = |p: &std::path::Path| p.display().to_string().replace('\'', "''");
    let script = format!(
        "Start-Sleep -Milliseconds 800; Start-Process -FilePath '{}' -ArgumentList '/P' -Wait; Start-Process -FilePath '{}'",
        q(installer),
        q(&exe)
    );
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Could not start the installer: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("1.0.1", "1.0.0"));
        assert!(is_newer("v1.1.0", "1.0.9"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(is_newer("1.1.0", "1.1.0-beta.2"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(!is_newer("1.1.0-beta.1", "1.1.0"));
    }

    #[test]
    fn only_this_repo_is_trusted() {
        assert!(is_trusted_url(
            "https://github.com/fr4gout/SonicBoost/releases/download/v1.0.1/SonicBoost_1.0.1_x64-setup.exe"
        ));
        assert!(is_trusted_url(
            "https://github.com/fr4gout/sonicboost/releases/download/v1.0.1/SonicBoost_1.0.1_x64-setup.exe"
        ));
        assert!(!is_trusted_url("https://evil.example/SonicBoost-setup.exe"));
        assert!(!is_trusted_url("https://github.com/someone/SonicBoost/releases/download/v1/x.exe"));
        assert!(!is_trusted_url("https://github.com/fr4gout/SonicBoost/releases/download/v1/notes.txt"));
    }
}
