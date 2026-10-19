//! Speech output: one worker thread with a small priority queue that drops
//! stale messages, in front of a speech [`Backend`]. The backend is the only
//! part that differs by platform: on desktop the built-in Kokoro/Piper engine
//! (or a Piper CLI) renders cached WAV files that rodio plays
//! (`voice_wav.rs`); on phones the system's offline voice speaks
//! (`voice_native.rs`). What is said, and when, is decided here and in the
//! director, the same everywhere.

use crossbeam_channel::{Receiver, Sender};
use std::sync::{Arc, atomic::{AtomicI16, AtomicU64, Ordering}};
use std::time::{Duration, Instant};

use crate::settings::VoiceSettings;

/// Turns text into sound.
pub trait Backend {
    /// Something ready to be spoken: a rendered file, or just the text.
    type Clip;
    /// Get the selected voice ready, so the first callout is instant.
    fn load(&mut self, s: &VoiceSettings) -> Result<(), String>;
    /// Prepare `text` with the current voice (render it, or check the
    /// engine is ready). A preview uses this to report errors.
    fn prepare(&mut self, s: &VoiceSettings, text: &str) -> Result<Self::Clip, String>;
    /// Start speaking a prepared clip.
    fn play(&mut self, clip: Self::Clip, s: &VoiceSettings) -> Result<Box<dyn Playing + '_>, String>;
}

/// A callout being spoken.
pub trait Playing {
    fn finished(&self) -> bool;
    fn stop(&mut self);
    fn set_volume(&mut self, volume: f32);
}

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

/// How long a protected call (pit, fuel and tyre news) may wait in the queue:
/// long enough to get through everything said at the line.
const PROTECTED_MAX_AGE: Duration = Duration::from_secs(45);

#[derive(Debug, Clone)]
pub struct Callout {
    pub text: String,
    pub priority: Priority,
    pub at: Instant,
    pub expected_position: Option<i16>,
    /// Pit, fuel and tyre news (and everything said at the line): goes to
    /// the front of the queue, waits longer there, and once playing is never
    /// cut off, not even by the link dropping. Newer calls wait their turn.
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
    /// Render a phrase (for a preview) and report whether it worked.
    Render(String, Sender<Result<(), String>>),
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
        let max_age = if c.protected { PROTECTED_MAX_AGE } else { c.priority.max_age() };
        self.relevant(c) && c.at.elapsed() < max_age
    }

    /// Whether a call being spoken has to stop: only when what it says is no
    /// longer true (a position that has already changed), or the session it
    /// was about is gone. Protected calls always finish.
    fn obsolete(&self, c: &Callout) -> bool {
        let position_moved = c.expected_position.is_some_and(|p| p != self.position.load(Ordering::Acquire));
        position_moved || (!c.protected && !self.relevant(c))
    }
}

impl Voice {
    /// `on_error` is called when a callout cannot be spoken, so the UI can say why.
    /// `backend` builds the speech backend on the voice thread (audio
    /// devices are not always `Send`).
    pub fn start<B, F>(settings: VoiceSettings, backend: F, on_error: impl Fn(String) + Send + 'static) -> Voice
    where
        B: Backend,
        F: FnOnce() -> B + Send + 'static,
    {
        let (tx, rx) = crossbeam_channel::unbounded();
        let context = Arc::new(Context::default());
        let worker_context = context.clone();
        std::thread::Builder::new()
            .name("voice".into())
            .spawn(move || {
                let mut backend = backend();
                // Load the voice now, so the first callout is instant.
                if settings.enabled {
                    if let Err(e) = backend.load(&settings) {
                        log::warn!("voice: {e}");
                    }
                }
                worker(rx, settings, backend, worker_context, on_error)
            })
            .expect("spawn voice thread");
        Voice { tx, context }
    }

    /// Render `text` with the current voice, so a preview can report errors.
    pub fn render(&self, text: String) -> Result<(), String> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.tx.send(Msg::Render(text, tx)).map_err(|_| "Voice engine stopped.".to_string())?;
        rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "The voice took too long to respond.".to_string())?
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

/// Queue a callout. Urgent news clears queued chatter, duplicates are
/// dropped, and routine calls don't pile up behind a backlog. Protected
/// calls are never dropped to make room.
fn enqueue(queue: &mut Vec<Callout>, c: Callout) {
    if c.expected_position.is_some() {
        queue.retain(|q| q.expected_position.is_none());
    }
    if queue.iter().any(|q| q.text == c.text) {
        return;
    }
    match c.priority {
        Priority::High => queue.retain(|q| q.priority != Priority::Low || q.protected),
        Priority::Low if !c.protected && queue.len() >= 2 => return,
        _ => {}
    }
    queue.push(c);
}

fn worker<B: Backend>(rx: Receiver<Msg>, mut settings: VoiceSettings, mut backend: B, context: Arc<Context>, on_error: impl Fn(String)) {
    let mut queue: Vec<Callout> = Vec::new();
    // Previews asked for while a callout was playing.
    let mut renders: Vec<(String, Sender<Result<(), String>>)> = Vec::new();
    loop {
        for (text, reply) in renders.drain(..) {
            let _ = reply.send(backend.prepare(&settings, &text).map(|_| ()));
        }
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
                if settings.enabled {
                    // A new voice loads now rather than on the next callout.
                    if let Err(e) = backend.load(&settings) {
                        on_error(e);
                    }
                }
                continue;
            }
            Some(Msg::Render(text, reply)) => {
                renders.push((text, reply));
                continue;
            }
            Some(Msg::Say(c)) => {
                enqueue(&mut queue, c);
                // Drain anything else waiting so priorities apply.
                while let Ok(m) = rx.try_recv() {
                    match m {
                        Msg::Say(c) => enqueue(&mut queue, c),
                        Msg::Configure(s) => settings = s,
                        Msg::Render(text, reply) => renders.push((text, reply)),
                    }
                }
            }
            None => {}
        }
        queue.retain(|c| context.current(c));
        sort(&mut queue);
        if queue.is_empty() {
            continue;
        }
        let c = queue.remove(0);
        if !settings.enabled {
            queue.clear();
            continue;
        }
        let clip = match backend.prepare(&settings, &c.text) {
            Ok(clip) => clip,
            Err(e) => {
                on_error(e);
                continue;
            }
        };
        // Recheck after synthesis: passes and urgent news can happen while
        // the engine loads the voice. Never play an obsolete position afterward.
        while let Ok(m) = rx.try_recv() {
            match m {
                Msg::Say(next) => enqueue(&mut queue, next),
                Msg::Configure(s) => settings = s,
                Msg::Render(text, reply) => renders.push((text, reply)),
            }
        }
        queue.retain(|q| context.current(q));
        if !settings.enabled || !context.current(&c) {
            continue;
        }
        if queue.iter().any(|q| ahead(q, &c)) {
            enqueue(&mut queue, c);
            continue;
        }
        let playing = match backend.play(clip, &settings) {
            Ok(p) => p,
            Err(e) => {
                on_error(e);
                continue;
            }
        };
        play(playing, &c, &rx, &mut settings, &mut queue, &mut renders, &context);
    }
}

/// Protected news first, then highest priority, oldest first within those.
fn sort(queue: &mut [Callout]) {
    queue.sort_by(|a, b| b.protected.cmp(&a.protected).then(b.priority.cmp(&a.priority)).then(a.at.cmp(&b.at)));
}

/// `a` should be said before `b`.
fn ahead(a: &Callout, b: &Callout) -> bool {
    (a.protected, a.priority) > (b.protected, b.priority)
}

/// Wait for a callout to finish, queueing what arrives meanwhile. Nothing
/// cuts a call off for being more urgent: a driver loses the whole message
/// if it stops halfway (pit news lost mid-sentence in the first corner).
/// Only a call that has become untrue stops (a position that has already
/// changed again), or the session it was about ending.
#[allow(clippy::too_many_arguments)]
fn play(
    mut playing: Box<dyn Playing + '_>,
    call: &Callout,
    rx: &Receiver<Msg>,
    settings: &mut VoiceSettings,
    queue: &mut Vec<Callout>,
    renders: &mut Vec<(String, Sender<Result<(), String>>)>,
    context: &Context,
) {
    playing.set_volume(settings.volume.clamp(0.0, 1.0));
    while !playing.finished() {
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(Msg::Say(c)) => enqueue(queue, c),
            Ok(Msg::Configure(s)) => {
                *settings = s;
                playing.set_volume(settings.volume.clamp(0.0, 1.0));
            }
            Ok(Msg::Render(text, reply)) => renders.push((text, reply)),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                playing.stop();
                break;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        if !settings.enabled || context.obsolete(call) {
            playing.stop();
            break;
        }
    }
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
    fn only_untrue_or_stale_calls_stop_mid_sentence() {
        let context = Context::default();
        context.position.store(5, Ordering::Release);
        let mut pos = Callout::new(Priority::Normal, "Up to P5.");
        pos.expected_position = Some(5);
        let mut pit = Callout::new(Priority::Normal, "Box this lap for fuel.");
        pit.protected = true;
        let chatter = Callout::new(Priority::Low, "Nice rhythm.");
        assert!(!context.obsolete(&pos) && !context.obsolete(&pit) && !context.obsolete(&chatter));
        context.position.store(4, Ordering::Release);
        assert!(context.obsolete(&pos), "the position call is already wrong");
        // The link drops: chatter stops, pit news finishes.
        context.generation.fetch_add(1, Ordering::AcqRel);
        assert!(context.obsolete(&chatter));
        assert!(!context.obsolete(&pit));
    }

    #[test]
    fn protected_news_goes_first_and_is_never_dropped() {
        let mut q = vec![];
        let mut fuel = Callout::new(Priority::Low, "Fuel for 6 laps.");
        fuel.protected = true;
        enqueue(&mut q, Callout::new(Priority::Low, "Nice rhythm."));
        enqueue(&mut q, Callout::new(Priority::Low, "Good pace."));
        enqueue(&mut q, fuel);
        enqueue(&mut q, Callout::new(Priority::High, "You've had a spin."));
        sort(&mut q);
        assert_eq!(q.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Fuel for 6 laps.", "You've had a spin."]);
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