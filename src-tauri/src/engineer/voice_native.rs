//! Phone speech: the system's own offline text-to-speech (Android
//! TextToSpeech, iOS AVSpeechSynthesizer) speaks each callout. The bundled
//! Kokoro engine can't run there: it lives in a separate GPL program
//! (`apexnorth-voice`), and iOS can't start other programs. See
//! `docs/mobile.md` for the plan to bring Kokoro to phones.
//!
//! The platform calls sit behind [`NativeSpeech`] (implemented in `lib.rs`
//! with the `apexnorth-device` plugin), so the queueing, priorities and
//! interruptions in `voice.rs` are the same code as on desktop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::voice::{Backend, Playing};
use crate::settings::VoiceSettings;

/// `model_path` prefix for a system voice: `system:<voice id>`.
pub const SYSTEM_PREFIX: &str = "system:";

/// The platform's speech engine.
pub trait NativeSpeech: Send + Sync {
    /// Start the engine (if it isn't running) and wait until it's ready.
    fn load(&self) -> Result<(), String>;
    /// Start speaking, replacing anything being said. `rate` 1.0 is the
    /// engine's normal speed; `volume` is 0.0 to 1.0.
    fn speak(&self, utterance: &Utterance) -> Result<(), String>;
    /// Still speaking the last utterance.
    fn speaking(&self) -> bool;
    fn stop(&self);
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Utterance {
    pub text: String,
    /// A system voice id, or none for the best installed voice for `lang`.
    pub voice: Option<String>,
    /// BCP 47 language, "en-GB" or "en-US".
    pub lang: &'static str,
    pub rate: f32,
    pub volume: f32,
}

/// The system voice a `model_path` setting asks for, and its language.
/// Desktop voice names carry over by accent: Kokoro's British speakers
/// (`kokoro:b?_…`) pick a British system voice, others an American one; an
/// unset voice gets a British one, like the desktop default (Emma).
pub fn voice_for(model_path: &str) -> (Option<String>, &'static str) {
    if let Some(id) = model_path.strip_prefix(SYSTEM_PREFIX).filter(|id| !id.is_empty()) {
        let lang = if id.to_ascii_lowercase().contains("en-us") || id.to_ascii_lowercase().contains("en_us") { "en-US" } else { "en-GB" };
        return (Some(id.to_string()), lang);
    }
    match model_path.strip_prefix("kokoro:") {
        Some(name) if !name.starts_with('b') => (None, "en-US"),
        _ => (None, "en-GB"),
    }
}

/// Speech rate for the engine from Piper's length scale (< 1 speaks faster).
pub fn rate(length_scale: f32) -> f32 {
    (1.0 / length_scale.clamp(0.5, 2.0)).clamp(0.5, 2.0)
}

pub struct NativeBackend {
    tts: Arc<dyn NativeSpeech>,
}

impl NativeBackend {
    pub fn new(tts: Arc<dyn NativeSpeech>) -> NativeBackend {
        NativeBackend { tts }
    }
}

impl Backend for NativeBackend {
    type Clip = String;

    fn load(&mut self, _s: &VoiceSettings) -> Result<(), String> {
        self.tts.load()
    }

    fn prepare(&mut self, _s: &VoiceSettings, text: &str) -> Result<String, String> {
        self.tts.load()?;
        Ok(text.to_string())
    }

    fn play(&mut self, text: String, s: &VoiceSettings) -> Result<Box<dyn Playing + '_>, String> {
        let (voice, lang) = voice_for(&s.model_path);
        let rate = rate(s.length_scale);
        // A generous limit, so an engine that never reports "done" can't
        // hold up every later call.
        let deadline = Instant::now() + Duration::from_secs(4) + Duration::from_millis((text.chars().count() as f32 * 90.0 / rate) as u64);
        self.tts.speak(&Utterance { text, voice, lang, rate, volume: s.volume.clamp(0.0, 1.0) })?;
        Ok(Box::new(NativePlaying { tts: self.tts.as_ref(), stopped: false, deadline }))
    }
}

struct NativePlaying<'a> {
    tts: &'a dyn NativeSpeech,
    stopped: bool,
    deadline: Instant,
}

impl Playing for NativePlaying<'_> {
    fn finished(&self) -> bool {
        self.stopped || !self.tts.speaking() || Instant::now() > self.deadline
    }
    fn stop(&mut self) {
        self.tts.stop();
        self.stopped = true;
    }
    /// The volume is set per utterance; a change applies to the next call.
    fn set_volume(&mut self, _volume: f32) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engineer::voice::{Callout, Priority, Voice};
    use parking_lot::Mutex;

    /// Speaks each utterance for a fixed time and records what was said.
    struct FakeSpeech {
        said: Mutex<Vec<(Utterance, Instant)>>,
        until: Mutex<Option<Instant>>,
        stops: Mutex<u32>,
        each: Duration,
    }

    impl NativeSpeech for FakeSpeech {
        fn load(&self) -> Result<(), String> {
            Ok(())
        }
        fn speak(&self, u: &Utterance) -> Result<(), String> {
            self.said.lock().push((u.clone(), Instant::now()));
            *self.until.lock() = Some(Instant::now() + self.each);
            Ok(())
        }
        fn speaking(&self) -> bool {
            self.until.lock().is_some_and(|t| Instant::now() < t)
        }
        fn stop(&self) {
            *self.stops.lock() += 1;
            *self.until.lock() = None;
        }
    }

    fn fake(each_ms: u64) -> Arc<FakeSpeech> {
        Arc::new(FakeSpeech { said: Mutex::new(vec![]), until: Mutex::new(None), stops: Mutex::new(0), each: Duration::from_millis(each_ms) })
    }

    fn wait_for(f: impl Fn() -> bool) {
        let t = Instant::now();
        while !f() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn voices_carry_over_by_accent() {
        assert_eq!(voice_for(""), (None, "en-GB"));
        assert_eq!(voice_for("kokoro:bf_emma"), (None, "en-GB"));
        assert_eq!(voice_for("kokoro:am_michael"), (None, "en-US"));
        assert_eq!(voice_for("/voices/ryan.onnx"), (None, "en-GB"));
        assert_eq!(voice_for("system:en-us-x-iol-local"), (Some("en-us-x-iol-local".into()), "en-US"));
        assert_eq!(voice_for("system:com.apple.voice.compact.en-GB.Daniel"), (Some("com.apple.voice.compact.en-GB.Daniel".into()), "en-GB"));
        assert!((rate(0.9) - 1.111).abs() < 0.01);
    }

    #[test]
    fn speaks_through_the_shared_queue() {
        let tts = fake(60);
        let backend_tts: Arc<dyn NativeSpeech> = tts.clone();
        let mut settings = VoiceSettings::default();
        settings.model_path = "kokoro:am_michael".into();
        settings.volume = 0.5;
        let voice = Voice::start(settings, move || NativeBackend::new(backend_tts), |e| panic!("voice error: {e}"));
        voice.say(Callout::new(Priority::Normal, "Sector two, 4 tenths up."));
        voice.say(Callout::new(Priority::Normal, "Nice rhythm."));
        wait_for(|| tts.said.lock().len() == 2 && !tts.speaking());
        let said = tts.said.lock();
        assert_eq!(said.iter().map(|(u, _)| u.text.as_str()).collect::<Vec<_>>(), ["Sector two, 4 tenths up.", "Nice rhythm."]);
        assert_eq!(said[0].0.lang, "en-US");
        assert_eq!(said[0].0.volume, 0.5);
        // The second call waits for the first to finish.
        assert!(said[1].1 - said[0].1 >= Duration::from_millis(55));
    }

    #[test]
    fn urgent_call_cuts_off_chatter() {
        let tts = fake(2_000);
        let backend_tts: Arc<dyn NativeSpeech> = tts.clone();
        let voice = Voice::start(VoiceSettings::default(), move || NativeBackend::new(backend_tts), |e| panic!("voice error: {e}"));
        voice.say(Callout::new(Priority::Low, "Good pace."));
        wait_for(|| tts.speaking());
        voice.say(Callout::new(Priority::High, "Box, box. Box this lap."));
        wait_for(|| tts.said.lock().len() == 2);
        assert_eq!(tts.said.lock()[1].0.text, "Box, box. Box this lap.");
        assert!(*tts.stops.lock() >= 1, "chatter was stopped");
    }
}
