//! Talks to `apexnorth-voice`, the separate voice program shipped with the
//! app (`src-tauri/voice-helper`). It loads the voice (Piper or Kokoro) once
//! and keeps it in memory; we send it text and it writes a WAV file. It runs as its own
//! process because it contains espeak-ng (GPL-3.0); a crash there never takes
//! the app down, and the next callout simply starts it again.

use crossbeam_channel::{Receiver, RecvTimeoutError};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "apexnorth-voice.exe"
    } else {
        "apexnorth-voice"
    }
}

/// Whether `dir` holds the voice helper and its engine files.
pub fn runtime_present(dir: &Path) -> bool {
    dir.join(exe_name()).is_file() && dir.join("lib").is_dir() && dir.join("espeak-ng-data").is_dir()
}

pub struct VoiceHelper {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<String>,
    /// Last lines the helper printed to stderr, for error messages.
    stderr: Arc<Mutex<VecDeque<String>>>,
}

impl VoiceHelper {
    pub fn start(dir: &Path) -> Result<VoiceHelper, String> {
        let exe = dir.join(exe_name());
        let mut cmd = Command::new(&exe);
        cmd.arg(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("Could not start the voice engine ({}): {e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("voice engine has no stdin")?;
        let stdout = child.stdout.take().ok_or("voice engine has no stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("voice engine has no stderr")?;

        let (tx, replies) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("voice-helper-out".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let log = stderr.clone();
        std::thread::Builder::new()
            .name("voice-helper-err".into())
            .spawn(move || {
                for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
                    log::warn!("voice engine: {line}");
                    let mut l = log.lock().unwrap();
                    l.push_back(line);
                    if l.len() > 5 {
                        l.pop_front();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(VoiceHelper { child, stdin, replies, stderr })
    }

    /// Whether the process is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    fn request(&mut self, req: Value, timeout: Duration) -> Result<Value, String> {
        writeln!(self.stdin, "{req}").and_then(|_| self.stdin.flush()).map_err(|_| self.died())?;
        let line = match self.replies.recv_timeout(timeout) {
            Ok(l) => l,
            Err(RecvTimeoutError::Timeout) => {
                let _ = self.child.kill();
                return Err("The voice engine stopped responding.".into());
            }
            Err(RecvTimeoutError::Disconnected) => return Err(self.died()),
        };
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("voice engine: {e}"))?;
        if v["ok"].as_bool() == Some(true) {
            Ok(v)
        } else {
            Err(v["error"].as_str().unwrap_or("voice engine error").to_string())
        }
    }

    fn died(&mut self) -> String {
        let _ = self.child.wait();
        // Give the stderr thread a moment to collect the last words.
        std::thread::sleep(Duration::from_millis(50));
        let last = self.stderr.lock().unwrap().back().cloned();
        match last {
            Some(l) => format!("The voice engine stopped: {l}"),
            None => "The voice engine stopped unexpectedly.".into(),
        }
    }

    /// Load a Piper voice (`.onnx` with its `.onnx.json`). The first load of
    /// a voice prepares a copy in `cache` and can take a few seconds.
    pub fn load(&mut self, model: &Path, cache: &Path) -> Result<(), String> {
        self.request(json!({ "op": "load", "model": model, "cache": cache }), Duration::from_secs(90)).map(|_| ())
    }

    /// Load a Kokoro speaker from the model in `dir`: a speaker name in the
    /// model or a voice file. Switching between speakers of the loaded model
    /// is instant; the first load takes a few seconds.
    pub fn load_kokoro(&mut self, dir: &Path, voice: &str, lang: &str, cache: &Path) -> Result<(), String> {
        let req = json!({ "op": "load", "engine": "kokoro", "model": dir, "voice": voice, "lang": lang, "cache": cache });
        self.request(req, Duration::from_secs(90)).map(|_| ())
    }

    /// Speak `text` into a WAV file at `out`. `length_scale` < 1 is faster.
    pub fn speak(&mut self, text: &str, length_scale: f32, out: &Path) -> Result<(), String> {
        self.request(json!({ "op": "speak", "text": text, "lengthScale": length_scale, "out": out }), Duration::from_secs(30)).map(|_| ())
    }
}

impl Drop for VoiceHelper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The bundled voice runtime for tests: `APEXNORTH_TTS_DIR`, else
/// `src-tauri/resources/tts` (filled by `scripts/fetch-voice.py`).
#[cfg(test)]
fn test_dir() -> std::path::PathBuf {
    std::env::var("APEXNORTH_TTS_DIR").map(Into::into).unwrap_or_else(|_| "resources/tts".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav_stats(path: &Path) -> (f32, f32) {
        let b = std::fs::read(path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        let rate = u32::from_le_bytes([b[24], b[25], b[26], b[27]]) as f32;
        let s: Vec<f32> = b[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
        (s.len() as f32 / rate, rms)
    }

    /// The real helper speaks every bundled Piper voice and the featured
    /// Kokoro voices (run in CI on each OS).
    #[test]
    #[ignore]
    fn speaks_with_the_bundled_voice() {
        let dir = test_dir();
        assert!(runtime_present(&dir), "no voice helper in {}", dir.display());
        let voices = crate::settings::find_voices(&[dir.join("voices")]);
        assert!(!voices.is_empty(), "no voices in {}", dir.display());
        let tmp = std::env::temp_dir().join(format!("apexnorth-voice-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let mut h = VoiceHelper::start(&dir).unwrap();
        for v in &voices {
            let t = std::time::Instant::now();
            h.load(Path::new(v), &tmp.join("cache")).unwrap();
            let loaded = t.elapsed();
            let t = std::time::Instant::now();
            let out = tmp.join("a.wav");
            h.speak("Box this lap. Pit window laps 5 to 7.", 1.0, &out).unwrap();
            let spoke = t.elapsed();
            let (secs, rms) = wav_stats(&out);
            println!("{v}: load {loaded:?}, phrase {spoke:?}, {secs:.2} s of audio, rms {rms:.3}");
            assert!(secs > 1.0 && secs < 8.0, "audio length {secs}");
            assert!(rms > 0.01, "silent audio");
            let fast = tmp.join("b.wav");
            h.speak("Box this lap. Pit window laps 5 to 7.", 0.8, &fast).unwrap();
            assert!(wav_stats(&fast).0 < secs, "length scale speeds it up");
        }
        // Every Kokoro voice in the catalog is in the bundled model, and the
        // featured ones speak (George with a British accent).
        let kokoro = crate::voices::kokoro_dir(Some(&dir)).expect("bundled Kokoro model");
        let all = crate::voices::options(&[], Some(&dir));
        for o in all.iter().filter(|o| o.group.starts_with("kokoro")) {
            let crate::voices::VoiceRef::Kokoro { voice, lang } = crate::voices::VoiceRef::parse(&o.value) else { panic!() };
            h.load_kokoro(&kokoro, voice, lang, &tmp.join("cache")).unwrap_or_else(|e| panic!("{voice}: {e}"));
            if o.group != "kokoro" {
                continue;
            }
            let t = std::time::Instant::now();
            let out = tmp.join("k.wav");
            h.speak("Box this lap. Pit window laps 5 to 7.", 1.0, &out).unwrap();
            let (secs, rms) = wav_stats(&out);
            println!("kokoro {voice} ({lang}): phrase {:?}, {secs:.2} s of audio, rms {rms:.3}", t.elapsed());
            assert!(secs > 1.0 && secs < 8.0 && rms > 0.01, "{voice}: {secs} s, rms {rms}");
        }
        // A voice file: a speaker cut out of the model's table.
        let table = std::fs::read(kokoro.join("voices.bin")).unwrap();
        let file = tmp.join("bf_test.bin");
        std::fs::write(&file, &table[..510 * 256 * 4]).unwrap();
        h.load_kokoro(&kokoro, &file.display().to_string(), "en-gb", &tmp.join("cache")).unwrap();
        h.speak("Radio check.", 1.0, &tmp.join("f.wav")).unwrap();
        assert!(wav_stats(&tmp.join("f.wav")).1 > 0.01);

        // A bad request gets an error, and the helper keeps running.
        assert!(h.load(Path::new("/no/such/voice.onnx"), &tmp).is_err());
        assert!(h.load_kokoro(&kokoro, "am_nobody", "en-us", &tmp).is_err());
        assert!(h.load_kokoro(&kokoro, "am_michael", "xx", &tmp).is_err());
        assert!(h.load_kokoro(&kokoro, &tmp.join("missing.pt").display().to_string(), "en-us", &tmp).is_err());
        assert!(h.alive());
        drop(h);
        let _ = std::fs::remove_dir_all(tmp);
    }
}
