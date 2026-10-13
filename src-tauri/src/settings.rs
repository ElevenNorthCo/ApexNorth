use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How much the engineer talks. Category toggles still decide what about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatterLevel {
    /// Critical information only.
    Minimal,
    /// Useful race updates without excessive chatter.
    #[default]
    Normal,
    /// Frequent coaching and strategy updates.
    Chatty,
    /// Maximum useful race-engineer feedback.
    CrewChief,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Callouts {
    pub lap_times: bool,
    pub sectors: bool,
    pub pace: bool,
    pub fuel: bool,
    #[serde(alias = "pitWindow")]
    pub pit_strategy: bool,
    pub tyres: bool,
    pub wheel_slip: bool,
    pub incidents: bool,
    pub race_start: bool,
    pub position: bool,
}

impl Default for Callouts {
    fn default() -> Self {
        Callouts {
            lap_times: true,
            sectors: true,
            pace: true,
            fuel: true,
            pit_strategy: true,
            tyres: true,
            wheel_slip: true,
            incidents: true,
            race_start: true,
            position: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceSettings {
    pub enabled: bool,
    pub piper_path: String,
    pub model_path: String,
    /// 0.0 - 1.0
    pub volume: f32,
    /// Piper `--length-scale`: < 1 speaks faster.
    pub length_scale: f32,
    pub chatter: ChatterLevel,
    pub callouts: Callouts,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings {
            enabled: true,
            piper_path: find_piper().map(|p| p.display().to_string()).unwrap_or_default(),
            model_path: String::new(),
            volume: 0.9, // DEFAULT_VOLUME in Settings.tsx
            length_scale: 0.9, // DEFAULT_LENGTH_SCALE in Settings.tsx
            chatter: ChatterLevel::default(),
            callouts: Callouts::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub last_ip: Option<String>,
    pub auto_connect: bool,
    /// "metric" or "imperial"
    pub units: String,
    /// What the engineer calls the driver. Empty = no name.
    pub driver_name: String,
    pub voice: VoiceSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { last_ip: None, auto_connect: true, units: "metric".into(), driver_name: String::new(), voice: VoiceSettings::default() }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }
}

/// Look for a `piper` executable on PATH and in common install locations.
pub fn find_piper() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "piper.exe" } else { "piper" };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("piper"));
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from));
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

/// Find Piper voice models (`*.onnx` with a matching `.onnx.json`).
pub fn find_voices(extra_dirs: &[PathBuf]) -> Vec<String> {
    let mut dirs = extra_dirs.to_vec();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/piper"));
        dirs.push(home.join(".local/share/piper/voices"));
        dirs.push(home.join("piper-voices"));
    }
    let mut out = vec![];
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "onnx") && p.with_extension("onnx.json").is_file() {
                out.push(p.display().to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_still_load() {
        let v: VoiceSettings = serde_json::from_str(r#"{"callouts": {"pitWindow": false, "fuel": false}}"#).unwrap();
        assert!(!v.callouts.pit_strategy && !v.callouts.fuel);
        assert!(v.callouts.sectors && v.callouts.incidents);
        assert_eq!(v.chatter, ChatterLevel::Normal);
        let c: VoiceSettings = serde_json::from_str(r#"{"chatter": "crewChief"}"#).unwrap();
        assert_eq!(c.chatter, ChatterLevel::CrewChief);
    }
}
