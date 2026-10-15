//! In-app updates: check GitHub Releases for a newer signed build, download it
//! with progress, install it and restart.
//!
//! The release workflow publishes a `latest.json` next to the installers and
//! signs each update with the project's updater key (the public half is in
//! `tauri.conf.json`). Stable builds read the Latest release's feed. Beta
//! builds (built with `APEXNORTH_CHANNEL=beta`) read the rolling
//! `beta-latest` feed. Its version must be the one the files were signed for
//! (plain `0.3.1`; the updater rejects a mismatch), so the beta build number
//! rides in the notes (`beta.42: <change>`), and a new beta of the same
//! version still counts as newer.
//!
//! Desktop only: phones update through their app store (or a new APK), so
//! there `check` finds nothing and `install` explains why.
#![cfg_attr(mobile, allow(dead_code))]

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::AppHandle;
#[cfg(desktop)]
use parking_lot::Mutex;
#[cfg(desktop)]
use tauri::Url;
#[cfg(desktop)]
use tauri_plugin_updater::{Update, UpdaterExt};

const REPO: &str = "ElevenNorthCo/ApexNorth";

/// "beta" for builds from the `beta` branch; anything else is stable.
const CHANNEL: Option<&str> = option_env!("APEXNORTH_CHANNEL");
/// The release workflow's run number, set on beta builds.
const BUILD: Option<&str> = option_env!("APEXNORTH_BUILD");

fn is_beta() -> bool {
    CHANNEL == Some("beta")
}

fn feed() -> String {
    if is_beta() {
        format!("https://github.com/{REPO}/releases/download/beta-latest/latest.json")
    } else {
        format!("https://github.com/{REPO}/releases/latest/download/latest.json")
    }
}

/// A beta feed's notes, "beta.42: <change>", split into the build number and
/// the change. Anything else is no build number and the notes as they are.
fn beta_notes(notes: &str) -> (u64, &str) {
    notes
        .strip_prefix("beta.")
        .and_then(|rest| rest.split_once(':'))
        .and_then(|(n, rest)| Some((n.parse().ok()?, rest.trim_start())))
        .unwrap_or((0, notes))
}

/// Is the remote release newer than this app? Stable compares versions. Beta
/// also takes a later build of the same version.
fn is_newer(current: (u64, u64, u64), remote: (u64, u64, u64), remote_build: u64, beta: bool, my_build: u64) -> bool {
    if remote != current {
        return remote > current;
    }
    beta && remote_build > my_build
}

/// Which update feed this build follows, for the Settings screen.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    /// "beta" or "stable".
    pub channel: &'static str,
    /// The beta build number (the release workflow's run number).
    pub build: Option<u64>,
    /// "macos", "windows", "linux", "android" or "ios".
    pub os: &'static str,
    /// A phone build: no in-app updates, the system voice, touch layout.
    pub mobile: bool,
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        channel: if is_beta() { "beta" } else { "stable" },
        build: BUILD.and_then(|b| b.parse().ok()).filter(|_| is_beta()),
        os: crate::platform::OS,
        mobile: crate::platform::MOBILE,
    }
}

/// The update found by the last check, ready to install.
#[cfg(desktop)]
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);
#[cfg(mobile)]
#[derive(Default)]
pub struct Pending;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Shown on the button: "0.3.0", or "0.3.0 beta 42".
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event")]
pub enum Progress {
    Started { total: Option<u64> },
    Downloading { done: u64, total: Option<u64> },
    Installing,
}

#[cfg(mobile)]
pub async fn check(_app: &AppHandle, _pending: &Pending) -> Result<Option<UpdateInfo>, String> {
    Ok(None)
}

#[cfg(mobile)]
pub async fn install(_app: &AppHandle, _pending: &Pending, _progress: Channel<Progress>) -> Result<(), String> {
    Err("On phones, updates come from the app store or a new test build.".into())
}

#[cfg(desktop)]
pub async fn check(app: &AppHandle, pending: &Pending) -> Result<Option<UpdateInfo>, String> {
    let beta = is_beta();
    let my_build = BUILD.and_then(|b| b.parse().ok()).unwrap_or(0);
    let url: Url = feed().parse().map_err(|e| format!("{e}"))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .version_comparator(move |current, remote| {
            let v = &remote.version;
            is_newer(
                (current.major, current.minor, current.patch),
                (v.major, v.minor, v.patch),
                beta_notes(remote.notes.as_deref().unwrap_or("")).0,
                beta,
                my_build,
            )
        })
        .build()
        .map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let info = found.as_ref().map(|u| match beta_notes(u.body.as_deref().unwrap_or("")) {
        (build, notes) if beta && build > 0 => UpdateInfo {
            version: format!("{} beta {build}", u.version),
            notes: Some(notes.to_string()),
        },
        _ => UpdateInfo { version: u.version.clone(), notes: u.body.clone() },
    });
    *pending.0.lock() = found;
    Ok(info)
}

#[cfg(desktop)]
pub async fn install(app: &AppHandle, pending: &Pending, progress: Channel<Progress>) -> Result<(), String> {
    let update = pending.0.lock().clone().ok_or("No update to install. Check again.")?;
    let mut done = 0u64;
    let mut started = false;
    update
        .download_and_install(
            |chunk, total| {
                if !started {
                    started = true;
                    let _ = progress.send(Progress::Started { total });
                }
                done += chunk as u64;
                let _ = progress.send(Progress::Downloading { done, total });
            },
            || {
                let _ = progress.send(Progress::Installing);
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    // Windows has already handed over to the installer, which restarts the app.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_takes_only_a_higher_version() {
        assert!(is_newer((0, 2, 0), (0, 3, 0), 0, false, 0));
        assert!(is_newer((0, 2, 0), (1, 0, 0), 0, false, 0));
        assert!(!is_newer((0, 2, 0), (0, 2, 0), 0, false, 0));
        assert!(!is_newer((0, 3, 0), (0, 2, 9), 0, false, 0));
    }

    #[test]
    fn beta_takes_a_later_build_of_the_same_version() {
        assert!(is_newer((0, 3, 0), (0, 3, 0), 42, true, 41));
        assert!(!is_newer((0, 3, 0), (0, 3, 0), 42, true, 42));
        assert!(!is_newer((0, 3, 0), (0, 3, 0), 40, true, 42));
        assert!(is_newer((0, 2, 0), (0, 3, 0), 1, true, 99));
        // A stable build never takes a rebuild of its own version.
        assert!(!is_newer((0, 3, 0), (0, 3, 0), 42, false, 0));
    }

    #[test]
    fn beta_build_numbers_from_the_notes() {
        assert_eq!(beta_notes("beta.42: Emma is the default voice"), (42, "Emma is the default voice"));
        assert_eq!(beta_notes("beta.7:x: y"), (7, "x: y"));
        assert_eq!(beta_notes("Emma is the default voice"), (0, "Emma is the default voice"));
        assert_eq!(beta_notes("beta.x: nope"), (0, "beta.x: nope"));
        assert_eq!(beta_notes(""), (0, ""));
    }
}
