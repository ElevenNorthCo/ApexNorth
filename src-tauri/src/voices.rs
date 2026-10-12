//! Where voices come from: the bundled engine folder (Piper's Ryan and the
//! Kokoro model with its speakers), the small catalog of extra Piper voices
//! that can be downloaded from Settings, and voice files the user adds.
//!
//! The chosen voice is one string (`VoiceSettings::model_path`): a Piper
//! `.onnx` path, `kokoro:<speaker>` for a speaker in the bundled Kokoro model,
//! or the path of a Kokoro voice file (`.pt`, `.npy`, `.bin`).

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::engineer::voice_helper;

/// The same list `scripts/fetch-voice.py` bundles from.
const CATALOG: &str = include_str!("../voices.json");

#[derive(Deserialize)]
struct Catalog {
    source: String,
    bundled: String,
    voices: Vec<Entry>,
    kokoro: KokoroCatalog,
}

#[derive(Deserialize)]
struct KokoroCatalog {
    default: String,
    voices: Vec<KokoroEntry>,
}

#[derive(Deserialize)]
struct KokoroEntry {
    id: String,
    name: String,
    accent: String,
    gender: String,
    #[serde(default)]
    featured: bool,
}

const KOKORO_PREFIX: &str = "kokoro:";
/// One speaker's style table in the Kokoro model: 510 × 256 float32.
const KOKORO_STYLE_BYTES: u64 = 510 * 256 * 4;
const KOKORO_FILE_EXTS: &[&str] = &["pt", "pth", "npy", "bin"];

/// What a `model_path` setting points at.
#[derive(Debug, PartialEq)]
pub enum VoiceRef<'a> {
    Piper(&'a Path),
    /// `voice` is a speaker name in the bundled model, or a voice file path.
    Kokoro { voice: &'a str, lang: &'static str },
}

impl<'a> VoiceRef<'a> {
    pub fn parse(model_path: &'a str) -> VoiceRef<'a> {
        if let Some(name) = model_path.strip_prefix(KOKORO_PREFIX) {
            return VoiceRef::Kokoro { voice: name, lang: kokoro_lang(name) };
        }
        let path = Path::new(model_path);
        if is_kokoro_file(path) {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            return VoiceRef::Kokoro { voice: model_path, lang: kokoro_lang(&stem) };
        }
        VoiceRef::Piper(path)
    }
}

fn is_kokoro_file(p: &Path) -> bool {
    p.extension().is_some_and(|x| KOKORO_FILE_EXTS.contains(&x.to_string_lossy().to_lowercase().as_str()))
}

/// Kokoro names its voices `<accent><gender>_<name>`: `b` is British English,
/// everything else is spoken as American English.
fn kokoro_lang(name: &str) -> &'static str {
    let mut c = name.chars();
    match (c.next(), c.next(), c.next()) {
        (Some('b'), Some('f' | 'm'), Some('_')) => "en-gb",
        _ => "en-us",
    }
}

/// The bundled Kokoro model folder, if this build ships one.
pub fn kokoro_dir(builtin: Option<&Path>) -> Option<PathBuf> {
    let d = builtin?.join("kokoro");
    (d.join("voices.bin").is_file() && ["model.onnx", "model.int8.onnx"].iter().any(|m| d.join(m).is_file())).then_some(d)
}

/// The voice a new install starts with: Kokoro when it's bundled, else Ryan.
pub fn default_voice(builtin: &Path) -> Option<String> {
    if kokoro_dir(Some(builtin)).is_some() {
        return Some(format!("{KOKORO_PREFIX}{}", catalog_data().kokoro.default));
    }
    bundled_voice(builtin)
}

/// Whether the chosen voice can be spoken.
pub fn voice_available(model_path: &str, builtin: Option<&Path>) -> bool {
    match VoiceRef::parse(model_path) {
        VoiceRef::Kokoro { voice, .. } if !model_path.starts_with(KOKORO_PREFIX) => kokoro_dir(builtin).is_some() && Path::new(voice).is_file(),
        VoiceRef::Kokoro { voice, .. } => kokoro_dir(builtin).is_some() && catalog_data().kokoro.voices.iter().any(|v| v.id == voice),
        VoiceRef::Piper(p) => p.is_file(),
    }
}

/// A voice to choose in Settings.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceOption {
    /// The `model_path` setting for this voice.
    pub value: String,
    pub label: String,
    /// "kokoro", "kokoroMore", "custom" or "piper".
    pub group: &'static str,
}

/// Every voice that can be chosen right now: the bundled Kokoro speakers,
/// Kokoro voice files and Piper voices in `dirs` (the voices folder first).
pub fn options(dirs: &[PathBuf], builtin: Option<&Path>) -> Vec<VoiceOption> {
    let c = catalog_data();
    let mut out = vec![];
    if kokoro_dir(builtin).is_some() {
        for v in &c.kokoro.voices {
            out.push(VoiceOption {
                value: format!("{KOKORO_PREFIX}{}", v.id),
                label: format!("{} ({} {})", v.name, v.accent, v.gender),
                group: if v.featured { "kokoro" } else { "kokoroMore" },
            });
        }
        for p in kokoro_files(dirs) {
            let label = p.file_stem().map_or_else(|| p.display().to_string(), |s| s.to_string_lossy().into_owned());
            out.push(VoiceOption { value: p.display().to_string(), label, group: "custom" });
        }
    }
    for p in crate::settings::find_voices(dirs) {
        let id = Path::new(&p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let label = c.voices.iter().find(|v| v.id == id).map_or(id, |v| format!("{} ({})", v.name, v.accent));
        out.push(VoiceOption { value: p, label, group: "piper" });
    }
    out
}

/// Kokoro voice files in `dirs`. A `.bin` must be exactly one style table,
/// so other binaries don't show up as voices.
fn kokoro_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|e| e.flatten().map(|e| e.path()))
        .filter(|p| is_kokoro_file(p))
        .filter(|p| p.extension().is_some_and(|x| x != "bin") || std::fs::metadata(p).is_ok_and(|m| m.len() == KOKORO_STYLE_BYTES))
        .collect();
    out.sort();
    out.dedup();
    out
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
    candidates.into_iter().find(|d| voice_helper::runtime_present(d))
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

    #[test]
    fn three_featured_kokoro_voices_one_of_them_british() {
        let k = catalog_data().kokoro;
        let featured: Vec<_> = k.voices.iter().filter(|v| v.featured).collect();
        assert_eq!(featured.len(), 3);
        assert!(featured.iter().any(|v| v.id == k.default));
        assert!(featured.iter().any(|v| kokoro_lang(&v.id) == "en-gb"));
        assert!(featured.iter().any(|v| v.gender == "female"));
        for v in &k.voices {
            assert_eq!(kokoro_lang(&v.id), if v.accent == "British" { "en-gb" } else { "en-us" }, "{}", v.id);
        }
    }

    #[test]
    fn voice_refs() {
        assert_eq!(VoiceRef::parse("kokoro:bm_george"), VoiceRef::Kokoro { voice: "bm_george", lang: "en-gb" });
        assert_eq!(VoiceRef::parse("/v/af_bella.pt"), VoiceRef::Kokoro { voice: "/v/af_bella.pt", lang: "en-us" });
        assert_eq!(VoiceRef::parse("/v/bf_mine.NPY"), VoiceRef::Kokoro { voice: "/v/bf_mine.NPY", lang: "en-gb" });
        assert_eq!(VoiceRef::parse("/v/en_US-ryan-high.onnx"), VoiceRef::Piper(Path::new("/v/en_US-ryan-high.onnx")));
        assert!(!voice_available("kokoro:am_michael", None), "no Kokoro without the bundled engine");
    }

    #[test]
    fn lists_kokoro_voices_and_voice_files() {
        let dir = std::env::temp_dir().join(format!("apexnorth-kokoro-{}", std::process::id()));
        let tts = dir.join("tts");
        std::fs::create_dir_all(tts.join("kokoro")).unwrap();
        std::fs::write(tts.join("kokoro/model.onnx"), b"").unwrap();
        std::fs::write(tts.join("kokoro/voices.bin"), b"").unwrap();
        let voices = dir.join("voices");
        std::fs::create_dir_all(&voices).unwrap();
        std::fs::write(voices.join("bf_mine.pt"), b"zip").unwrap();
        std::fs::write(voices.join("right.bin"), vec![0u8; KOKORO_STYLE_BYTES as usize]).unwrap();
        std::fs::write(voices.join("other.bin"), b"not a voice").unwrap();
        let list = options(&[voices.clone()], Some(&tts));
        assert_eq!(list.iter().filter(|o| o.group == "kokoro").count(), 3);
        let custom: Vec<_> = list.iter().filter(|o| o.group == "custom").map(|o| o.label.as_str()).collect();
        assert_eq!(custom, ["bf_mine", "right"]);
        assert_eq!(default_voice(&tts).as_deref(), Some("kokoro:am_michael"));
        assert!(voice_available("kokoro:af_heart", Some(&tts)));
        assert!(!voice_available("kokoro:af_nobody", Some(&tts)));
        assert!(voice_available(&voices.join("bf_mine.pt").display().to_string(), Some(&tts)));
        assert!(options(&[voices], None).iter().all(|o| o.group == "piper"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
