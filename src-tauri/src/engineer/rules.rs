//! Race-engineer callout rules. Turns engine events and live state into short
//! spoken messages, debounced so the engineer never nags.

use std::time::{Duration, Instant};

use super::voice::{Callout, Priority};
use crate::session::engine::{EngineEvent, LiveState};
use crate::session::fuel::PitWindow;
use crate::settings::Callouts;

const TYRE_HOT_C: f32 = 100.0;
const TYRE_COLD_C: f32 = 50.0;

#[derive(Default)]
pub struct Engineer {
    pit_window: Option<PitWindow>,
    fuel_critical_warned: bool,
    grid_announced: bool,
    hot_since: [Option<Instant>; 2],
    cold_since: Option<Instant>,
    last_tyre_call: Option<Instant>,
    spin_edges: Vec<Instant>,
    lock_edges: Vec<Instant>,
    spinning: bool,
    locking: bool,
    last_slip_call: Option<Instant>,
    laps_done: i32,
}

/// "1 34 point 7" — reads naturally through TTS.
pub fn speak_time(ms: u32) -> String {
    let m = ms / 60_000;
    let s = (ms % 60_000) / 1000;
    let tenths = (ms % 1000) / 100;
    match (m, s) {
        (0, _) => format!("{s} point {tenths}"),
        (_, 0..=9) => format!("{m} oh {s} point {tenths}"),
        _ => format!("{m} {s} point {tenths}"),
    }
}

pub fn speak_gap(ms: u32) -> String {
    match ms {
        0..=49 => "a few hundredths".into(),
        50..=149 => "a tenth".into(),
        150..=949 => format!("{} tenths", (ms + 50) / 100),
        _ => format!("{:.1} seconds", ms as f32 / 1000.0),
    }
}

fn cooled(last: Option<Instant>, every: Duration) -> bool {
    last.map_or(true, |t| t.elapsed() >= every)
}

impl Engineer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_event(&mut self, ev: &EngineEvent, cfg: &Callouts) -> Vec<Callout> {
        let mut out = vec![];
        match ev {
            EngineEvent::SessionStarted { .. } => *self = Engineer::default(),
            EngineEvent::RaceStart if cfg.race_start => {
                out.push(Callout::new(Priority::High, "Lights out. Go, go, go!"));
            }
            EngineEvent::PositionChanged { to } if cfg.position => {
                out.push(Callout::new(Priority::Low, format!("Grid slot P{to}.")));
            }
            EngineEvent::LapCompleted(lap, ctx) => {
                self.laps_done += 1;
                if cfg.lap_times {
                    let t = speak_time(lap.time_ms);
                    let text = if lap.is_pit {
                        format!("Pit lap, {t}.")
                    } else if !lap.is_valid {
                        format!("{t}. That lap won't count.")
                    } else {
                        match (ctx.reference_before, ctx.session_best_before) {
                            (Some(r), _) if lap.time_ms < r => {
                                format!("Purple lap! {t}. New personal best by {}.", speak_gap(r - lap.time_ms))
                            }
                            (_, Some(sb)) if lap.time_ms < sb => format!("{t}. Best lap of the session."),
                            (Some(r), _) => format!("{t}. {} off your best.", speak_gap(lap.time_ms - r)),
                            (None, _) => format!("Lap {}, {t}.", lap.lap_no),
                        }
                    };
                    out.push(Callout::new(Priority::Normal, text));
                    if ctx.laps_remaining == Some(1) {
                        out.push(Callout::new(Priority::Normal, "Final lap. Bring it home."));
                    }
                }
                if cfg.fuel {
                    if let Some(left) = ctx.fuel_laps_left {
                        let need = ctx.laps_remaining.map_or(f32::MAX, |r| r as f32);
                        if left < 2.0 && left < need && !self.fuel_critical_warned {
                            self.fuel_critical_warned = true;
                            out.push(Callout::new(Priority::High, "Fuel is critical. Less than two laps left."));
                        } else if self.laps_done % 3 == 0 || (left < 5.0 && left < need) {
                            out.push(Callout::new(Priority::Low, format!("Fuel for {:.0} laps.", left.floor())));
                        }
                        if left >= 2.0 {
                            self.fuel_critical_warned = false;
                        }
                    }
                }
                if cfg.pit_window && self.pit_window != Some(ctx.pit_window) {
                    let prev = self.pit_window.replace(ctx.pit_window);
                    let msg = match ctx.pit_window {
                        PitWindow::Open => Some((Priority::High, "Pit window is open.")),
                        PitWindow::BoxNow => Some((Priority::High, "Box, box. Box this lap.")),
                        PitWindow::NoStop if prev.is_some_and(|p| p != PitWindow::None) => {
                            Some((Priority::Normal, "Fuel is good to the end."))
                        }
                        _ => None,
                    };
                    if let Some((p, m)) = msg {
                        out.push(Callout::new(p, m));
                    }
                }
            }
            _ => {}
        }
        out
    }

    pub fn on_live(&mut self, s: &LiveState, cfg: &Callouts) -> Vec<Callout> {
        let mut out = vec![];
        if s.paused || !s.on_track {
            return out;
        }
        let now = Instant::now();

        if cfg.position && !self.grid_announced && s.lap.is_none() {
            if let (Some(p), Some(n)) = (s.position, s.num_cars) {
                self.grid_announced = true;
                out.push(Callout::new(Priority::Normal, format!("You start P{p} of {n}. Stay out of trouble into turn one.")));
            }
        }

        if cfg.tyres {
            let axle = [(s.tyre_temp[0] + s.tyre_temp[1]) / 2.0, (s.tyre_temp[2] + s.tyre_temp[3]) / 2.0];
            for (i, t) in axle.iter().enumerate() {
                if *t > TYRE_HOT_C {
                    let since = *self.hot_since[i].get_or_insert(now);
                    if now - since > Duration::from_secs(4) && cooled(self.last_tyre_call, Duration::from_secs(45)) {
                        self.last_tyre_call = Some(now);
                        let which = if i == 0 { "Fronts" } else { "Rears" };
                        out.push(Callout::new(Priority::Normal, format!("{which} are overheating. Look after the tyres.")));
                    }
                } else {
                    self.hot_since[i] = None;
                }
            }
            let avg = (axle[0] + axle[1]) / 2.0;
            if self.laps_done >= 1 && s.speed_kph > 80.0 && avg < TYRE_COLD_C && avg > 1.0 {
                let since = *self.cold_since.get_or_insert(now);
                if now - since > Duration::from_secs(10) && cooled(self.last_tyre_call, Duration::from_secs(90)) {
                    self.last_tyre_call = Some(now);
                    out.push(Callout::new(Priority::Low, "Tyres are cold. Push to bring them in."));
                }
            } else {
                self.cold_since = None;
            }
        }

        if cfg.wheel_slip {
            let rear = s.wheel_slip[2].max(s.wheel_slip[3]);
            let front = s.wheel_slip[0].min(s.wheel_slip[1]);
            let spinning = rear > 0.15 && s.throttle > 50;
            let locking = front < -0.2 && s.brake > 50;
            if spinning && !self.spinning {
                self.spin_edges.push(now);
            }
            if locking && !self.locking {
                self.lock_edges.push(now);
            }
            self.spinning = spinning;
            self.locking = locking;
            let window = Duration::from_secs(20);
            self.spin_edges.retain(|t| now - *t < window);
            self.lock_edges.retain(|t| now - *t < window);
            if cooled(self.last_slip_call, Duration::from_secs(60)) {
                if self.spin_edges.len() >= 4 {
                    self.last_slip_call = Some(now);
                    self.spin_edges.clear();
                    out.push(Callout::new(Priority::Normal, "Wheelspin on exit. Be smoother on the throttle."));
                } else if self.lock_edges.len() >= 3 {
                    self.last_slip_call = Some(now);
                    self.lock_edges.clear();
                    out.push(Callout::new(Priority::Normal, "You're locking the fronts. Ease the brake pressure."));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_times() {
        assert_eq!(speak_time(94_700), "1 34 point 7");
        assert_eq!(speak_time(64_350), "1 oh 4 point 3");
        assert_eq!(speak_time(42_000), "42 point 0");
        assert_eq!(speak_gap(320), "3 tenths");
        assert_eq!(speak_gap(1460), "1.5 seconds");
    }
}
