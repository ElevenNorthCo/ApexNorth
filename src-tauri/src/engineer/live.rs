//! Watches the live 60 Hz state for tyre temperature changes, wheelspin and
//! lock-ups. Calls happen on changes of state, not on a timer.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::director::{Call, Topic};
use super::phrases::Phrases;
use super::voice::Priority;
use crate::session::engine::LiveState;
use crate::settings::ChatterLevel;

pub struct LiveCtx<'a> {
    pub s: &'a LiveState,
    pub level: ChatterLevel,
    pub now: Instant,
    pub laps_done: i32,
    pub since_incident: Option<Duration>,
    pub since_start: Option<Duration>,
}

impl LiveCtx<'_> {
    fn within(d: Option<Duration>, secs: u64) -> bool {
        d.is_some_and(|d| d < Duration::from_secs(secs))
    }
}

// ---------------------------------------------------------------- tyres

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Band {
    Cold,
    Window,
    Hot,
    Over,
}

const OVER_IN: f32 = 110.0;
const OVER_OUT: f32 = 106.0;
const HOT_IN: f32 = 100.0;
const HOT_OUT: f32 = 96.0;
const COLD_IN: f32 = 50.0;
const COLD_OUT: f32 = 55.0;
/// After an incident, "hot" triggers this much earlier.
const INCIDENT_SHIFT: f32 = 5.0;

fn band_of(t: f32, cur: Option<Band>, shift: f32) -> Band {
    let raw = if t > OVER_IN {
        Band::Over
    } else if t > HOT_IN - shift {
        Band::Hot
    } else if t < COLD_IN {
        Band::Cold
    } else {
        Band::Window
    };
    // Hysteresis: stay in the current band until clearly out of it.
    match cur {
        Some(Band::Over) if t > OVER_OUT => Band::Over,
        Some(Band::Hot) if raw < Band::Hot && t > HOT_OUT - shift => Band::Hot,
        Some(Band::Cold) if t < COLD_OUT => Band::Cold,
        _ => raw,
    }
}

#[derive(Default)]
struct Axle {
    band: Option<Band>,
    candidate: Option<(Band, Instant)>,
    /// One average temperature per second, newest last.
    hist: VecDeque<f32>,
}

#[derive(Default)]
pub struct TyreWatch {
    axles: [Axle; 2],
    last_sample: Option<Instant>,
    window_said: bool,
    imbalance: Option<(usize, Instant)>,
    incident: Option<(Instant, bool)>,
}

const AXLE: [&str; 2] = ["Front", "Rear"];
const AXLE_PL: [&str; 2] = ["Fronts", "Rears"];
const CORNER: [&str; 4] = ["Front-left", "Front-right", "Rear-left", "Rear-right"];

impl TyreWatch {
    pub fn on_incident(&mut self, now: Instant) {
        self.incident = Some((now, false));
    }

    pub fn update(&mut self, cx: &LiveCtx, ph: &mut Phrases) -> Vec<Call> {
        let mut out = vec![];
        let t = cx.s.tyre_temp;
        if t.iter().all(|x| *x <= 1.0) {
            return out;
        }
        let avg = [(t[0] + t[1]) / 2.0, (t[2] + t[3]) / 2.0];
        let shift = if LiveCtx::within(cx.since_incident, 60) { INCIDENT_SHIFT } else { 0.0 };
        let now = cx.now;

        let sample = self.last_sample.map_or(true, |at| now - at >= Duration::from_secs(1));
        if sample {
            self.last_sample = Some(now);
        }
        for i in 0..2 {
            let ax = &mut self.axles[i];
            if sample {
                ax.hist.push_back(avg[i]);
                while ax.hist.len() > 11 {
                    ax.hist.pop_front();
                }
            }
            let b = band_of(avg[i], ax.band, shift);
            if Some(b) == ax.band {
                ax.candidate = None;
                continue;
            }
            let since = match ax.candidate {
                Some((cb, at)) if cb == b => at,
                _ => {
                    ax.candidate = Some((b, now));
                    now
                }
            };
            let dwell = if b == Band::Over { 2 } else { 4 };
            if now - since < Duration::from_secs(dwell) {
                continue;
            }
            let prev = ax.band.replace(b);
            ax.candidate = None;
            out.extend(self.band_call(i, prev, b, cx, ph));
        }

        // One corner much hotter than the rest.
        let hottest = (0..4).max_by(|a, b| t[*a].total_cmp(&t[*b])).unwrap_or(0);
        let others = (t.iter().sum::<f32>() - t[hottest]) / 3.0;
        if t[hottest] - others > 12.0 && t[hottest] > 70.0 {
            let since = match self.imbalance {
                Some((i, at)) if i == hottest => at,
                _ => {
                    self.imbalance = Some((hottest, now));
                    now
                }
            };
            if now - since >= Duration::from_secs(8) {
                out.push(
                    Call::new(Topic::Tyres, ChatterLevel::Chatty, Priority::Low, "tyre.imbalance", format!("{} is running hotter than the others.", CORNER[hottest]))
                        .cooldown(240),
                );
            }
        } else {
            self.imbalance = None;
        }

        // Climbing fast: 12 °C in 10 seconds, already warm.
        for i in 0..2 {
            let h = &self.axles[i].hist;
            // Only once already warm: warming up from cold is expected.
            if h.len() == 11 && h[0] > 75.0 && h[10] - h[0] > 12.0 && h[10] > 85.0 {
                let key = if i == 0 { "tyre.rate.f" } else { "tyre.rate.r" };
                out.push(
                    Call::new(Topic::Tyres, ChatterLevel::Chatty, Priority::Low, key, format!("{} temperatures are climbing quickly.", AXLE[i])).cooldown(180),
                );
            }
        }

        // Follow-up after a spin.
        if let Some((at, warned)) = self.incident {
            let since = now - at;
            if !warned && since < Duration::from_secs(6) && avg[1] > HOT_IN - INCIDENT_SHIFT {
                self.incident = Some((at, true));
                out.push(Call::new(Topic::Tyres, ChatterLevel::Chatty, Priority::Low, "tyre.incident", "Rear tyres may be hot after that."));
            } else if warned && avg[1] < HOT_OUT - INCIDENT_SHIFT {
                self.incident = None;
                out.push(Call::new(Topic::Tyres, ChatterLevel::Chatty, Priority::Low, "tyre.recover", "Tyres are recovering after that spin."));
            } else if since > Duration::from_secs(90) {
                self.incident = None;
            }
        }
        out
    }

    fn band_call(&mut self, i: usize, prev: Option<Band>, b: Band, cx: &LiveCtx, ph: &mut Phrases) -> Option<Call> {
        let fk = |f: &'static str, r: &'static str| if i == 0 { f } else { r };
        match (prev, b) {
            (_, Band::Over) => Some(
                Call::new(
                    Topic::Tyres,
                    ChatterLevel::Minimal,
                    Priority::High,
                    fk("tyre.over.f", "tyre.over.r"),
                    format!("{} are overheating now. Look after them.", AXLE_PL[i]),
                )
                .cooldown(120)
                .severity(2),
            ),
            (None, _) => None,
            (Some(Band::Window) | Some(Band::Cold), Band::Hot) => Some(
                Call::new(
                    Topic::Tyres,
                    ChatterLevel::Normal,
                    Priority::Normal,
                    fk("tyre.hot.f", "tyre.hot.r"),
                    ph.pick(
                        fk("tyre.hot.f", "tyre.hot.r"),
                        &[&format!("{} tyres are getting hot.", AXLE[i]), &format!("{} temperatures are getting high.", AXLE[i])],
                    ),
                )
                // Long memory: temperatures hovering at the line shouldn't nag.
                .cooldown(240)
                .severity(1),
            ),
            (Some(Band::Hot) | Some(Band::Over), Band::Window) => Some(
                Call::new(
                    Topic::Tyres,
                    ChatterLevel::Chatty,
                    Priority::Low,
                    fk("tyre.down.f", "tyre.down.r"),
                    format!("{} temperatures are coming back down.", AXLE[i]),
                )
                .cooldown(180),
            ),
            (Some(Band::Cold), Band::Window) => {
                let other = self.axles[1 - i].band;
                if other == Some(Band::Window) && !self.window_said {
                    self.window_said = true;
                    Some(Call::new(Topic::Tyres, ChatterLevel::Chatty, Priority::Low, "tyre.window", "Tyres are in a good window."))
                } else {
                    Some(
                        Call::new(
                            Topic::Tyres,
                            ChatterLevel::Chatty,
                            Priority::Low,
                            fk("tyre.warm.f", "tyre.warm.r"),
                            format!("{} are coming up to temperature.", AXLE_PL[i]),
                        )
                        .cooldown(120),
                    )
                }
            }
            (Some(_), Band::Cold) if cx.laps_done >= 1 && cx.s.speed_kph > 80.0 => Some(
                Call::new(Topic::Tyres, ChatterLevel::Normal, Priority::Low, "tyre.cold", "Tyres are cold. Push to bring them in.").cooldown(120),
            ),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- slip

/// One wheelspin or lock-up episode.
#[derive(Default)]
struct Episode {
    active: bool,
    peak: f32,
    called: bool,
}

#[derive(Default)]
pub struct SlipWatch {
    spin: Episode,
    lock: Episode,
    spins: VecDeque<Instant>,
    locks: VecDeque<Instant>,
}

/// Rear slip ratio that starts an episode, and one that is worth a call by itself.
const SPIN_START: f32 = 0.15;
const SPIN_BIG: f32 = 0.35;
const LOCK_START: f32 = -0.2;
const LOCK_BIG: f32 = -0.4;

fn slip_cooldown(level: ChatterLevel, repeated: bool) -> u64 {
    match (level, repeated) {
        (ChatterLevel::CrewChief, false) => 15,
        (ChatterLevel::Chatty, false) => 25,
        (_, false) => 45,
        (ChatterLevel::CrewChief, true) => 45,
        (ChatterLevel::Chatty, true) => 60,
        (_, true) => 90,
    }
}

impl SlipWatch {
    pub fn update(&mut self, cx: &LiveCtx, ph: &mut Phrases) -> Vec<Call> {
        let mut out = vec![];
        let s = cx.s;
        // A spin or a launch makes huge slip that isn't worth coaching.
        if LiveCtx::within(cx.since_incident, 6) || LiveCtx::within(cx.since_start, 6) || s.speed_kph < 30.0 {
            self.spin = Episode::default();
            self.lock = Episode::default();
            return out;
        }
        let now = cx.now;
        let rear = s.wheel_slip[2].max(s.wheel_slip[3]);
        let front = s.wheel_slip[0].min(s.wheel_slip[1]);
        let level = cx.level;

        // Wheelspin.
        if !self.spin.active && rear > SPIN_START && s.throttle > 40 {
            self.spin = Episode { active: true, peak: rear, called: false };
        }
        if self.spin.active {
            self.spin.peak = self.spin.peak.max(rear);
            if !self.spin.called && self.spin.peak > SPIN_BIG && s.throttle > 60 {
                self.spin.called = true;
                out.push(
                    Call::new(
                        Topic::WheelSlip,
                        ChatterLevel::Normal,
                        Priority::Normal,
                        "slip.spin",
                        ph.pick("slip.spin", &["Wheelspin on exit.", "Careful on the throttle. Wheelspin there."]),
                    )
                    .cooldown(slip_cooldown(level, false)),
                );
            }
            if rear < 0.08 || s.throttle < 20 {
                self.spin.active = false;
                self.spins.push_back(now);
            }
        }
        self.spins.retain(|t| now - *t < Duration::from_secs(30));
        if self.spins.len() >= 3 {
            self.spins.clear();
            out.push(
                Call::new(
                    Topic::WheelSlip,
                    ChatterLevel::Normal,
                    Priority::Normal,
                    "slip.spin.repeat",
                    "You're getting repeated wheelspin. Be smoother on throttle.",
                )
                .cooldown(slip_cooldown(level, true)),
            );
        }

        // Lock-ups.
        if !self.lock.active && front < LOCK_START && s.brake > 30 {
            self.lock = Episode { active: true, peak: front, called: false };
        }
        if self.lock.active {
            self.lock.peak = self.lock.peak.min(front);
            if !self.lock.called && self.lock.peak < LOCK_BIG {
                self.lock.called = true;
                out.push(
                    Call::new(
                        Topic::WheelSlip,
                        ChatterLevel::Normal,
                        Priority::Normal,
                        "slip.lock",
                        ph.pick("slip.lock", &["Fronts locked there.", "Lock-up into that one."]),
                    )
                    .cooldown(slip_cooldown(level, false)),
                );
            }
            if front > -0.08 || s.brake < 10 {
                self.lock.active = false;
                self.locks.push_back(now);
            }
        }
        self.locks.retain(|t| now - *t < Duration::from_secs(40));
        if self.locks.len() >= 3 {
            self.locks.clear();
            out.push(
                Call::new(
                    Topic::WheelSlip,
                    ChatterLevel::Normal,
                    Priority::Normal,
                    "slip.lock.repeat",
                    "You're consistently locking the fronts. Ease the brake pressure.",
                )
                .cooldown(slip_cooldown(level, true)),
            );
        }
        // A repeated-pattern call says it all; drop the single-event one.
        if out.iter().any(|c| c.key.ends_with(".repeat")) {
            out.retain(|c| c.key.ends_with(".repeat"));
        }
        out
    }
}
