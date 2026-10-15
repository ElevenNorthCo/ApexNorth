//! ApexNorth's phone integrations, called from the app's Rust code:
//!
//! - the system's offline text-to-speech (Android `TextToSpeech`, iOS
//!   `AVSpeechSynthesizer`), which speaks the race engineer on phones;
//! - keeping the screen on while telemetry is live.
//!
//! Kotlin lives in `android/`, Swift in `ios/`. On desktop every call
//! returns [`Error::Unsupported`]; the app uses its own engine there.

use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

#[cfg(desktop)]
pub use desktop::Device;
#[cfg(mobile)]
pub use mobile::Device;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not available on this platform")]
    Unsupported,
    #[cfg(mobile)]
    #[error(transparent)]
    PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

pub type Result<T> = std::result::Result<T, Error>;

/// One phrase to speak.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Speak<'a> {
    pub text: &'a str,
    /// A system voice id; none picks the best installed voice for `lang`.
    pub voice: Option<&'a str>,
    /// BCP 47 language, e.g. "en-GB".
    pub lang: &'a str,
    /// 1.0 is the engine's normal speed.
    pub rate: f32,
    /// 0.0 to 1.0.
    pub volume: f32,
}

/// An installed voice that works offline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemVoice {
    pub id: String,
    pub name: String,
    pub lang: String,
    /// Higher is better (iOS: 1 default, 2 enhanced, 3 premium; Android: 100 to 500).
    #[serde(default)]
    pub quality: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    pub speaking: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Voices {
    pub voices: Vec<SystemVoice>,
}

/// Access to the plugin from any [`Manager`] (`app.device()`).
pub trait DeviceExt<R: Runtime> {
    fn device(&self) -> &Device<R>;
}

impl<R: Runtime, T: Manager<R>> DeviceExt<R> for T {
    fn device(&self) -> &Device<R> {
        self.state::<Device<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("apexnorth-device")
        .setup(|app, api| {
            #[cfg(mobile)]
            let device = mobile::init(app, api)?;
            #[cfg(desktop)]
            let device = desktop::init(app, api);
            app.manage(device);
            Ok(())
        })
        .build()
}
