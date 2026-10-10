//! Where voices come from: the bundled engine folder, and the small catalog
//! of extra Piper voices that can be downloaded from Settings.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::engineer::tts;

/// The same list `scripts/fetch-voice.py` bundles from.
const CATALOG: &str = include_str!("../voices.json");

#[derive(Deserialize)]
struct Catalog {
    source: String,
    bundled: String,
    voices: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: String,
    name: String,
    accent: String,
    path: String,
    size_mb: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogVoice {
    pub id: String,
    pub name: String,
    pub accent: String,
    pub size_mb: u32,
    pub bundled: bool,
    /// Model path once it's on disk.
    pub path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    pub received: u64,
    pub total: Option<u64>,
}

fn catalog_data() -> Catalog {
    serde_json::from_str(CATALOG).expect("voices.json")
}

/// The bundled voice engine folder, if this build ships one. Checked in the
/// app's resources, then (for `tauri dev`) the source tree.
pub fn builtin_dir(resource_dir: Option<PathBuf>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("APEXNORTH_TTS_DIR").map(PathBuf::from).into_iter().collect();
    if let Some(r) = resource_dir {
        candidates.push(r.join("tts"));
        candidates.push(r.join("resources").join("tts"));
    }
    if cfg!(debug_assertions) {
        candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("tts"));
    }
    candidates.into_iter().find(|d| tts::runtime_present(d))
}

/// Model path of the voice shipped with the app.
pub fn bundled_voice(dir: &Path) -> Option<String> {
    let id = catalog_data().bundled;
    let p = dir.join("voices").join(format!("{id}.onnx"));
    if p.is_file() {
        return Some(p.display().to_string());
    }
    crate::settings::find_voices(&[dir.join("voices")]).into_iter().next()
}

fn installed(id: &str, voices_dir: &Path, builtin: Option<&Path>) -> Option<PathBuf> {
    let mut dirs = vec![voices_dir.to_path_buf()];
    dirs.extend(builtin.map(|b| b.join("voices")));
    dirs.into_iter()
        .map(|d| d.join(format!("{id}.onnx")))
        .find(|p| p.is_file() && PathBuf::from(format!("{}.json", p.display())).is_file())
}

pub fn catalog(voices_dir: &Path, builtin: Option<&Path>) -> Vec<CatalogVoice> {
    let c = catalog_data();
    c.voices
        .into_iter()
        .map(|e| CatalogVoice {
            path: installed(&e.id, voices_dir, builtin).map(|p| p.display().to_string()),
            bundled: e.id == c.bundled,
            id: e.id,
            name: e.name,
            accent: e.accent,
            size_mb: e.size_mb,
        })
        .collect()
}

/// Download a catalog voice (model and settings) into `voices_dir`.
pub fn download(id: &str, voices_dir: &Path, progress: impl Fn(u64, Option<u64>)) -> Result<String, String> {
    let c = catalog_data();
    let e = c.voices.iter().find(|v| v.id == id).ok_or_else(|| format!("Unknown voice {id}"))?;
    std::fs::create_dir_all(voices_dir).map_err(|e| e.to_string())?;
    let base = format!("{}/{}/{}", c.source, e.path, e.id);
    let model = voices_dir.join(format!("{id}.onnx"));
    let json = voices_dir.join(format!("{id}.onnx.json"));
    let agent = ureq::AgentBuilder::new().try_proxy_from_env(true).build();

    // The model goes last, so a voice only shows up once it's complete.
    let part = voices_dir.join(format!("{id}.onnx.part"));
    let res = (|| {
        fetch(&agent, &format!("{base}.onnx.json"), &json, &|_, _| {})?;
        fetch(&agent, &format!("{base}.onnx"), &part, &progress)?;
        std::fs::rename(&part, &model).map_err(|e| e.to_string())
    })();
    if let Err(err) = res {
        let _ = std::fs::remove_file(&part);
        return Err(format!("Could not download the {} voice: {err}", e.name));
    }
    Ok(model.display().to_string())
}

fn fetch(agent: &ureq::Agent, url: &str, to: &Path, progress: &dyn Fn(u64, Option<u64>)) -> Result<(), String> {
    let resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let total = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut received = 0u64;
    let mut last = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        if received - last >= 1 << 20 {
            last = received;
            progress(received, total);
        }
    }
    progress(received, total);
    if total.is_some_and(|t| t != received) {
        return Err("download was cut short".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_parses_and_includes_the_bundled_voice() {
        let c = catalog_data();
        assert!(c.voices.iter().any(|v| v.id == c.bundled));
        assert!(c.voices.iter().all(|v| v.path.ends_with(v.id.rsplit('-').next().unwrap())));
        let dir = std::env::temp_dir().join(format!("apexnorth-voices-{}", std::process::id()));
        let list = catalog(&dir, None);
        assert_eq!(list.len(), c.voices.len());
        assert!(list.iter().all(|v| v.path.is_none()));
    }
}
