//! Speech output: Piper CLI → cached WAV → rodio playback, on one worker
//! thread with a small priority queue that drops stale messages.

use crossbeam_channel::{Receiver, Sender};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, atomic::{AtomicI16, AtomicU64, Ordering}};
use std::time::{Duration, Instant};

use crate::settings::VoiceSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
}

impl Priority {
    fn max_age(self) -> Duration {
        match self {
            Priority::Low => Duration::from_secs(5),
            Priority::Normal => Duration::from_secs(10),
            Priority::High => Duration::from_secs(20),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Callout {
    pub text: String,
    pub priority: Priority,
    pub at: Instant,
    pub expected_position: Option<i16>,
    /// Once playing, a protected call is never cut off by a higher priority
    /// one (fuel and pit news at the line); the newer call waits its turn.
    pub protected: bool,
    /// Survives the session ending (the flag falls just before the game resets).
    pub outlives_session: bool,
    generation: u64,
}

impl Callout {
    pub fn new(priority: Priority, text: impl Into<String>) -> Callout {
        Callout { text: text.into(), priority, at: Instant::now(), expected_position: None, protected: false, outlives_session: false, generation: 0 }
    }
}

enum Msg {
    Say(Callout),
    Configure(VoiceSettings),
}

#[derive(Clone)]
pub struct Voice {
    tx: Sender<Msg>,
    context: Arc<Context>,
}

#[derive(Default)]
struct Context {
    position: AtomicI16,
    generation: AtomicU64,
}

impl Context {
    /// Still about this session and, for position calls, this position.
    fn relevant(&self, c: &Callout) -> bool {
        (c.outlives_session || c.generation == self.generation.load(Ordering::Acquire))
            && c.expected_position.map_or(true, |p| p == self.position.load(Ordering::Acquire))
    }

    /// Relevant and not too old to be worth saying. Age only matters before
    /// playback starts: a long call must never be cut off for being "stale".
    fn current(&self, c: &Callout) -> bool {
        self.relevant(c) && c.at.elapsed() < c.priority.max_age()
    }
}

impl Voice {
    /// `on_error` is called when a callout cannot be spoken, so the UI can say why.
    pub fn start(settings: VoiceSettings, cache_dir: PathBuf, on_error: impl Fn(String) + Send + 'static) -> Voice {
        let (tx, rx) = crossbeam_channel::unbounded();
        let context = Arc::new(Context::default());
        let worker_context = context.clone();
        std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || worker(rx, settings, cache_dir, worker_context, on_error))
            .expect("spawn voice thread");
        Voice { tx, context }
    }

    pub fn say(&self, mut c: Callout) {
        c.generation = self.context.generation.load(Ordering::Acquire);
        let _ = self.tx.send(Msg::Say(c));
    }

    pub fn update_position(&self, position: Option<i16>) {
        self.context.position.store(position.unwrap_or(0), Ordering::Release);
    }

    /// Queued and playing calls from the previous session must not leak through.
    pub fn clear(&self) {
        self.context.generation.fetch_add(1, Ordering::AcqRel);
        self.update_position(None);
    }

    pub fn configure(&self, s: VoiceSettings) {
        let _ = self.tx.send(Msg::Configure(s));
    }
}

/// Render `text` to a WAV file with Piper, reusing a cached file when possible.
pub fn synthesize(s: &VoiceSettings, cache_dir: &Path, text: &str) -> Result<PathBuf, String> {
    if s.piper_path.is_empty() {
        return Err("Piper not found. Set the Piper path in Settings.".into());
    }
    if s.model_path.is_empty() {
        return Err("No voice model selected. Choose a Piper .onnx voice in Settings.".into());
    }
    let mut h = DefaultHasher::new();
    (&s.model_path, s.length_scale.to_bits(), text).hash(&mut h);
    let out = cache_dir.join(format!("{:016x}.wav", h.finish()));
    if out.is_file() {
        return Ok(out);
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    // Voice previews and the worker can synthesize the same phrase concurrently.
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    let tmp = out.with_extension(format!("{}.{}.tmp.wav", std::process::id(), NEXT_TMP.fetch_add(1, Ordering::Relaxed)));
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

/// Queue a callout. Urgent news clears queued chatter, duplicates are
/// dropped, and routine calls don't pile up behind a backlog.
fn enqueue(queue: &mut Vec<Callout>, c: Callout) {
    if c.expected_position.is_some() {
        queue.retain(|q| q.expected_position.is_none());
    }
    if queue.iter().any(|q| q.text == c.text) {
        return;
    }
    match c.priority {
        Priority::High => queue.retain(|q| q.priority != Priority::Low),
        Priority::Low if queue.len() >= 2 => return,
        _ => {}
    }
    queue.push(c);
}

fn worker(rx: Receiver<Msg>, mut settings: VoiceSettings, cache_dir: PathBuf, context: Arc<Context>, on_error: impl Fn(String)) {
    // The audio device is opened lazily and kept for the life of the thread.
    let mut audio: Option<(rodio::OutputStream, rodio::OutputStreamHandle)> = None;
    let mut queue: Vec<Callout> = Vec::new();
    loop {
        let msg = if queue.is_empty() {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        } else {
            rx.try_recv().ok()
        };
        match msg {
            Some(Msg::Configure(s)) => {
                settings = s;
                continue;
            }
            Some(Msg::Say(c)) => {
                enqueue(&mut queue, c);
                // Drain anything else waiting so priorities apply.
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Msg::Say(c) => enqueue(&mut queue, c),
                        Msg::Configure(s) => settings = s,
                    }
                }
            }
            None => {}
        }
        queue.retain(|c| context.current(c));
        // Highest priority first, oldest first within a priority.
        queue.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.at.cmp(&b.at)));
        if queue.is_empty() {
            continue;
        }
        let c = queue.remove(0);
        if !settings.enabled {
            queue.clear();
            continue;
        }
        let wav = match synthesize(&settings, &cache_dir, &c.text) {
            Ok(w) => w,
            Err(e) => {
                on_error(e);
                continue;
            }
        };
        // Recheck after synthesis: passes and urgent news can happen while
        // Piper loads the model. Never play an obsolete position afterward.
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Say(next) => enqueue(&mut queue, next),
                Msg::Configure(s) => settings = s,
            }
        }
        queue.retain(|q| context.current(q));
        if !settings.enabled || !context.current(&c) {
            continue;
        }
        if queue.iter().any(|q| q.priority > c.priority) {
            enqueue(&mut queue, c);
            continue;
        }
        if audio.is_none() {
            audio = rodio::OutputStream::try_default().ok();
        }
        let Some((_, handle)) = &audio else {
            on_error("No audio output device found.".into());
            continue;
        };
        if let Err(e) = play(handle, &wav, &c, &rx, &mut settings, &mut queue, &context) {
            on_error(format!("Playback failed: {e}"));
        }
    }
}

fn play(handle: &rodio::OutputStreamHandle, wav: &Path, call: &Callout, rx: &Receiver<Msg>, settings: &mut VoiceSettings, queue: &mut Vec<Callout>, context: &Context) -> Result<(), String> {
    let file = std::fs::File::open(wav).map_err(|e| e.to_string())?;
    let source = rodio::Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?;
    let sink = rodio::Sink::try_new(handle).map_err(|e| e.to_string())?;
    sink.set_volume(settings.volume.clamp(0.0, 1.0));
    sink.append(source);
    while !sink.empty() {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(Msg::Say(c)) => {
                let interrupt = !call.protected && c.priority > call.priority && context.current(&c);
                enqueue(queue, c);
                if interrupt {
                    sink.stop();
                    break;
                }
            }
            Ok(Msg::Configure(s)) => {
                *settings = s;
                sink.set_volume(settings.volume.clamp(0.0, 1.0));
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                sink.stop();
                break;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if !settings.enabled || !context.relevant(call) {
            sink.stop();
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_calls_expire_when_position_or_session_changes() {
        let context = Context::default();
        context.position.store(3, Ordering::Release);
        let mut c = Callout::new(Priority::Normal, "P3.");
        c.expected_position = Some(3);
        assert!(context.current(&c));
        context.position.store(2, Ordering::Release);
        assert!(!context.current(&c));
        context.position.store(3, Ordering::Release);
        context.generation.fetch_add(1, Ordering::AcqRel);
        assert!(!context.current(&c));
    }

    #[test]
    fn high_priority_supersedes_chatter() {
        let mut q = vec![];
        enqueue(&mut q, Callout::new(Priority::Low, "Nice rhythm."));
        enqueue(&mut q, Callout::new(Priority::Normal, "Sector two, 4 tenths up."));
        enqueue(&mut q, Callout::new(Priority::Normal, "Sector two, 4 tenths up."));
        assert_eq!(q.len(), 2);
        enqueue(&mut q, Callout::new(Priority::High, "Box, box. Box this lap."));
        assert_eq!(q.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Sector two, 4 tenths up.", "Box, box. Box this lap."]);
        enqueue(&mut q, Callout::new(Priority::Low, "Good pace."));
        assert_eq!(q.len(), 2, "routine call dropped behind a backlog");
    }
}