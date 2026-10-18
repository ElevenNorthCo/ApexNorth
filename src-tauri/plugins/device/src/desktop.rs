use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{Error, Speak, SystemVoice};

pub fn init<R: Runtime, C: DeserializeOwned>(_app: &AppHandle<R>, _api: PluginApi<R, C>) -> Device<R> {
    Device(PhantomData)
}

/// Desktop has its own voice engine and keep-awake; nothing here is used.
pub struct Device<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> Device<R> {
    pub fn load_speech(&self) -> crate::Result<()> {
        Err(Error::Unsupported)
    }
    pub fn speak(&self, _s: &Speak) -> crate::Result<()> {
        Err(Error::Unsupported)
    }
    pub fn speaking(&self) -> crate::Result<bool> {
        Err(Error::Unsupported)
    }
    pub fn stop_speaking(&self) -> crate::Result<()> {
        Err(Error::Unsupported)
    }
    pub fn voices(&self) -> crate::Result<Vec<SystemVoice>> {
        Err(Error::Unsupported)
    }
    pub fn keep_awake(&self, _on: bool) -> crate::Result<()> {
        Err(Error::Unsupported)
    }
}
