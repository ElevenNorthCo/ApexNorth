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
//! Android reads the same feed: its APK is the `android-aarch64` platform.
//! The device plugin downloads it and opens the system installer (Android
//! checks the APK is signed with the same key as the installed app). iOS
//! can't update itself: it updates through TestFlight / the App Store, so
//! there `check` finds nothing and `install` explains why.
#![cfg_attr(mobile, allow(dead_code))]

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::AppHandle;
#[cfg(any(desktop, target_os = "android"))]
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
/// Android: the APK's download URL.
#[cfg(target_os = "android")]
#[derive(Default)]
pub struct Pending(Mutex<Option<String>>);
#[cfg(target_os = "ios")]
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
    /// Android: the user has to allow ApexNorth to install apps first (the
    /// setting is open); then they tap Update again.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    NeedsPermission,
}

/// What the Update button shows: "0.3.0", or "0.3.0 beta 42" with the change.
fn offer(version: &str, notes: Option<&str>, beta: bool) -> UpdateInfo {
    match beta_notes(notes.unwrap_or("")) {
        (build, change) if beta && build > 0 => UpdateInfo {
            version: format!("{version} beta {build}"),
            notes: Some(change.to_string()),
        },
        _ => UpdateInfo { version: version.to_string(), notes: notes.map(str::to_string) },
    }
}

/// The feed's platform key for the Android APK.
#[cfg(any(target_os = "android", test))]
const ANDROID: &str = "android-aarch64";

/// The newer Android release in a feed (`latest.json`), and its APK URL.
#[cfg(any(target_os = "android", test))]
fn android_update(feed: &str, current: (u64, u64, u64), beta: bool, my_build: u64) -> Result<Option<(UpdateInfo, String)>, String> {
    #[derive(serde::Deserialize)]
    struct Feed {
        version: String,
        notes: Option<String>,
        #[serde(default)]
        platforms: std::collections::HashMap<String, Platform>,
    }
    #[derive(serde::Deserialize)]
    struct Platform {
        url: String,
    }
    let feed: Feed = serde_json::from_str(feed).map_err(|e| format!("Bad update feed: {e}"))?;
    let Some(apk) = feed.platforms.get(ANDROID) else { return Ok(None) };
    let mut v = feed.version.trim_start_matches('v').split(['.', '-', '+']).map(|n| n.parse::<u64>().unwrap_or(0));
    let remote = (v.next().unwrap_or(0), v.next().unwrap_or(0), v.next().unwrap_or(0));
    let remote_build = beta_notes(feed.notes.as_deref().unwrap_or("")).0;
    if !is_newer(current, remote, remote_build, beta, my_build) {
        return Ok(None);
    }
    Ok(Some((offer(&feed.version, feed.notes.as_deref(), beta), apk.url.clone())))
}

#[cfg(target_os = "android")]
pub async fn check(app: &AppHandle, pending: &Pending) -> Result<Option<UpdateInfo>, String> {
    use tauri_plugin_apexnorth_device::DeviceExt;
    // Plugin calls block until Kotlin answers: keep them off the async workers.
    let handle = app.clone();
    let text = tauri::async_runtime::spawn_blocking(move || handle.device().fetch_text(&feed()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let v = &app.package_info().version;
    let my_build = BUILD.and_then(|b| b.parse().ok()).unwrap_or(0);
    let found = android_update(&text, (v.major, v.minor, v.patch), is_beta(), my_build)?;
    let (info, url) = found.unzip();
    *pending.0.lock() = url;
    Ok(info)
}

/// Download the APK (progress to the button), then open the system
/// installer. Android shows "Update ApexNorth?"; once the user agrees it
/// replaces the app.
#[cfg(target_os = "android")]
pub async fn install(app: &AppHandle, pending: &Pending, progress: Channel<Progress>) -> Result<(), String> {
    use tauri_plugin_apexnorth_device::DeviceExt;
    let url = pending.0.lock().clone().ok_or("No update to install. Check again.")?;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let device = app.device();
        let err = |e: tauri_plugin_apexnorth_device::Error| e.to_string();
        // Already downloaded when the user is coming back from allowing installs.
        if device.update_progress().map_err(err)?.state != "done" {
            device.download_update(&url).map_err(err)?;
            let _ = progress.send(Progress::Started { total: None });
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let d = device.update_progress().map_err(err)?;
                let total = u64::try_from(d.total).ok().filter(|&t| t > 0);
                match d.state.as_str() {
                    "done" => break,
                    "failed" => return Err(d.error.unwrap_or_else(|| "Download failed.".into())),
                    _ => {
                        let _ = progress.send(Progress::Downloading { done: d.done, total });
                    }
                }
            }
        }
        let _ = progress.send(Progress::Installing);
        if !device.install_update().map_err(err)? {
            let _ = progress.send(Progress::NeedsPermission);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(target_os = "ios")]
pub async fn check(_app: &AppHandle, _pending: &Pending) -> Result<Option<UpdateInfo>, String> {
    Ok(None)
}

#[cfg(target_os = "ios")]
pub async fn install(_app: &AppHandle, _pending: &Pending, _progress: Channel<Progress>) -> Result<(), String> {
    Err("On iPhone, updates come from TestFlight or the App Store.".into())
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
    let info = found.as_ref().map(|u| offer(&u.version, u.body.as_deref(), beta));
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

    const FEED: &str = r#"{"version":"0.4.0","notes":"beta.12: Phones","pub_date":"2026-10-17T00:00:00Z",
        "platforms":{"darwin-aarch64":{"signature":"s","url":"https://x/mac.tar.gz"},
                     "android-aarch64":{"signature":"s","url":"https://x/ApexNorth-Android.apk"}}}"#;

    #[test]
    fn android_finds_its_apk_in_the_feed() {
        let (info, url) = android_update(FEED, (0, 3, 1), false, 0).unwrap().unwrap();
        assert_eq!(url, "https://x/ApexNorth-Android.apk");
        assert_eq!(info.version, "0.4.0");
        // Beta: the build number comes from the notes.
        let (info, _) = android_update(FEED, (0, 4, 0), true, 11).unwrap().unwrap();
        assert_eq!(info.version, "0.4.0 beta 12");
        assert_eq!(info.notes.as_deref(), Some("Phones"));
        assert!(android_update(FEED, (0, 4, 0), true, 12).unwrap().is_none());
        assert!(android_update(FEED, (0, 4, 0), false, 0).unwrap().is_none());
        // A release without an APK offers nothing.
        let desktop_only = r#"{"version":"9.0.0","platforms":{"darwin-aarch64":{"signature":"s","url":"u"}}}"#;
        assert!(android_update(desktop_only, (0, 4, 0), false, 0).unwrap().is_none());
    }
}
