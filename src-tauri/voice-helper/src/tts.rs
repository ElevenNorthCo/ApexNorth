//! Piper and Kokoro voices through sherpa-onnx (ONNX Runtime + espeak-ng),
//! shipped as shared libraries next to this helper and loaded at runtime. The
//! voice model stays in memory, so a new phrase takes a fraction of a second.
//!
//! Piper voices are ordinary Piper voices (`name.onnx` + `name.onnx.json`).
//! sherpa reads the voice settings from the model's ONNX metadata, which Piper
//! models don't carry, so each voice is prepared once into the cache: a copy
//! of the model with the metadata appended, plus a `tokens.txt` from the
//! phoneme map.
//!
//! Kokoro is one model with many speakers (`voices.bin`, one style table per
//! speaker). A speaker is picked by name, or a voice file (`.pt`, `.npy` or a
//! raw `.bin` style table) is swapped into slot 0 of a cached copy.
//!
//! sherpa calls `exit()` on many bad inputs instead of returning an error, so
//! every file is checked here before it gets them.

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CString};
use std::io::{BufReader, Read, Seek, Write};
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

/// A voice ready to load, with every file checked (see the module docs).
pub enum Spec {
    Piper(Prepared),
    Kokoro(Kokoro),
}

impl Spec {
    /// What the sherpa handle holds. Two specs with the same key differ only
    /// in the speaker, which needs no reload.
    pub fn key(&self) -> String {
        match self {
            Spec::Piper(p) => format!("piper|{}", p.model.display()),
            Spec::Kokoro(k) => format!("kokoro|{}|{}|{}", k.model.display(), k.voices.display(), k.lang),
        }
    }

    pub fn sid(&self) -> i32 {
        match self {
            Spec::Piper(_) => 0,
            Spec::Kokoro(k) => k.sid,
        }
    }
}

/// A voice loaded in memory.
pub struct Synth {
    rt: &'static Runtime,
    tts: *const c_void,
    pub sample_rate: u32,
    /// Speaker within the model (Kokoro); 0 for Piper.
    pub sid: i32,
    pub key: String,
}

// The sherpa handle is only used from one thread at a time (the voice worker
// or a preview), never shared.
unsafe impl Send for Synth {}

impl Synth {
    pub fn load(rt: &'static Runtime, spec: &Spec) -> Result<Synth, String> {
        let c = |p: &Path| CString::new(p.to_string_lossy().as_bytes()).map_err(|e| e.to_string());
        let data_c = c(&rt.espeak_data)?;
        let provider = CString::new("cpu").unwrap();
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4)) as i32;
        // SAFETY: a zeroed config is all null pointers and zeros, which
        // sherpa treats as "not set" for every model type we don't use.
        let mut cfg: TtsConfig = unsafe { std::mem::zeroed() };
        // Kept alive until the handle is created.
        let mut strings = vec![];
        // Kokoro takes one sentence at a time (and warns about anything else).
        cfg.max_num_sentences = if matches!(spec, Spec::Kokoro(_)) { 1 } else { 2 };
        let (sample_rate, what) = match spec {
            Spec::Piper(voice) => {
                let (model_c, tokens_c) = (c(&voice.model)?, c(&voice.tokens)?);
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
                strings.extend([model_c, tokens_c]);
                (voice.sample_rate, voice.source.clone())
            }
            Spec::Kokoro(k) => {
                let (model_c, voices_c, tokens_c, dict_c) = (c(&k.model)?, c(&k.voices)?, c(&k.tokens)?, c(&k.dict)?);
                let lang_c = CString::new(k.lang.as_str()).map_err(|e| e.to_string())?;
                cfg.model.kokoro = KokoroConfig {
                    model: model_c.as_ptr(),
                    voices: voices_c.as_ptr(),
                    tokens: tokens_c.as_ptr(),
                    data_dir: data_c.as_ptr(),
                    length_scale: 1.0,
                    dict_dir: dict_c.as_ptr(),
                    lexicon: ptr::null(),
                    // English goes through espeak-ng with this accent.
                    lang: lang_c.as_ptr(),
                };
                strings.extend([model_c, voices_c, tokens_c, dict_c, lang_c]);
                (k.sample_rate, k.model.clone())
            }
        };
        cfg.model.num_threads = threads;
        cfg.model.provider = provider.as_ptr();
        cfg.silence_scale = 0.2;
        let tts = unsafe { (rt.create)(&cfg) };
        drop(strings);
        if tts.is_null() {
            return Err(format!("The voice engine could not load {}.", what.display()));
        }
        Ok(Synth { rt, tts, sample_rate, sid: spec.sid(), key: spec.key() })
    }

    /// Speak `text`. `length_scale` < 1 speaks faster (Piper's convention).
    pub fn speak(&mut self, text: &str, length_scale: f32) -> Result<Vec<f32>, String> {
        let text = CString::new(text.replace('\0', " ")).map_err(|e| e.to_string())?;
        let speed = 1.0 / length_scale.clamp(0.5, 2.0);
        unsafe {
            let audio = (self.rt.generate)(self.tts, text.as_ptr(), self.sid, speed);
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

pub struct Prepared {
    /// The voice as the user chose it, for error messages.
    source: PathBuf,
    model: PathBuf,
    tokens: PathBuf,
    sample_rate: u32,
    noise_scale: f32,
    noise_w: f32,
}

/// Make (or reuse) the sherpa-ready copy of a Piper voice.
pub fn prepare_piper(model: &Path, cache_dir: &Path) -> Result<Spec, String> {
    prepare(model, cache_dir).map(Spec::Piper)
}

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
    let out = Prepared { source: model.to_path_buf(), model: dir.join("model.onnx"), tokens: dir.join("tokens.txt"), sample_rate, noise_scale, noise_w };
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

pub struct Kokoro {
    model: PathBuf,
    voices: PathBuf,
    tokens: PathBuf,
    dict: PathBuf,
    lang: String,
    sid: i32,
    sample_rate: u32,
}

/// Accents the English Kokoro voices are spoken with, and the espeak-ng
/// voice for each (its British English voice is plain "en"; an unknown name
/// throws a C++ exception that takes the helper down).
const KOKORO_LANGS: &[(&str, &str)] = &[("en-us", "en-us"), ("en-gb", "en")];

/// Files sherpa's Kokoro loader insists on in `dict/` (jieba, for Chinese
/// text; English never uses them, but the loader exits without them).
const KOKORO_DICT: &[&str] = &["jieba.dict.utf8", "hmm_model.utf8", "user.dict.utf8", "idf.utf8", "stop_words.utf8"];

/// Check the Kokoro model in `dir` and pick a speaker. `voice` is a speaker
/// name from the model (`am_michael`) or a voice file; a voice file is
/// copied into slot 0 of a speaker table in `cache_dir`.
pub fn prepare_kokoro(dir: &Path, voice: &str, lang: &str, cache_dir: &Path) -> Result<Spec, String> {
    let lang = KOKORO_LANGS.iter().find(|(l, _)| *l == lang).map(|(_, espeak)| *espeak).ok_or_else(|| format!("Unknown Kokoro accent {lang}"))?;
    let model = ["model.onnx", "model.int8.onnx"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("Kokoro model missing in {}", dir.display()))?;
    let (tokens, dict, stock) = (dir.join("tokens.txt"), dir.join("dict"), dir.join("voices.bin"));
    check_tokens(&tokens)?;
    if let Some(f) = KOKORO_DICT.iter().find(|f| !dict.join(f).is_file()) {
        return Err(format!("Kokoro file missing: {}", dict.join(f).display()));
    }

    let meta = onnx_metadata(&model)?;
    if meta.get("model_type").map(String::as_str) != Some("kokoro") {
        return Err(format!("{} is not a Kokoro model", model.display()));
    }
    let dims: Vec<usize> = meta.get("style_dim").map_or(vec![], |d| d.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let n_speakers: usize = meta.get("n_speakers").and_then(|n| n.parse().ok()).unwrap_or(0);
    if dims.len() != 3 || dims[1] != 1 || n_speakers == 0 {
        return Err(format!("{} has no Kokoro speaker settings", model.display()));
    }
    let style_bytes = dims[0] * dims[2] * 4;
    let table_len = std::fs::metadata(&stock).map_err(|_| format!("Kokoro file missing: {}", stock.display()))?.len();
    if table_len != (style_bytes * n_speakers) as u64 {
        return Err(format!("{} doesn't match the Kokoro model", stock.display()));
    }
    let sample_rate = meta.get("sample_rate").and_then(|r| r.parse().ok()).unwrap_or(24000);

    let (voices, sid) = if is_voice_file(voice) {
        let style = read_style(Path::new(voice), style_bytes)?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&(&style, table_len, &stock), &mut h);
        let dir = cache_dir.join(format!("kokoro-{:016x}", std::hash::Hasher::finish(&h)));
        let out = dir.join("voices.bin");
        if std::fs::metadata(&out).map(|m| m.len()).ok() != Some(table_len) {
            let mut table = std::fs::read(&stock).map_err(|e| e.to_string())?;
            table[..style_bytes].copy_from_slice(&style);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let tmp = dir.join("voices.bin.part");
            std::fs::write(&tmp, table).map_err(|e| format!("Could not prepare the voice: {e}"))?;
            std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
        }
        (out, 0)
    } else {
        let ids = meta.get("speaker2id").map(String::as_str).unwrap_or_default();
        let sid = ids
            .split(',')
            .filter_map(|p| p.split_once("->"))
            .find(|(name, _)| name.trim() == voice)
            .and_then(|(_, id)| id.trim().parse::<i32>().ok())
            .filter(|&id| id >= 0 && (id as usize) < n_speakers)
            .ok_or_else(|| format!("The Kokoro model has no voice called {voice}"))?;
        (stock, sid)
    };
    Ok(Spec::Kokoro(Kokoro { model, voices, tokens, dict, lang: lang.to_string(), sid, sample_rate }))
}

/// A voice file rather than a speaker name.
pub fn is_voice_file(voice: &str) -> bool {
    voice.contains('/') || voice.contains('\\') || [".pt", ".pth", ".npy", ".bin"].iter().any(|x| voice.to_lowercase().ends_with(x))
}

/// sherpa reads `symbol id` lines and exits on a symbol that isn't one
/// character, so check the file first.
fn check_tokens(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|_| format!("Voice file missing: {}", path.display()))?;
    for line in text.lines().filter(|l| !l.is_empty()) {
        let (sym, id) = line.rsplit_once(' ').ok_or_else(|| format!("Bad line in {}: {line}", path.display()))?;
        if sym.chars().count() != 1 || id.parse::<i64>().is_err() {
            return Err(format!("Bad line in {}: {line}", path.display()));
        }
    }
    Ok(())
}

/// The `metadata_props` of an ONNX model, read without loading the graph.
fn onnx_metadata(path: &Path) -> Result<HashMap<String, String>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Voice model missing ({}): {e}", path.display()))?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let mut r = BufReader::new(file);
    let bad = || format!("{} is not an ONNX model", path.display());
    let mut out = HashMap::new();
    let mut pos = 0u64;
    while pos < len {
        let key = read_varint(&mut r).ok_or_else(bad)?;
        match key & 7 {
            0 => {
                read_varint(&mut r).ok_or_else(bad)?;
            }
            1 => r.seek_relative(8).map_err(|_| bad())?,
            5 => r.seek_relative(4).map_err(|_| bad())?,
            2 => {
                let n = read_varint(&mut r).ok_or_else(bad)?;
                if n > len {
                    return Err(bad());
                }
                if key >> 3 == 14 && n < 1 << 20 {
                    let mut entry = vec![0; n as usize];
                    r.read_exact(&mut entry).map_err(|_| bad())?;
                    let (mut k, mut v) = (String::new(), String::new());
                    let mut e = &entry[..];
                    while !e.is_empty() {
                        let tag = read_varint(&mut e).ok_or_else(bad)?;
                        let l = read_varint(&mut e).ok_or_else(bad)? as usize;
                        if tag & 7 != 2 || l > e.len() {
                            return Err(bad());
                        }
                        let text = String::from_utf8_lossy(&e[..l]).into_owned();
                        match tag >> 3 {
                            1 => k = text,
                            2 => v = text,
                            _ => {}
                        }
                        e = &e[l..];
                    }
                    out.entry(k).or_insert(v);
                } else {
                    r.seek_relative(n as i64).map_err(|_| bad())?;
                }
            }
            _ => return Err(bad()),
        }
        pos = r.stream_position().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

fn read_varint(r: &mut impl Read) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let mut b = [0u8];
        r.read_exact(&mut b).ok()?;
        v |= u64::from(b[0] & 0x7f) << shift;
        if b[0] < 0x80 {
            return Some(v);
        }
    }
    None
}

/// One Kokoro speaker's style table (`len` bytes of little-endian float32)
/// from a voice file: PyTorch `.pt` (as published with Kokoro), NumPy
/// `.npy`, or the raw floats (`.bin`).
pub fn read_style(path: &Path, len: usize) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read the voice file {}: {e}", path.display()))?;
    let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    let data = match ext.as_str() {
        "npy" => npy_data(&bytes),
        "pt" | "pth" => torch_data(&bytes, len),
        _ => Some(&bytes[..]),
    };
    match data {
        Some(d) if d.len() == len => Ok(d.to_vec()),
        _ => Err(format!("{} is not a Kokoro voice file", path.display())),
    }
}

/// The array data of a little-endian float32 NumPy file.
fn npy_data(b: &[u8]) -> Option<&[u8]> {
    if b.len() < 10 || &b[..6] != b"\x93NUMPY" {
        return None;
    }
    let (header_len, start) = match b[6] {
        1 => (u16::from_le_bytes([b[8], b[9]]) as usize, 10),
        2 | 3 => (u32::from_le_bytes(b.get(8..12)?.try_into().ok()?) as usize, 12),
        _ => return None,
    };
    let header = std::str::from_utf8(b.get(start..start + header_len)?).ok()?;
    let compact: String = header.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.contains("'descr':'<f4'") || !compact.contains("'fortran_order':False") {
        return None;
    }
    b.get(start + header_len..)
}

/// The tensor data in a `torch.save` file: a zip with each tensor's floats
/// stored uncompressed as `<name>/data/<n>`. Takes the one of `len` bytes.
fn torch_data(b: &[u8], len: usize) -> Option<&[u8]> {
    let u16_at = |i: usize| b.get(i..i + 2).map(|x| u16::from_le_bytes([x[0], x[1]]) as usize);
    let u32_at = |i: usize| b.get(i..i + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize);
    // End of central directory: the last record, before an optional comment.
    let eocd = (0..b.len().saturating_sub(21)).rev().take(1 << 16).find(|&i| u32_at(i) == Some(0x0605_4b50))?;
    let (count, mut at) = (u16_at(eocd + 10)?, u32_at(eocd + 16)?);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let (method, size) = (u16_at(at + 10)?, u32_at(at + 20)?);
        let (name_len, extra_len, comment_len) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        let local = u32_at(at + 42)?;
        let name = b.get(at + 46..at + 46 + name_len)?;
        if method == 0 && size == len && name.windows(6).any(|w| w == b"/data/") && u32_at(local)? == 0x0403_4b50 {
            let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
            return b.get(start..start + len);
        }
        at += 46 + name_len + extra_len + comment_len;
    }
    None
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

    fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("apexnorth-voice-helper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn style() -> Vec<u8> {
        (0..12u32).flat_map(|i| (i as f32 * 0.5).to_le_bytes()).collect()
    }

    #[test]
    fn reads_npy_voice() {
        let header = "{'descr': '<f4', 'fortran_order': False, 'shape': (3, 1, 4), }";
        let mut b = b"\x93NUMPY\x01\x00".to_vec();
        b.extend_from_slice(&(header.len() as u16).to_le_bytes());
        b.extend_from_slice(header.as_bytes());
        b.extend_from_slice(&style());
        assert_eq!(read_style(&tmp("v.npy", &b), 48).unwrap(), style());
        assert!(read_style(&tmp("v2.npy", &b), 40).is_err(), "wrong size");
        let f8: Vec<u8> = b.iter().map(|&c| if c == b'4' { b'8' } else { c }).collect();
        assert!(npy_data(&f8).is_none(), "float64 isn't a voice");
    }

    #[test]
    fn reads_torch_voice() {
        // A stored zip like torch.save writes: a pickle, then the tensor data.
        let mut zip = vec![];
        let mut central = vec![];
        let entries: [(&str, Vec<u8>); 2] = [("am_test/data.pkl", b"pickle".to_vec()), ("am_test/data/0", style())];
        for (name, data) in &entries {
            let offset = zip.len() as u32;
            let extra = [0u8; 6]; // torch pads to align the data
            zip.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            zip.extend_from_slice(&[0; 22]);
            zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
            zip.extend_from_slice(&(extra.len() as u16).to_le_bytes());
            zip.extend_from_slice(name.as_bytes());
            zip.extend_from_slice(&extra);
            zip.extend_from_slice(data);
            let mut c = 0x0201_4b50u32.to_le_bytes().to_vec();
            c.extend_from_slice(&[0; 6]);
            c.extend_from_slice(&0u16.to_le_bytes()); // stored
            c.extend_from_slice(&[0; 8]);
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(name.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 12]);
            c.extend_from_slice(&offset.to_le_bytes());
            c.extend_from_slice(name.as_bytes());
            central.extend(c);
        }
        let cd_offset = zip.len() as u32;
        zip.extend_from_slice(&central);
        zip.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        zip.extend_from_slice(&[0; 4]);
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&2u16.to_le_bytes());
        zip.extend_from_slice(&(central.len() as u32).to_le_bytes());
        zip.extend_from_slice(&cd_offset.to_le_bytes());
        zip.extend_from_slice(&[0; 2]);
        assert_eq!(read_style(&tmp("am_test.pt", &zip), 48).unwrap(), style());
        assert!(read_style(&tmp("other.pt", &zip), 96).is_err());
        assert!(read_style(&tmp("junk.pt", b"not a zip"), 48).is_err());
        assert_eq!(read_style(&tmp("raw.bin", &style()), 48).unwrap(), style());
    }

    #[test]
    fn reads_onnx_metadata_around_the_graph() {
        let mut model = vec![0x08, 0x07]; // ir_version = 7
        model.extend_from_slice(&[0x3a, 0x03, 1, 2, 3]); // graph (skipped)
        model.extend(metadata_props(&[("model_type", "kokoro".into()), ("n_speakers", "54".into())]));
        let m = onnx_metadata(&tmp("m.onnx", &model)).unwrap();
        assert_eq!(m["model_type"], "kokoro");
        assert_eq!(m["n_speakers"], "54");
        assert!(onnx_metadata(&tmp("bad.onnx", &[0x3a, 0x7f])).is_err());
    }

    #[test]
    fn token_files_are_checked() {
        assert!(check_tokens(&tmp("t1.txt", "; 1\n  16\nʣ 18\n".as_bytes())).is_ok());
        assert!(check_tokens(&tmp("t2.txt", b"ab 1\n")).is_err());
        assert!(is_voice_file("/x/af_bella.pt") && is_voice_file("my.npy") && !is_voice_file("am_michael"));
    }

    #[test]
    fn wav_header() {
        let w = wav_bytes(&[0.0, 1.0, -1.0], 22050);
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), 32767);
    }
}
