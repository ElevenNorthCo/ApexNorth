//! Decides whether and when a candidate call is spoken: chatter level,
//! category toggles, per-fact anti-nag memory, a minimum gap between routine
//! calls, and holding non-urgent calls for a calm moment on track.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::voice::{Callout, Priority};
use crate::session::engine::LiveState;
use crate::settings::{Callouts, ChatterLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    LapTimes,
    Sectors,
    Pace,
    Fuel,
    PitStrategy,
    Tyres,
    WheelSlip,
    Incidents,
    RaceStart,
    Position,
}

impl Topic {
    fn enabled(self, c: &Callouts) -> bool {
        match self {
            Topic::LapTimes => c.lap_times,
            Topic::Sectors => c.sectors,
            Topic::Pace => c.pace,
            Topic::Fuel => c.fuel,
            Topic::PitStrategy => c.pit_strategy,
            Topic::Tyres => c.tyres,
            Topic::WheelSlip => c.wheel_slip,
            Topic::Incidents => c.incidents,
            Topic::RaceStart => c.race_start,
            Topic::Position => c.position,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EngineerConfig {
    pub chatter: ChatterLevel,
    pub callouts: Callouts,
    /// The driver's name, used now and then ("Good luck out there, Michael").
    pub driver: Option<String>,
}

/// Something the engineer could say, with what it's about.
#[derive(Debug, Clone)]
pub struct Call {
    pub topic: Topic,
    /// The lowest chatter level that hears this.
    pub level: ChatterLevel,
    pub priority: Priority,
    /// Names the fact, so it isn't repeated while nothing changes.
    pub key: &'static str,
    /// Don't repeat this key within this time unless severity rises.
    pub cooldown: Duration,
    pub severity: u8,
    /// Speak straight away, even mid-corner.
    pub urgent: bool,
    pub text: String,
    pub expected_position: Option<i16>,
    /// Don't cut this off mid-sentence for newer, higher priority news.
    pub protected: bool,
    pub outlives_session: bool,
}

impl Call {
    pub fn new(topic: Topic, level: ChatterLevel, priority: Priority, key: &'static str, text: impl Into<String>) -> Call {
        Call {
            topic,
            level,
            priority,
            key,
            cooldown: Duration::ZERO,
            severity: 0,
            urgent: priority == Priority::High,
            text: text.into(),
            expected_position: None,
            protected: false,
            outlives_session: false,
        }
    }

    pub fn cooldown(mut self, secs: u64) -> Call {
        self.cooldown = Duration::from_secs(secs);
        self
    }

    pub fn position(mut self, position: i16) -> Call {
        self.expected_position = Some(position);
        self
    }

    fn into_callout(self) -> Callout {
        let mut c = Callout::new(self.priority, self.text);
        c.expected_position = self.expected_position;
        c.protected = self.protected;
        c.outlives_session = self.outlives_session;
        c
    }

    /// The chequered-flag call: said at once, never cut off, and not
    /// cleared when the game resets the session right after.
    pub fn finale(mut self) -> Call {
        self.urgent = true;
        self.protected = true;
        self.outlives_session = true;
        self
    }

    pub fn protect(mut self) -> Call {
        self.protected = true;
        self
    }

    pub fn severity(mut self, s: u8) -> Call {
        self.severity = s;
        self
    }
}

/// Minimum time between routine (low priority) calls at each level.
fn routine_gap(level: ChatterLevel) -> Duration {
    Duration::from_secs(match level {
        ChatterLevel::Minimal => 60,
        ChatterLevel::Normal => 30,
        ChatterLevel::Chatty => 15,
        ChatterLevel::CrewChief => 8,
    })
}

/// How long a call may wait for a calm moment.
fn max_hold(p: Priority) -> Duration {
    match p {
        Priority::Low => Duration::from_secs(4),
        _ => Duration::from_secs(6),
    }
}

/// Low workload: not braking, not cornering hard.
pub fn calm(s: &LiveState) -> bool {
    s.brake < 10 && s.g_lat.abs() < 0.6 && s.steer.map_or(true, |w| w.abs() < 0.3)
}

#[derive(Default)]
pub struct Director {
    spoken: HashMap<&'static str, (Instant, u8)>,
    last_routine: Option<Instant>,
    held: Vec<(Call, Instant)>,
}

impl Director {
    /// Filter calls and return the ones to speak now. The rest wait for
    /// `release`.
    pub fn submit(&mut self, calls: Vec<Call>, cfg: &EngineerConfig, now: Instant) -> Vec<Callout> {
        let mut out = vec![];
        for c in calls {
            if !self.admit(&c, cfg, now) {
                continue;
            }
            if c.urgent {
                out.push(c.into_callout());
            } else {
                let deadline = now + max_hold(c.priority);
                self.held.push((c, deadline));
            }
        }
        out
    }

    fn admit(&mut self, c: &Call, cfg: &EngineerConfig, now: Instant) -> bool {
        if !c.topic.enabled(&cfg.callouts) || cfg.chatter < c.level {
            return false;
        }
        if let Some((at, sev)) = self.spoken.get(c.key) {
            if c.severity <= *sev && now.saturating_duration_since(*at) < c.cooldown {
                return false;
            }
        }
        if c.priority == Priority::Low && !c.urgent {
            if self.last_routine.is_some_and(|t| now.saturating_duration_since(t) < routine_gap(cfg.chatter)) {
                return false;
            }
            self.last_routine = Some(now);
        }
        self.spoken.insert(c.key, (now, c.severity));
        true
    }

    /// Let held calls go when the driver has a moment, or when they've waited
    /// long enough. Stale routine calls are dropped instead.
    pub fn release(&mut self, calm: bool, now: Instant) -> Vec<Callout> {
        let mut out = vec![];
        let mut keep = vec![];
        for (c, deadline) in self.held.drain(..) {
            if calm || (now >= deadline && c.priority != Priority::Low) {
                out.push(c.into_callout());
            } else if now < deadline {
                keep.push((c, deadline));
            }
        }
        self.held = keep;
        out.sort_by(|a, b| b.priority.cmp(&a.priority));
        out
    }

    /// A held position announcement is obsolete as soon as the position changes.
    pub fn retain_position(&mut self, current: Option<i16>) {
        self.held.retain(|(c, _)| c.expected_position.is_none() || c.expected_position == current);
    }

    #[cfg(test)]
    pub fn held(&self) -> usize {
        self.held.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(chatter: ChatterLevel) -> EngineerConfig {
        EngineerConfig { chatter, callouts: Callouts::default(), driver: None }
    }

    fn call(level: ChatterLevel, p: Priority, key: &'static str) -> Call {
        Call::new(Topic::Sectors, level, p, key, key)
    }

    #[test]
    fn level_and_toggle_gate() {
        let t = Instant::now();
        let mut d = Director::default();
        let chatty = call(ChatterLevel::Chatty, Priority::High, "a");
        assert!(d.submit(vec![chatty.clone()], &cfg(ChatterLevel::Normal), t).is_empty());
        assert_eq!(d.submit(vec![chatty.clone()], &cfg(ChatterLevel::Chatty), t).len(), 1);
        let mut off = cfg(ChatterLevel::CrewChief);
        off.callouts.sectors = false;
        assert!(d.submit(vec![call(ChatterLevel::Minimal, Priority::High, "b")], &off, t).is_empty());
    }

    #[test]
    fn cooldown_unless_worse() {
        let t = Instant::now();
        let c = cfg(ChatterLevel::Normal);
        let mut d = Director::default();
        let hot = call(ChatterLevel::Normal, Priority::High, "hot").cooldown(60);
        assert_eq!(d.submit(vec![hot.clone()], &c, t).len(), 1);
        assert!(d.submit(vec![hot.clone()], &c, t + Duration::from_secs(10)).is_empty());
        assert_eq!(d.submit(vec![hot.clone().severity(2)], &c, t + Duration::from_secs(11)).len(), 1);
        assert_eq!(d.submit(vec![hot], &c, t + Duration::from_secs(80)).len(), 1);
    }

    #[test]
    fn routine_gap_by_level() {
        let t = Instant::now();
        let low = |k| call(ChatterLevel::Normal, Priority::Low, k);
        let mut d = Director::default();
        d.submit(vec![low("x"), low("y")], &cfg(ChatterLevel::Normal), t);
        assert_eq!(d.held(), 1, "second routine call inside the gap is dropped");
        let mut e = Director::default();
        e.submit(vec![low("x")], &cfg(ChatterLevel::CrewChief), t);
        e.submit(vec![low("y")], &cfg(ChatterLevel::CrewChief), t + Duration::from_secs(9));
        assert_eq!(e.held(), 2);
    }

    #[test]
    fn holds_for_calm_moment() {
        let t = Instant::now();
        let mut d = Director::default();
        let c = cfg(ChatterLevel::Normal);
        assert!(d.submit(vec![call(ChatterLevel::Normal, Priority::Normal, "n")], &c, t).is_empty());
        d.submit(vec![call(ChatterLevel::Normal, Priority::Low, "l")], &c, t);
        assert!(d.release(false, t + Duration::from_secs(1)).is_empty());
        // Past the deadline mid-corner: the normal call goes, the routine one is dropped.
        let out = d.release(false, t + Duration::from_secs(7));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "n");
        assert_eq!(d.held(), 0);
    }
}