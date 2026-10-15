use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{Speak, Status, SystemVoice, Voices};

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
}
