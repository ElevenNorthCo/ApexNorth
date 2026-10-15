//! Desktop speech: the bundled voice helper (Kokoro or Piper, kept loaded in
//! a separate GPL process) or an external Piper CLI renders each phrase to a
//! cached WAV file, and rodio plays it.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::voice::{Backend, Playing};
use super::voice_helper::VoiceHelper;
use crate::settings::VoiceSettings;
use crate::voices::{self, VoiceRef};

/// Renders with [`Speaker`], plays with rodio.
pub struct WavBackend {
    speaker: Speaker,
    /// The audio device is opened lazily and kept for the life of the thread.
    audio: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
}

impl WavBackend {
    /// `builtin` is the bundled voice engine folder (`lib/`, `espeak-ng-data/`),
    /// when the app ships one.
    pub fn new(builtin: Option<PathBuf>, cache_dir: PathBuf) -> WavBackend {
        WavBackend { speaker: Speaker { builtin, cache_dir, helper: None, loaded: None }, audio: None }
    }
}

impl Backend for WavBackend {
    type Clip = PathBuf;

    fn load(&mut self, s: &VoiceSettings) -> Result<(), String> {
        self.speaker.load(s)
    }

    fn prepare(&mut self, s: &VoiceSettings, text: &str) -> Result<PathBuf, String> {
        self.speaker.render(s, text)
    }

    fn play(&mut self, wav: PathBuf, _s: &VoiceSettings) -> Result<Box<dyn Playing + '_>, String> {
        if self.audio.is_none() {
            self.audio = rodio::OutputStream::try_default().ok();
        }
        let Some((_, handle)) = &self.audio else {
            return Err("No audio output device found.".into());
        };
        let fail = |e: String| format!("Playback failed: {e}");
        let file = std::fs::File::open(&wav).map_err(|e| fail(e.to_string()))?;
        let source = rodio::Decoder::new(BufReader::new(file)).map_err(|e| fail(e.to_string()))?;
        let sink = rodio::Sink::try_new(handle).map_err(|e| fail(e.to_string()))?;
        sink.append(source);
        Ok(Box::new(SinkPlaying(sink)))
    }
}

struct SinkPlaying(rodio::Sink);

impl Playing for SinkPlaying {
    fn finished(&self) -> bool {
        self.0.empty()
    }
    fn stop(&mut self) {
        self.0.stop();
    }
    fn set_volume(&mut self, volume: f32) {
        self.0.set_volume(volume);
    }
}

/// Turns text into WAV files: the bundled voice helper with the voice kept
/// in memory, or an external Piper CLI when the app ships no helper.
struct Speaker {
    builtin: Option<PathBuf>,
    cache_dir: PathBuf,
    helper: Option<VoiceHelper>,
    /// Model path the helper has loaded.
    loaded: Option<String>,
}

impl Speaker {
    /// Make sure the helper is running with the selected voice loaded
    /// (bundled engine only). A helper that died is started again.
    fn load(&mut self, s: &VoiceSettings) -> Result<(), String> {
        let Some(dir) = &self.builtin else { return Ok(()) };
        if s.model_path.is_empty() {
            return Err("No voice selected. Choose a voice in Settings.".into());
        }
        if !self.helper.as_mut().is_some_and(|h| h.alive()) {
            self.helper = Some(VoiceHelper::start(dir)?);
            self.loaded = None;
        }
        if self.loaded.as_deref() == Some(s.model_path.as_str()) {
            return Ok(());
        }
        let h = self.helper.as_mut().expect("helper running");
        let cache = self.cache_dir.join("voices");
        match VoiceRef::parse(&s.model_path) {
            VoiceRef::Piper(model) => h.load(model, &cache)?,
            VoiceRef::Kokoro { voice, lang } => {
                let kokoro = voices::kokoro_dir(Some(dir)).ok_or("This build has no Kokoro voices. Choose another voice in Settings.")?;
                h.load_kokoro(&kokoro, voice, lang, &cache)?
            }
        }
        self.loaded = Some(s.model_path.clone());
        Ok(())
    }

    /// Render `text`, reusing a cached file when possible.
    fn render(&mut self, s: &VoiceSettings, text: &str) -> Result<PathBuf, String> {
        if self.builtin.is_none() {
            return synthesize_cli(s, &self.cache_dir, text);
        }
        if s.model_path.is_empty() {
            return Err("No voice selected. Choose a voice in Settings.".into());
        }
        let out = cache_path(&self.cache_dir, "builtin", s, text);
        if out.is_file() {
            return Ok(out);
        }
        self.load(s)?;
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| e.to_string())?;
        let tmp = temp_path(&out);
        let h = self.helper.as_mut().expect("helper running");
        if let Err(e) = h.speak(text, s.length_scale, &tmp) {
            let _ = std::fs::remove_file(&tmp);
            if !h.alive() {
                self.helper = None;
            }
            return Err(e);
        }
        std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

fn cache_path(cache_dir: &Path, engine: &str, s: &VoiceSettings, text: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    (engine, &s.model_path, s.length_scale.to_bits(), text).hash(&mut h);
    cache_dir.join(format!("{:016x}.wav", h.finish()))
}

fn temp_path(out: &Path) -> PathBuf {
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    out.with_extension(format!("{}.{}.tmp.wav", std::process::id(), NEXT_TMP.fetch_add(1, Ordering::Relaxed)))
}

/// Render `text` to a WAV file with an external Piper CLI.
fn synthesize_cli(s: &VoiceSettings, cache_dir: &Path, text: &str) -> Result<PathBuf, String> {
    if s.piper_path.is_empty() {
        return Err("Piper not found. Set the Piper path in Settings.".into());
    }
    if s.model_path.is_empty() {
        return Err("No voice model selected. Choose a Piper .onnx voice in Settings.".into());
    }
    if !matches!(VoiceRef::parse(&s.model_path), VoiceRef::Piper(_)) {
        return Err("Kokoro voices need the built-in voice engine. Choose a Piper .onnx voice in Settings.".into());
    }
    let out = cache_path(cache_dir, "cli", s, text);
    if out.is_file() {
        return Ok(out);
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let tmp = temp_path(&out);
    let mut child = Command::new(&s.piper_path)
        .arg("--model")
        .arg(&s.model_path)
        .arg("--length-scale")
        .arg(format!("{:.2}", s.length_scale))
        .arg("--output-file")
        .arg(&tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start Piper ({}): {e}", s.piper_path))?;
    child
        .stdin
        .take()
        .ok_or("no stdin")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if started.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&tmp);
            return Err("Piper timed out after 15 seconds.".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() || !tmp.is_file() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err.lines().last().unwrap_or("unknown error");
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Piper failed: {last}"));
    }
    if out.is_file() {
        let _ = std::fs::remove_file(&tmp);
        return Ok(out);
    }
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    Ok(out)
}
