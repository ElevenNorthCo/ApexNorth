use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{Download, Insets, Installed, Speak, Status, SystemVoice, Text, Voices};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_apexnorth_device);

pub fn init<R: Runtime, C: DeserializeOwned>(_app: &AppHandle<R>, api: PluginApi<R, C>) -> crate::Result<Device<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("com.elevennorth.apexnorth.device", "DevicePlugin")?;
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_apexnorth_device)?;
    Ok(Device(handle))
}

/// The Kotlin or Swift side of the plugin. Each call blocks until the
/// platform answers, so don't call it from the main thread.
pub struct Device<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Device<R> {
    /// Start the speech engine and wait until it's ready.
    pub fn load_speech(&self) -> crate::Result<()> {
        self.0.run_mobile_plugin::<Value>("prepare", ())?;
        Ok(())
    }

    /// Speak, replacing anything being said.
    pub fn speak(&self, s: &Speak) -> crate::Result<()> {
        self.0.run_mobile_plugin::<Value>("speak", s)?;
        Ok(())
    }

    pub fn speaking(&self) -> crate::Result<bool> {
        Ok(self.0.run_mobile_plugin::<Status>("status", ())?.speaking)
    }

    pub fn stop_speaking(&self) -> crate::Result<()> {
        self.0.run_mobile_plugin::<Value>("stop", ())?;
        Ok(())
    }

    /// English voices that work offline.
    pub fn voices(&self) -> crate::Result<Vec<SystemVoice>> {
        Ok(self.0.run_mobile_plugin::<Voices>("voices", ())?.voices)
    }

    /// Keep the screen on (or let it sleep again). Returns at once; the
    /// change happens on the UI thread.
    pub fn keep_awake(&self, on: bool) -> crate::Result<()> {
        self.0.run_mobile_plugin::<Value>("keepAwake", serde_json::json!({ "on": on }))?;
        Ok(())
    }

    /// GET a small text file (Android).
    pub fn fetch_text(&self, url: &str) -> crate::Result<String> {
        Ok(self.0.run_mobile_plugin::<Text>("fetchText", serde_json::json!({ "url": url }))?.text)
    }

    /// Start downloading the update APK (Android); poll [`Self::update_progress`].
    pub fn download_update(&self, url: &str) -> crate::Result<()> {
        self.0.run_mobile_plugin::<Value>("downloadUpdate", serde_json::json!({ "url": url }))?;
        Ok(())
    }

    pub fn update_progress(&self) -> crate::Result<Download> {
        Ok(self.0.run_mobile_plugin::<Download>("updateProgress", ())?)
    }

    /// Open the system installer on the downloaded APK (Android). False when
    /// Android first needs the user to allow ApexNorth to install apps (the
    /// setting has been opened for them).
    pub fn install_update(&self) -> crate::Result<bool> {
        Ok(!self.0.run_mobile_plugin::<Installed>("installUpdate", ())?.needs_permission)
    }

    /// The screen's real safe areas (iOS).
    pub fn safe_area(&self) -> crate::Result<Insets> {
        Ok(self.0.run_mobile_plugin::<Insets>("safeArea", ())?)
    }
}
