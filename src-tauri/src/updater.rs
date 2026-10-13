//! In-app updates: check GitHub Releases for a newer signed build, download it
//! with progress, install it and restart.
//!
//! The release workflow publishes a `latest.json` next to the installers and
//! signs each update with the project's updater key (the public half is in
//! `tauri.conf.json`). Stable builds read the Latest release's feed. Beta
//! builds (built with `APEXNORTH_CHANNEL=beta`) read the rolling
//! `beta-latest` feed, whose version carries the build number
//! (`0.3.0+beta.42`), so a new beta of the same version still counts as newer.

use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Url};
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

/// The build number in a beta version's metadata ("beta.42" → 42).
fn beta_build(build_metadata: &str) -> u64 {
    build_metadata.rsplit('.').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Is the remote release newer than this app? Stable compares versions. Beta
/// also takes a later build of the same version.
fn is_newer(current: (u64, u64, u64), remote: (u64, u64, u64), remote_build: &str, beta: bool, my_build: u64) -> bool {
    if remote != current {
        return remote > current;
    }
    beta && beta_build(remote_build) > my_build
}

/// Which update feed this build follows, for the Settings screen.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    /// "beta" or "stable".
    pub channel: &'static str,
    /// The beta build number (the release workflow's run number).
    pub build: Option<u64>,
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        channel: if is_beta() { "beta" } else { "stable" },
        build: BUILD.and_then(|b| b.parse().ok()).filter(|_| is_beta()),
    }
}

/// The update found by the last check, ready to install.
#[derive(Default)]
pub struct Pending(Mutex<Option<Update>>);

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
                v.build.as_str(),
                beta,
                my_build,
            )
        })
        .build()
        .map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    let info = found.as_ref().map(|u| UpdateInfo {
        version: u.version.replace("+beta.", " beta "),
        notes: u.body.clone(),
    });
    *pending.0.lock() = found;
    Ok(info)
}

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
        assert!(is_newer((0, 2, 0), (0, 3, 0), "", false, 0));
        assert!(is_newer((0, 2, 0), (1, 0, 0), "", false, 0));
        assert!(!is_newer((0, 2, 0), (0, 2, 0), "", false, 0));
        assert!(!is_newer((0, 3, 0), (0, 2, 9), "", false, 0));
    }

    #[test]
    fn beta_takes_a_later_build_of_the_same_version() {
        assert!(is_newer((0, 3, 0), (0, 3, 0), "beta.42", true, 41));
        assert!(!is_newer((0, 3, 0), (0, 3, 0), "beta.42", true, 42));
        assert!(!is_newer((0, 3, 0), (0, 3, 0), "beta.40", true, 42));
        assert!(is_newer((0, 2, 0), (0, 3, 0), "beta.1", true, 99));
        // A stable build never takes a rebuild of its own version.
        assert!(!is_newer((0, 3, 0), (0, 3, 0), "beta.42", false, 0));
    }

    #[test]
    fn beta_build_numbers() {
        assert_eq!(beta_build("beta.42"), 42);
        assert_eq!(beta_build(""), 0);
        assert_eq!(beta_build("beta.x"), 0);
    }
}
