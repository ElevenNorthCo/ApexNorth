//! What differs between desktop and phones outside the voice (which is in
//! `engineer/voice*.rs`). Everything else — telemetry, the engine, the
//! engineer's rules, strategy, storage — is the same code on every platform.

use tauri::AppHandle;

/// The OS this build runs on ("macos", "windows", "linux", "android", "ios").
pub const OS: &str = std::env::consts::OS;
/// A phone or tablet build (Android or iOS).
pub const MOBILE: bool = cfg!(mobile);

/// The test hook that starts demo mode at launch (see `smoke` in lib.rs):
/// `APEXNORTH_AUTODEMO=1`, or on Android the system property
/// `debug.apexnorth.autodemo=1` (apps there can't be given environment
/// variables).
pub fn autodemo_requested() -> bool {
    if std::env::var_os("APEXNORTH_AUTODEMO").is_some_and(|v| v == "1") {
        return true;
    }
    #[cfg(target_os = "android")]
    {
        return android::property("debug.apexnorth.autodemo").as_deref() == Some("1");
    }
    #[allow(unreachable_code)]
    false
}

/// A line in the system log where the platform keeps one apart from stdout
/// (Android's logcat, tag "ApexNorth"); nothing elsewhere.
pub fn system_log(_line: &str) {
    #[cfg(target_os = "android")]
    android::log(_line);
}

#[cfg(target_os = "android")]
mod android {
    use std::ffi::{c_char, c_int, CStr, CString};

    extern "C" {
        // bionic libc
        fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
    }
    #[link(name = "log")]
    extern "C" {
        fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
    }

    pub fn property(name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        // PROP_VALUE_MAX is 92 bytes, including the terminator.
        let mut value = [0 as c_char; 92];
        let n = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr()) };
        (n > 0).then(|| unsafe { CStr::from_ptr(value.as_ptr()) }.to_string_lossy().into_owned())
    }

    pub fn log(line: &str) {
        const INFO: c_int = 4;
        if let Ok(text) = CString::new(line) {
            unsafe { __android_log_write(INFO, c"ApexNorth".as_ptr(), text.as_ptr()) };
        }
    }
}

/// Keeps the screen on while telemetry is live, so the dashboard doesn't
/// sleep mid-race. Released when dropped.
pub struct ScreenAwake {
    #[cfg(desktop)]
    held: Option<keepawake::KeepAwake>,
    #[cfg(mobile)]
    held: bool,
    #[cfg(mobile)]
    app: AppHandle,
}

impl ScreenAwake {
    #[cfg_attr(desktop, allow(unused_variables))]
    pub fn new(app: &AppHandle) -> ScreenAwake {
        ScreenAwake {
            #[cfg(desktop)]
            held: None,
            #[cfg(mobile)]
            held: false,
            #[cfg(mobile)]
            app: app.clone(),
        }
    }

    #[cfg(desktop)]
    pub fn set(&mut self, on: bool) {
        if !on {
            self.held = None;
        } else if self.held.is_none() {
            self.held = keepawake::Builder::default()
                .display(true)
                .idle(true)
                .reason("Live telemetry")
                .app_name("ApexNorth")
                .app_reverse_domain("com.elevennorth.apexnorth")
                .create()
                .ok();
        }
    }

    #[cfg(mobile)]
    pub fn set(&mut self, on: bool) {
        use tauri_plugin_apexnorth_device::DeviceExt;
        if self.held == on {
            return;
        }
        self.held = on;
        // Off the link thread: a plugin call waits on the UI thread, which
        // may itself be waiting for the link to stop (disconnect).
        let app = self.app.clone();
        std::thread::spawn(move || {
            if let Err(e) = app.device().keep_awake(on) {
                log::warn!("keep awake: {e}");
            }
        });
    }
}

impl Drop for ScreenAwake {
    fn drop(&mut self) {
        self.set(false);
    }
}

/// The phone's own offline voice, through the `apexnorth-device` plugin.
#[cfg(mobile)]
pub struct PhoneSpeech(pub AppHandle);

#[cfg(mobile)]
impl crate::engineer::voice_native::NativeSpeech for PhoneSpeech {
    fn load(&self) -> Result<(), String> {
        use tauri_plugin_apexnorth_device::DeviceExt;
        self.0.device().load_speech().map_err(|e| e.to_string())
    }

    fn speak(&self, u: &crate::engineer::voice_native::Utterance) -> Result<(), String> {
        use tauri_plugin_apexnorth_device::{DeviceExt, Speak};
        let s = Speak { text: &u.text, voice: u.voice.as_deref(), lang: u.lang, rate: u.rate, volume: u.volume };
        self.0.device().speak(&s).map_err(|e| e.to_string())
    }

    fn speaking(&self) -> bool {
        use tauri_plugin_apexnorth_device::DeviceExt;
        self.0.device().speaking().unwrap_or(false)
    }

    fn stop(&self) {
        use tauri_plugin_apexnorth_device::DeviceExt;
        let _ = self.0.device().stop_speaking();
    }
}
