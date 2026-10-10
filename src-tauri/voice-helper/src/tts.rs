//! Piper voices through sherpa-onnx (ONNX Runtime + espeak-ng), shipped as
//! shared libraries next to this helper and loaded at runtime. The voice
//! model stays in memory, so a new phrase takes a fraction of a second.
//!
//! Voices are ordinary Piper voices (`name.onnx` + `name.onnx.json`). sherpa
//! reads the voice settings from the model's ONNX metadata, which Piper models
//! don't carry, so each voice is prepared once into the cache: a copy of the
//! model with the metadata appended, plus a `tokens.txt` from the phoneme map.

use std::ffi::{c_char, c_void, CString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::OnceLock;

use libloading::Library;

// sherpa-onnx 1.12.9: the struct layouts below match that version (`scripts/fetch-voice.py`
// downloads the same one). Change both together.

#[repr(C)]
struct VitsConfig {
    model: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: f32,
    noise_scale_w: f32,
    length_scale: f32,
    dict_dir: *const c_char,
}

#[repr(C)]
struct MatchaConfig {
    acoustic_model: *const c_char,
    vocoder: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: f32,
    length_scale: f32,
    dict_dir: *const c_char,
}

#[repr(C)]
struct KokoroConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: f32,
    dict_dir: *const c_char,
    lexicon: *const c_char,
    lang: *const c_char,
}

#[repr(C)]
struct KittenConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: f32,
}

#[repr(C)]
struct ModelConfig {
    vits: VitsConfig,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    matcha: MatchaConfig,
    kokoro: KokoroConfig,
    kitten: KittenConfig,
}

#[repr(C)]
struct TtsConfig {
    model: ModelConfig,
    rule_fsts: *const c_char,
    max_num_sentences: i32,
    rule_fars: *const c_char,
    silence_scale: f32,
}

#[repr(C)]
struct GeneratedAudio {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}

type CreateFn = unsafe extern "C" fn(*const TtsConfig) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type GenerateFn = unsafe extern "C" fn(*const c_void, *const c_char, i32, f32) -> *const GeneratedAudio;
type FreeAudioFn = unsafe extern "C" fn(*const GeneratedAudio);

/// The loaded sherpa-onnx library. Loaded once and never unloaded.
pub struct Runtime {
    _libs: Vec<Library>,
    create: CreateFn,
    destroy: DestroyFn,
    generate: GenerateFn,
    free_audio: FreeAudioFn,
    espeak_data: PathBuf,
}

/// Library file names, dependency first.
fn lib_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["onnxruntime.dll", "sherpa-onnx-c-api.dll"]
    } else if cfg!(target_os = "macos") {
        &["libonnxruntime.1.17.1.dylib", "libsherpa-onnx-c-api.dylib"]
    } else {
        &["libonnxruntime.so", "libsherpa-onnx-c-api.so"]
    }
}

/// Whether `dir` holds the built-in voice runtime (libraries + espeak-ng data).
pub fn runtime_present(dir: &Path) -> bool {
    lib_names().iter().all(|n| dir.join("lib").join(n).is_file()) && dir.join("espeak-ng-data").is_dir()
}

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

impl Runtime {
    /// Load the runtime from `dir` (`lib/` and `espeak-ng-data/`). The first
    /// call wins; later calls return the same runtime.
    pub fn get(dir: &Path) -> Result<&'static Runtime, String> {
        RUNTIME.get_or_init(|| unsafe { Runtime::load(dir) }).as_ref().map_err(Clone::clone)
    }

    unsafe fn load(dir: &Path) -> Result<Runtime, String> {
        let mut libs = vec![];
        for name in lib_names() {
            let path = dir.join("lib").join(name);
            // Load the dependency by full path first, so the system's own copy
            // (Windows ships an onnxruntime.dll) is never picked up instead.
            #[cfg(windows)]
            let lib = libloading::os::windows::Library::load_with_flags(&path, libloading::os::windows::LOAD_WITH_ALTERED_SEARCH_PATH)
                .map(Library::from);
            #[cfg(not(windows))]
            let lib = Library::new(&path);
            libs.push(lib.map_err(|e| format!("Could not load the voice engine ({}): {e}", path.display()))?);
        }
        let api = libs.last().expect("sherpa library");
        let sym = |name: &[u8]| format!("Voice engine is missing {}", String::from_utf8_lossy(name));
        let create = *api.get::<CreateFn>(b"SherpaOnnxCreateOfflineTts\0").map_err(|_| sym(b"create"))?;
        let destroy = *api.get::<DestroyFn>(b"SherpaOnnxDestroyOfflineTts\0").map_err(|_| sym(b"destroy"))?;
        let generate = *api.get::<GenerateFn>(b"SherpaOnnxOfflineTtsGenerate\0").map_err(|_| sym(b"generate"))?;
        let free_audio = *api.get::<FreeAudioFn>(b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0").map_err(|_| sym(b"free"))?;
        Ok(Runtime { _libs: libs, create, destroy, generate, free_audio, espeak_data: dir.join("espeak-ng-data") })
    }
}

/// A Piper voice loaded in memory.
pub struct Synth {
    rt: &'static Runtime,
    tts: *const c_void,
    pub sample_rate: u32,
}

// The sherpa handle is only used from one thread at a time (the voice worker
// or a preview), never shared.
unsafe impl Send for Synth {}

impl Synth {
    /// Load the voice at `model` (a Piper `.onnx` with its `.onnx.json`),
    /// preparing a sherpa-ready copy in `cache_dir` the first time.
    pub fn load(rt: &'static Runtime, model: &Path, cache_dir: &Path) -> Result<Synth, String> {
        let voice = prepare(model, cache_dir)?;
        let c = |p: &Path| CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string());
        let model_c = c(&voice.model)?;
        let tokens_c = c(&voice.tokens)?;
        let data_c = c(&rt.espeak_data)?;
        let provider = CString::new("cpu").unwrap();
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4)) as i32;
        // SAFETY: a zeroed config is all null pointers and zeros, which
        // sherpa treats as "not set" for every model type we don't use.
        let mut cfg: TtsConfig = unsafe { std::mem::zeroed() };
        cfg.model.vits = VitsConfig {
            model: model_c.as_ptr(),
            lexicon: ptr::null(),
            tokens: tokens_c.as_ptr(),
            data_dir: data_c.as_ptr(),
            noise_scale: voice.noise_scale,
            noise_scale_w: voice.noise_w,
            length_scale: 1.0,
            dict_dir: ptr::null(),
        };
        cfg.model.num_threads = threads;
        cfg.model.provider = provider.as_ptr();
        cfg.max_num_sentences = 2;
        cfg.silence_scale = 0.2;
        let tts = unsafe { (rt.create)(&cfg) };
        if tts.is_null() {
            return Err(format!("The voice engine could not load {}.", model.display()));
        }
        Ok(Synth { rt, tts, sample_rate: voice.sample_rate })
    }

    /// Speak `text`. `length_scale` < 1 speaks faster (Piper's convention).
    pub fn speak(&mut self, text: &str, length_scale: f32) -> Result<Vec<f32>, String> {
        let text = CString::new(text.replace('\0', " ")).map_err(|e| e.to_string())?;
        let speed = 1.0 / length_scale.clamp(0.5, 2.0);
        unsafe {
            let audio = (self.rt.generate)(self.tts, text.as_ptr(), 0, speed);
            if audio.is_null() {
                return Err("The voice engine produced no audio.".into());
            }
            let a = &*audio;
            let samples = if a.samples.is_null() || a.n <= 0 { vec![] } else { std::slice::from_raw_parts(a.samples, a.n as usize).to_vec() };
            if a.sample_rate > 0 {
                self.sample_rate = a.sample_rate as u32;
            }
            (self.rt.free_audio)(audio);
            Ok(samples)
        }
    }
}

impl Drop for Synth {
    fn drop(&mut self) {
        unsafe { (self.rt.destroy)(self.tts) }
    }
}

struct Prepared {
    model: PathBuf,
    tokens: PathBuf,
    sample_rate: u32,
    noise_scale: f32,
    noise_w: f32,
}

/// Make (or reuse) the sherpa-ready copy of a Piper voice.
fn prepare(model: &Path, cache_dir: &Path) -> Result<Prepared, String> {
    let json_path = PathBuf::from(format!("{}.json", model.display()));
    let json = std::fs::read_to_string(&json_path).map_err(|_| format!("Voice settings file missing: {}", json_path.display()))?;
    let cfg: serde_json::Value = serde_json::from_str(&json).map_err(|e| format!("Bad voice settings in {}: {e}", json_path.display()))?;
    let sample_rate = cfg["audio"]["sample_rate"].as_u64().unwrap_or(22050) as u32;
    let f = |v: &serde_json::Value, d: f64| v.as_f64().unwrap_or(d) as f32;
    let noise_scale = f(&cfg["inference"]["noise_scale"], 0.667);
    let noise_w = f(&cfg["inference"]["noise_w"], 0.8);
    let map = cfg["phoneme_id_map"].as_object().ok_or("Voice settings have no phoneme map")?;

    let meta = std::fs::metadata(model).map_err(|e| format!("Voice model missing ({}): {e}", model.display()))?;
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&(model, meta.len(), stamp, &json), &mut h);
    let dir = cache_dir.join(format!("{:016x}", std::hash::Hasher::finish(&h)));
    let out = Prepared { model: dir.join("model.onnx"), tokens: dir.join("tokens.txt"), sample_rate, noise_scale, noise_w };
    if out.model.is_file() && out.tokens.is_file() {
        return Ok(out);
    }

    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut tokens = String::new();
    for (sym, ids) in map {
        // sherpa reads one character per token (and exits the process on
        // anything else); Piper's newer maps add multi-character diphthongs
        // that single-character voices never produce, so leave those out.
        if sym.chars().count() != 1 || sym == "\n" || sym == "\r" {
            continue;
        }
        if let Some(id) = ids.as_array().and_then(|a| a.first()).and_then(|v| v.as_i64()) {
            // One "symbol id" per line; the space symbol is written as " id".
            tokens.push_str(&format!("{sym} {id}\n"));
        }
    }
    let str_of = |v: &serde_json::Value, d: &str| v.as_str().unwrap_or(d).to_string();
    let props = [
        ("model_type", "vits".to_string()),
        ("comment", "piper".to_string()),
        ("language", str_of(&cfg["language"]["name_english"], "English")),
        ("voice", str_of(&cfg["espeak"]["voice"], "en-us")),
        ("has_espeak", "1".to_string()),
        ("n_speakers", cfg["num_speakers"].as_u64().unwrap_or(1).to_string()),
        ("sample_rate", sample_rate.to_string()),
    ];
    let tmp = dir.join("model.onnx.part");
    std::fs::copy(model, &tmp).map_err(|e| format!("Could not prepare the voice: {e}"))?;
    let mut file = std::fs::OpenOptions::new().append(true).open(&tmp).map_err(|e| e.to_string())?;
    file.write_all(&metadata_props(&props)).map_err(|e| e.to_string())?;
    drop(file);
    std::fs::write(&out.tokens, tokens).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &out.model).map_err(|e| e.to_string())?;
    Ok(out)
}

/// ONNX `ModelProto.metadata_props` entries (field 14) as protobuf bytes.
/// Fields may appear in any order in a protobuf message, so appending these
/// to a model file adds the metadata without rewriting the model.
fn metadata_props(props: &[(&str, String)]) -> Vec<u8> {
    fn varint(mut v: usize, out: &mut Vec<u8>) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
    fn field(tag: u8, bytes: &[u8], out: &mut Vec<u8>) {
        out.push(tag);
        varint(bytes.len(), out);
        out.extend_from_slice(bytes);
    }
    let mut out = vec![];
    for (k, v) in props {
        let mut entry = vec![];
        field(0x0a, k.as_bytes(), &mut entry); // key = 1
        field(0x12, v.as_bytes(), &mut entry); // value = 2
        field(0x72, &entry, &mut out); // metadata_props = 14
    }
    out
}

/// 16-bit mono WAV bytes.
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_is_valid_protobuf() {
        let b = metadata_props(&[("sample_rate", "22050".into())]);
        assert_eq!(b[0], 0x72);
        assert_eq!(b[1] as usize, b.len() - 2);
        assert_eq!(&b[2..4], &[0x0a, 11]);
        assert_eq!(&b[4..15], b"sample_rate");
        assert_eq!(&b[15..17], &[0x12, 5]);
    }

    #[test]
    fn wav_header() {
        let w = wav_bytes(&[0.0, 1.0, -1.0], 22050);
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
    }
}
