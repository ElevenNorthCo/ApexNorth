//! Race-engineer decision layer. Turns engine events and live state into
//! candidate calls; the director decides which are spoken and when.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::director::{self, Call, Director, EngineerConfig, Topic};
use super::live::{LiveCtx, SlipWatch, TyreWatch};
use super::phrases::{capitalize, speak_gap, speak_laps, speak_time, word, Phrases};
use super::voice::{Callout, Priority};
use crate::session::engine::{EngineEvent, LapContext, LiveState, SectorSplit};
use crate::session::fuel::{PitWindow, RaceStrategy};
use crate::session::incidents::IncidentKind;
use crate::session::models::{CompletedLap, InvalidReason, SECTOR_COUNT};
use crate::session::strategy::{PitDriver, PitPlan, TYRE_CONFIDENCE};
use crate::session::tyres::TyrePerformance;
use crate::settings::ChatterLevel::{self, Chatty, CrewChief, Minimal, Normal};

/// A new race position must hold this long before it's announced, so a
/// pass and re-pass becomes one call about the net change.
const POSITION_SETTLE: Duration = Duration::from_secs(4);
/// Opening shuffle: wait longer and sum it up as the start.
const START_PHASE: Duration = Duration::from_secs(25);
const START_SETTLE: Duration = Duration::from_secs(6);
/// Laps without a position change before a "holding" encouragement.
const HOLD_LAPS: u8 = 3;

/// The late-lap stretch where a "box this lap" reminder is given.
const PRE_PIT: std::ops::Range<f32> = 0.75..0.9;

/// Pace memory for the lap in progress.
#[derive(Default)]
struct LapPace {
    sector_deltas: Vec<i32>,
    worst_cum: i32,
    overall_said: bool,
    recovery_said: bool,
    good_said: bool,
}

#[derive(Default)]
pub struct Engineer {
    director: Director,
    phrases: Phrases,
    tyres: TyreWatch,
    slip: SlipWatch,
    grid_announced: bool,
    laps_done: i32,
    current_lap: i16,
    race_start_at: Option<Instant>,
    incident_at: Option<Instant>,
    pace: LapPace,
    recent_laps: VecDeque<u32>,
    // Fuel and strategy memory.
    strategy: Option<RaceStrategy>,
    plan: Option<PitPlan>,
    fuel_announced: bool,
    announced_per_lap: Option<f32>,
    announced_stops: Option<u8>,
    announced_plan: Option<PitPlan>,
    fuel_critical_warned: bool,
    good_to_end_said: bool,
    window_open_said: bool,
    box_called_lap: Option<i16>,
    prepit_lap: Option<i16>,
    // Race position memory: last announced, latest confirmed, when it changed.
    pos_said: Option<i16>,
    pos_now: Option<i16>,
    pos_changed_at: Option<Instant>,
    laps_since_pos_change: u8,
    /// Lap seen in the live state, and the fuel level when it began.
    live_lap: Option<i16>,
    lap_start_fuel: Option<f32>,
    // Tyre degradation memory, per stint.
    tyre_perf_said: Option<TyrePerformance>,
    projection_said: bool,
    holding_said: bool,
    /// The driver's name from the settings, if any.
    driver: Option<String>,
}

impl Engineer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_event(&mut self, ev: &EngineEvent, cfg: &EngineerConfig, now: Instant) -> Vec<Callout> {
        let level = cfg.chatter;
        self.sync_driver(cfg);
        let mut calls = vec![];
        match ev {
            EngineEvent::SessionStarted { .. } => *self = Engineer { driver: self.driver.take(), ..Engineer::default() },
            EngineEvent::RaceStart => {
                self.race_start_at = Some(now);
                calls.push(Call::new(Topic::RaceStart, Minimal, Priority::High, "race.start", "Lights out. Go, go, go!"));
            }
            EngineEvent::PositionChanged { to } => {
                calls.push(Call::new(Topic::Position, Normal, Priority::Low, "grid.slot", format!("Grid slot P{to}.")).position(*to));
            }
            EngineEvent::RacePositionChanged { from, to } => {
                self.director.retain_position(Some(*to));
                self.pos_said.get_or_insert(*from);
                self.pos_now = Some(*to);
                self.pos_changed_at = Some(now);
                self.laps_since_pos_change = 0;
            }
            EngineEvent::SectorCompleted(s) => calls.extend(self.sector_calls(s, level)),
            EngineEvent::Incident(i) => {
                self.incident_at = Some(now);
                self.tyres.on_incident(now);
                if i.kind == IncidentKind::Spin {
                    let text = self.phrases.pick("incident.spin", &["You've had a spin.", "That was a spin. Gather it up."]);
                    calls.push(Call::new(Topic::Incidents, Minimal, Priority::High, "incident.spin", text).cooldown(20));
                    if i.major {
                        let text = self
                            .phrases
                            .pick("incident.after", &["Take it easy for the next couple of corners.", "Tyres may need a moment to recover."]);
                        calls.push(Call::new(Topic::Incidents, Chatty, Priority::Normal, "incident.after", text).cooldown(30));
                    }
                }
            }
            EngineEvent::LapCompleted(lap, ctx) => {
                self.laps_done += 1;
                self.current_lap = ctx.next_lap;
                self.strategy = ctx.strategy.clone();
                self.plan = ctx.plan.clone();
                calls.extend(self.lap_calls(lap, ctx));
                calls.extend(self.pace_calls(lap, ctx, level));
                calls.extend(self.fuel_calls(ctx, level));
                calls.extend(self.pit_calls(lap, ctx));
                calls.extend(self.tyre_deg_calls(lap, ctx));
                calls.extend(self.position_hold_call(lap, ctx));
                self.pace = LapPace::default();
            }
            _ => {}
        }
        self.director.submit(calls, cfg, now)
    }

    fn sync_driver(&mut self, cfg: &EngineerConfig) {
        if self.driver != cfg.driver {
            self.driver = cfg.driver.clone();
        }
    }

    /// ", Michael" when the driver gave a name, else "".
    fn name(&self) -> String {
        self.driver.as_ref().map_or(String::new(), |n| format!(", {n}"))
    }

    pub fn on_live(&mut self, s: &LiveState, cfg: &EngineerConfig, now: Instant) -> Vec<Callout> {
        self.sync_driver(cfg);
        self.director.retain_position(s.position);
        if s.lap != self.live_lap {
            self.live_lap = s.lap;
            self.lap_start_fuel = s.fuel_pct;
        }
        if s.paused || !s.on_track {
            return vec![];
        }
        let mut calls = vec![];
        if !self.grid_announced && s.lap.is_none() {
            if let (Some(p), Some(n)) = (s.position, s.num_cars) {
                self.grid_announced = true;
                calls.push(Call::new(
                    Topic::Position,
                    Normal,
                    Priority::Normal,
                    "grid.start",
                    match &self.driver {
                        Some(name) => format!("You start P{p} of {n}. Stay out of trouble into turn one. Good luck out there, {name}."),
                        None => format!("You start P{p} of {n}. Stay out of trouble into turn one."),
                    },
                ).position(p));
            }
        }
        let cx = LiveCtx {
            s,
            level: cfg.chatter,
            now,
            laps_done: self.laps_done,
            since_incident: self.incident_at.map(|t| now - t),
            since_start: self.race_start_at.map(|t| now - t),
        };
        calls.extend(self.tyres.update(&cx, &mut self.phrases));
        calls.extend(self.slip.update(&cx, &mut self.phrases));
        calls.extend(self.pre_pit_call(s));
        calls.extend(self.position_change_call(s, now));
        let mut out = self.director.submit(calls, cfg, now);
        out.extend(self.director.release(director::calm(s), now));
        out
    }

    fn sector_calls(&mut self, s: &SectorSplit, level: ChatterLevel) -> Vec<Call> {
        let mut out = vec![];
        if s.sector == 1 {
            self.pace = LapPace::default();
        }
        if let Some(d) = s.delta_to_reference_sector_ms {
            self.pace.sector_deltas.push(d);
        }
        let cum = s.delta_to_reference_ms;
        if let Some(c) = cum {
            self.pace.worst_cum = self.pace.worst_cum.max(c);
        }
        if s.sector >= SECTOR_COUNT {
            // The lap call covers the line.
            return out;
        }
        let n = word(s.sector);
        // Gains get mentioned more often than losses, so a fading car
        // doesn't hear "down" every sector.
        let (up_cd, down_cd) = match level {
            Minimal | Normal => (30, 30),
            Chatty => (12, 25),
            CrewChief => (8, 20),
        };
        if s.session_best {
            let text = self.phrases.pick(
                "sector.pb",
                &["Purple sector.", "Personal best sector.", "Nice one. Best sector so far.", "That's your quickest sector.", "Nice sector. Personal best."],
            );
            let cd = match level {
                Minimal | Normal => 20,
                Chatty => 15,
                CrewChief => 10,
            };
            out.push(Call::new(Topic::Sectors, Normal, Priority::Normal, "sector.pb", text).cooldown(cd));
        } else if let Some(d) = s.delta_to_reference_sector_ms {
            let gap = speak_gap(d.unsigned_abs());
            let big = capitalize(&gap);
            let (lvl, prio, key, text) = match d {
                ..=-300 => (
                    Normal,
                    Priority::Normal,
                    "sector.up",
                    self.phrases.pick(
                        "sector.up.big",
                        &[&format!("Sector {n}, {gap} up."), &format!("{big} quicker through sector {n}."), &format!("Strong sector {n}. {big} up.")],
                    ),
                ),
                -299..=-100 => (
                    Chatty,
                    Priority::Low,
                    "sector.up",
                    self.phrases.pick("sector.up", &[&format!("Sector {n}, {gap} up."), &format!("{big} up in sector {n}.")]),
                ),
                300.. => (
                    Normal,
                    Priority::Normal,
                    "sector.down",
                    self.phrases.pick(
                        "sector.down.big",
                        &[&format!("You lost {gap} there."), &format!("Sector {n}, {gap} down."), &format!("{big} lost in sector {n}.")],
                    ),
                ),
                100..=299 => (CrewChief, Priority::Low, "sector.down", format!("Sector {n}, {gap} down.")),
                _ if s.sector == SECTOR_COUNT - 1 && cum.is_some_and(|c| c.abs() < 100) => {
                    (CrewChief, Priority::Low, "sector.match", "You're matching your best lap.".to_string())
                }
                _ => return out,
            };
            let cd = if key == "sector.down" { down_cd } else { up_cd };
            out.push(Call::new(Topic::Sectors, lvl, prio, key, text).cooldown(cd));
        }

        // Pace through the lap, from the third sector on.
        let Some(c) = cum.filter(|_| s.sector >= 3) else { return out };
        let p = &mut self.pace;
        if !p.recovery_said && p.worst_cum >= 300 && c as f32 <= p.worst_cum as f32 * 0.3 {
            p.recovery_said = true;
            out.push(Call::new(Topic::Pace, Chatty, Priority::Low, "pace.recovery", "Good recovery. You made most of that time back."));
        } else if !p.overall_said && c <= -500 {
            p.overall_said = true;
            let gap = speak_gap(c.unsigned_abs());
            let text = self.phrases.pick("pace.overall", &[&format!("{} up overall.", capitalize(&gap)), &format!("You're {gap} up on your best.")]);
            out.push(Call::new(Topic::Pace, Chatty, Priority::Low, "pace.overall", text));
        } else if !p.good_said && c <= 0 && p.sector_deltas.iter().filter(|d| **d <= 50).count() >= 3 {
            p.good_said = true;
            let text = self.phrases.pick("pace.lap", &["You're putting together a good lap.", "This is a strong lap. Keep it going."]);
            out.push(Call::new(Topic::Pace, Chatty, Priority::Low, "pace.lap", text).cooldown(120));
        }
        out
    }

    fn lap_calls(&mut self, lap: &CompletedLap, ctx: &LapContext) -> Vec<Call> {
        let t = speak_time(lap.time_ms);
        let text = match lap.invalid_reason {
            Some(InvalidReason::PitLap) => Some(format!("Pit lap, {t}.")),
            // Timing joined part-way round: nothing to judge.
            Some(InvalidReason::JoinedMidLap) => None,
            Some(_) => Some(format!("{t}. That lap won't count.")),
            None => Some(match (ctx.reference_before, ctx.session_best_before) {
                (Some(r), _) if lap.time_ms < r => {
                    let gap = speak_gap(r - lap.time_ms);
                    let name = self.name();
                    self.phrases.pick(
                        "lap.pb",
                        &[&format!("Purple lap{name}! {t}. New personal best by {gap}."), &format!("New personal best{name}! {t}. That's {gap} quicker.")],
                    )
                }
                (_, Some(sb)) if lap.time_ms < sb => format!("{t}. Best lap of the session."),
                (Some(r), _) => format!("{t}. {} off your best.", capitalize(&speak_gap(lap.time_ms - r))),
                (None, _) => format!("Lap {}, {t}.", lap.lap_no),
            }),
        };
        let mut out = vec![];
        if let Some(text) = text {
            out.push(Call::new(Topic::LapTimes, Minimal, Priority::Normal, "lap.time", text));
        }
        if ctx.laps_remaining == Some(1) {
            let name = self.name();
            let text = match self.pos_now.or(self.pos_said) {
                Some(1) => format!("Final lap{name}. You're leading, bring it home."),
                Some(p) => format!("Final lap{name}. You're P{p}, bring it home."),
                None => format!("Final lap{name}. Bring it home."),
            };
            out.push(Call::new(Topic::LapTimes, Minimal, Priority::Normal, "lap.final", text));
        }
        out
    }

    fn pace_calls(&mut self, lap: &CompletedLap, ctx: &LapContext, level: ChatterLevel) -> Vec<Call> {
        if !lap.is_valid {
            return vec![];
        }
        self.recent_laps.push_back(lap.time_ms);
        while self.recent_laps.len() > 5 {
            self.recent_laps.pop_front();
        }
        let best = [ctx.reference_before, ctx.session_best_before].into_iter().flatten().min();
        if best.map_or(true, |b| lap.time_ms < b) {
            // A personal best already got its own call.
            return vec![];
        }
        let n = self.recent_laps.len();
        let last3: Vec<u32> = self.recent_laps.iter().skip(n.saturating_sub(3)).copied().collect();
        if last3.len() == 3 {
            if last3[0] > last3[1] + 50 && last3[1] > last3[2] + 50 {
                return vec![Call::new(Topic::Pace, Chatty, Priority::Low, "pace.improving", "You're consistently improving.").cooldown(240)];
            }
            let spread = last3.iter().max().unwrap() - last3.iter().min().unwrap();
            if spread <= 300 {
                let text = self.phrases.pick("pace.consistent", &["That's good consistency.", "Nice rhythm. Very consistent."]);
                return vec![Call::new(Topic::Pace, Chatty, Priority::Low, "pace.consistent", text).cooldown(300)];
            }
        }
        if best.is_some_and(|b| lap.time_ms <= b + 300) {
            let text = self
                .phrases
                .pick("pace.good", &["Good pace. Keep it up.", "Nice rhythm.", "This is strong pace.", "Keep doing what you're doing."]);
            let cd = if level <= Normal { 300 } else { 150 };
            return vec![Call::new(Topic::Pace, Normal, Priority::Low, "pace.good", text).cooldown(cd)];
        }
        vec![]
    }

    fn fuel_calls(&mut self, ctx: &LapContext, level: ChatterLevel) -> Vec<Call> {
        let mut out = vec![];
        let Some(s) = &ctx.strategy else { return out };
        let lap_limited = ctx.total_laps > 0;
        let per = s.fuel_per_lap_pct;
        let pct = format!("{per:.1}");
        if !self.fuel_announced {
            self.fuel_announced = true;
            self.announced_per_lap = Some(per);
            self.announced_stops = Some(s.stops_required);
            let stops = match (lap_limited, s.stops_required) {
                (false, _) => String::new(),
                (true, 0) => {
                    self.good_to_end_said = true;
                    " Fuel is good to the end.".into()
                }
                (true, 1) => " One stop expected.".into(),
                (true, n) => format!(" {n} stops expected."),
            };
            out.push(Call::new(Topic::Fuel, Normal, Priority::Normal, "fuel.estimate", format!("Fuel usage is {pct} percent per lap.{stops}")));
            return out;
        }
        let needs_stop = s.stops_required > 0 || !lap_limited;
        if needs_stop && s.fuel_laps_remaining < 2.0 {
            if !self.fuel_critical_warned {
                self.fuel_critical_warned = true;
                out.push(Call::new(
                    Topic::Fuel,
                    Minimal,
                    Priority::High,
                    "fuel.critical",
                    format!("Fuel is critical. {} of fuel remaining.", capitalize(&speak_laps(s.fuel_laps_remaining))),
                ));
            }
        } else if s.fuel_laps_remaining >= 2.5 {
            self.fuel_critical_warned = false;
        }

        if let Some(a) = self.announced_per_lap.filter(|a| (per - a).abs() / a > 0.08) {
            self.announced_per_lap = Some(per);
            let dir = if per > a { "increased" } else { "dropped" };
            out.push(Call::new(Topic::Fuel, Chatty, Priority::Low, "fuel.rate", format!("Fuel usage has {dir}. Now {pct} percent per lap.")).cooldown(60));
        }

        if lap_limited && self.announced_stops != Some(s.stops_required) {
            let prev = self.announced_stops.replace(s.stops_required).unwrap_or(0);
            let text = match s.stops_required {
                0 => {
                    self.good_to_end_said = true;
                    "Fuel is good to the end.".to_string()
                }
                1 if prev == 0 => "Fuel usage has increased. One stop needed now.".into(),
                n if n > prev => format!("Fuel usage has increased. Strategy updated. {n} stops now."),
                1 => "Fuel looks better. One stop to go.".into(),
                n => format!("Fuel looks better. {n} stops to go."),
            };
            out.push(Call::new(Topic::Fuel, Normal, Priority::Normal, "fuel.stops", text));
        }

        // A routine fuel read at the line.
        let every = match level {
            Minimal => 0,
            Normal => 3,
            Chatty => 2,
            CrewChief => 1,
        };
        let settled = s.stops_required == 0 && self.good_to_end_said && level < CrewChief;
        if every > 0 && self.laps_done % every == 0 && !settled && s.fuel_laps_remaining < 40.0 && !self.fuel_critical_warned {
            out.push(Call::new(Topic::Fuel, Normal, Priority::Low, "fuel.laps", format!("Fuel for {}.", speak_laps(s.fuel_laps_remaining))));
        }
        let boxing = ctx.plan.as_ref().is_some_and(|p| p.box_this_lap(ctx.next_lap));
        if let Some(latest) = s.latest_pit_lap.filter(|_| s.stops_required > 0 && !boxing) {
            if latest - ctx.next_lap + 1 == 2 {
                out.push(
                    Call::new(Topic::Fuel, Normal, Priority::Normal, "fuel.margin", "You can make two more laps comfortably.").cooldown(180),
                );
            }
        }
        out
    }

    fn pit_calls(&mut self, lap: &CompletedLap, ctx: &LapContext) -> Vec<Call> {
        let mut out = vec![];
        if lap.is_pit {
            // A new stint: announce the next window fresh.
            self.announced_plan = None;
            self.window_open_said = false;
        }
        let (Some(s), Some(p)) = (&ctx.strategy, &ctx.plan) else {
            self.announced_plan = None;
            return out;
        };
        let cur = ctx.next_lap;
        if p.box_this_lap(cur) {
            if self.box_called_lap != Some(cur) {
                self.box_called_lap = Some(cur);
                let text = match p.driver {
                    PitDriver::Both => "Box this lap. Both fuel and tyre performance favor the stop.".to_string(),
                    PitDriver::Tyres => "Consider a tyre stop this lap. The pace trend suggests the tyres are fading.".into(),
                    PitDriver::Fuel if s.window == PitWindow::BoxNow => "Box, box. Fuel is at the estimated reserve limit.".into(),
                    PitDriver::Fuel => format!("Recommended stop this lap. Estimated fuel limit is lap {}.", p.fuel_latest),
                };
                let urgent = s.window == PitWindow::BoxNow;
                out.push(Call::new(Topic::PitStrategy, if urgent { Minimal } else { Normal }, if urgent { Priority::High } else { Priority::Normal }, "pit.box", text));
            }
        } else {
            let (e, l) = (s.earliest_pit_lap.unwrap_or(p.lap), s.latest_pit_lap.unwrap_or(p.lap));
            let text = match &self.announced_plan {
                None if e == l => Some(format!("Pit on lap {l}.")),
                None => Some(format!("Pit window laps {e} to {l}. Lap {} is the target.", p.lap)),
                Some(prev) if p.driver == PitDriver::Tyres && prev.driver != PitDriver::Tyres => {
                    Some(format!("Fuel can make lap {}, but tyre degradation favors stopping on lap {}.", p.fuel_target, p.lap))
                }
                Some(prev) if p.lap < prev.lap && p.driver == PitDriver::Tyres => {
                    Some(format!("The tyres are fading faster. Target stop is now lap {}.", p.lap))
                }
                Some(prev) if p.lap < prev.lap => Some(self.phrases.pick(
                    "pit.earlier",
                    &[
                        &format!("Fuel usage is higher than expected. Pit window has moved forward to lap {}.", p.lap),
                        &format!("Fuel usage has increased. Target stop is now lap {}.", p.lap),
                    ],
                )),
                Some(prev) if p.lap > prev.lap => Some(format!("Fuel looks better than expected. You can extend to lap {}.", p.lap)),
                Some(_) if s.window == PitWindow::Open && !self.window_open_said => {
                    self.window_open_said = true;
                    Some("Pit window is open.".into())
                }
                Some(_) => None,
            };
            if let Some(text) = text {
                out.push(Call::new(Topic::PitStrategy, Normal, Priority::Normal, "pit.window", text));
            }
        }
        self.announced_plan = Some(p.clone());
        out
    }

    fn tyre_deg_calls(&mut self, lap: &CompletedLap, ctx: &LapContext) -> Vec<Call> {
        let mut out = vec![];
        if lap.is_pit {
            self.tyre_perf_said = None;
            self.projection_said = false;
            self.holding_said = false;
            return out;
        }
        let t = &ctx.tyres;
        if t.confidence >= 0.4 && t.performance >= TyrePerformance::Declining && self.tyre_perf_said.map_or(true, |p| t.performance > p) {
            self.tyre_perf_said = Some(t.performance);
            let text = match t.performance {
                TyrePerformance::Declining => "Tyre performance is starting to drop.",
                TyrePerformance::Poor => "Tyre performance is dropping quickly.",
                _ => "Pace is falling away. Estimated tyre degradation is high.",
            };
            out.push(Call::new(Topic::Tyres, Normal, Priority::Normal, "tyre.deg", text).severity(t.performance as u8));
            if let Some(loss) = t.estimated_loss_per_lap_ms.filter(|l| *l >= 150 && t.confidence >= TYRE_CONFIDENCE) {
                out.push(Call::new(
                    Topic::Tyres,
                    Chatty,
                    Priority::Low,
                    "tyre.loss",
                    format!("You're losing roughly {} a lap as the tyres fade.", speak_gap(loss as u32)),
                ));
            }
        }
        if !self.projection_said && t.confidence >= TYRE_CONFIDENCE && ctx.total_laps > 0 {
            if let Some(pl) = t.projected_problem_lap.filter(|pl| *pl > ctx.next_lap && *pl <= ctx.total_laps) {
                self.projection_said = true;
                out.push(Call::new(
                    Topic::Tyres,
                    Chatty,
                    Priority::Low,
                    "tyre.projection",
                    format!("At the current degradation rate, expect a significant pace loss around lap {pl}."),
                ));
            }
        }
        let stopping = ctx.strategy.as_ref().is_some_and(|s| s.stops_required > 0);
        if !self.holding_said && stopping && t.stint_laps >= 4 && t.performance == TyrePerformance::Stable && t.confidence >= 0.6 {
            self.holding_said = true;
            out.push(Call::new(Topic::Tyres, Chatty, Priority::Low, "tyre.holding", "Tyres are holding up well. No need to stop early."));
        }
        out
    }

    /// Late in a lap where the plan says pit, remind the driver before the
    /// pit entry (whose position we don't know).
    /// Announce a settled race position change: the net gain or loss since
    /// the last call, cheering gains and steadying the driver after losses.
    fn position_change_call(&mut self, s: &LiveState, now: Instant) -> Option<Call> {
        let (said, cur, at) = (self.pos_said?, self.pos_now?, self.pos_changed_at?);
        // Don't announce the previous confirmed position during another pass.
        if s.position != Some(cur) {
            return None;
        }
        if cur == said {
            self.pos_changed_at = None;
            return None;
        }
        let at_start = self.race_start_at.is_some_and(|t| at.saturating_duration_since(t) < START_PHASE);
        if now.saturating_duration_since(at) < if at_start { START_SETTLE } else { POSITION_SETTLE } {
            return None;
        }
        self.pos_said = Some(cur);
        self.pos_changed_at = None;
        let gained = said - cur;
        let n = gained.unsigned_abs() as usize;
        let count = if word(n).is_empty() { n.to_string() } else { word(n).to_string() };
        let places = if n == 1 { "one place".to_string() } else { format!("{count} places") };
        let last_laps = s.laps_remaining.is_some_and(|l| l <= 1);
        let after_incident = self.incident_at.is_some_and(|t| now.saturating_duration_since(t) < Duration::from_secs(25));
        let p = &mut self.phrases;
        let (level, text) = if gained > 0 {
            if cur == 1 {
                (Minimal, p.pick("pos.lead", &["You're leading! P1. Keep it clean.", "That's the lead! P1. Head down, eyes forward.", "P1! You're out front now. Smooth and steady."]))
            } else if at_start {
                (Normal, format!("Great start! Up {places} to P{cur}."))
            } else if cur <= 3 && said > 3 {
                (Minimal, format!("Into a podium spot, P{cur}! Keep pushing."))
            } else if last_laps {
                (Normal, format!("Up to P{cur}! Hold it to the flag."))
            } else if n >= 2 {
                (Normal, p.pick("pos.gain.many", &[&format!("Brilliant! {} gained, up to P{cur}.", capitalize(&places)), &format!("Up {places} to P{cur}. You're flying.")]))
            } else {
                (
                    Normal,
                    p.pick(
                        "pos.gain",
                        &[
                            &format!("Nice move. P{cur}."),
                            &format!("Great pass. Up to P{cur}. Next car's the target."),
                            &format!("Good job, P{cur}. Go get the next one."),
                            &format!("P{cur}. That's how it's done."),
                        ],
                    ),
                )
            }
        } else if said == 1 {
            (Minimal, format!("Lost the lead, P{cur}. Stay with them, it's not over."))
        } else if at_start {
            (Normal, format!("Tough start, down to P{cur}. Long way to go, settle in."))
        } else if after_incident {
            (Normal, format!("Lost {places} there, P{cur}. Breathe, reset. Plenty of race left."))
        } else if said <= 3 && cur > 3 {
            (Normal, format!("Dropped off the podium, P{cur}. Plenty of fight left."))
        } else if last_laps {
            (Normal, format!("P{cur}. Keep it clean and bring it home."))
        } else if n >= 2 {
            (Normal, p.pick("pos.loss.many", &[&format!("Down {places} to P{cur}. Stay calm, clean laps will bring them back."), &format!("P{cur}. Shake it off. Your pace is still good.")]))
        } else {
            (
                Normal,
                p.pick(
                    "pos.loss",
                    &[
                        &format!("Lost one, P{cur}. Stay calm, you'll get it back."),
                        &format!("P{cur}. No panic, keep your lines clean."),
                        &format!("Dropped to P{cur}. Stick with them, chances will come."),
                    ],
                ),
            )
        };
        Some(Call::new(Topic::Position, level, Priority::Normal, "pos.change", text).position(cur))
    }

    /// After a few laps in the same place: keep the driver chasing, or calm
    /// out front.
    fn position_hold_call(&mut self, lap: &CompletedLap, ctx: &LapContext) -> Option<Call> {
        let pos = self.pos_now.or(self.pos_said)?;
        if ctx.total_laps <= 0 || ctx.laps_remaining.is_some_and(|l| l <= 1) {
            return None;
        }
        self.laps_since_pos_change = self.laps_since_pos_change.saturating_add(1);
        if self.laps_since_pos_change < HOLD_LAPS {
            return None;
        }
        let best = [ctx.reference_before, ctx.session_best_before].into_iter().flatten().min();
        let strong = lap.is_valid && best.is_some_and(|b| lap.time_ms <= b + 300);
        let ahead = pos - 1;
        let text = if pos == 1 {
            self.phrases.pick("pos.hold.lead", &["Still leading. Smooth and steady.", "P1 and in control. Keep that rhythm."])
        } else if strong {
            self.phrases.pick(
                "pos.hold.chase",
                &[&format!("Strong lap. Keep that up and you'll reel in P{ahead}."), &format!("That pace will catch P{ahead}. Keep pushing.")],
            )
        } else {
            self.phrases.pick(
                "pos.hold",
                &[&format!("Holding P{pos}. Stay patient, P{ahead} is the target."), &format!("Solid, P{pos}. Clean laps, the next car will make a mistake.")],
            )
        };
        self.laps_since_pos_change = 0;
        Some(Call::new(Topic::Position, Chatty, Priority::Normal, "pos.hold", text).position(pos).cooldown(150))
    }

    fn pre_pit_call(&mut self, s: &LiveState) -> Option<Call> {
        let p = self.plan.as_ref()?;
        let lap = s.lap?;
        let frac = s.lap_fraction?;
        // Already refuelled this lap: the stop has happened.
        let refuelled = matches!((s.fuel_pct, self.lap_start_fuel), (Some(f), Some(start)) if f > start + 1.0);
        if refuelled || lap != self.current_lap || !p.box_this_lap(lap) || !PRE_PIT.contains(&frac) || self.prepit_lap == Some(lap) {
            return None;
        }
        self.prepit_lap = Some(lap);
        let tight = self.strategy.as_ref().is_some_and(|st| st.window == PitWindow::BoxNow);
        let text = if tight {
            "Box this lap. Fuel is at the estimated reserve limit.".to_string()
        } else if p.driver == PitDriver::Tyres {
            "Tyre stop recommended this lap. Check your pace before extending.".to_string()
        } else {
            format!("Planned stop this lap. Estimated fuel limit is lap {}.", p.fuel_latest)
        };
        Some(Call::new(Topic::PitStrategy, if tight { Minimal } else { Normal }, if tight { Priority::High } else { Priority::Normal }, "pit.prepit", text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::fuel::FuelModel;
    use crate::session::models::CompletedLap;
    use crate::session::tyres::TyreEstimate;
    use crate::settings::Callouts;
    use std::time::Duration;

    fn cfg(chatter: ChatterLevel) -> EngineerConfig {
        EngineerConfig { chatter, callouts: Callouts::default(), driver: None }
    }

    fn split(sector: usize, d: i32, cum: i32, pb: bool) -> EngineEvent {
        EngineEvent::SectorCompleted(SectorSplit {
            lap_no: 3,
            sector,
            sector_ms: 20_000,
            delta_to_best_sector_ms: Some(d),
            delta_to_reference_sector_ms: Some(d),
            delta_to_reference_ms: Some(cum),
            session_best: pb,
        })
    }

    fn calm_live() -> LiveState {
        LiveState { on_track: true, lap: Some(3), speed_kph: 200.0, ..Default::default() }
    }

    /// Feed events, then one calm live frame, and collect what's spoken.
    fn say(e: &mut Engineer, evs: &[EngineEvent], c: &EngineerConfig, now: Instant) -> Vec<String> {
        let mut out: Vec<String> = evs.iter().flat_map(|ev| e.on_event(ev, c, now)).map(|c| c.text).collect();
        out.extend(e.on_live(&calm_live(), c, now).into_iter().map(|c| c.text));
        out
    }

    #[test]
    fn minimal_suppresses_routine_sectors() {
        let t = Instant::now();
        let evs = [split(1, -200, -200, false), split(2, -400, -600, false)];
        assert!(say(&mut Engineer::new(), &evs, &cfg(Minimal), t).is_empty());
        let normal = say(&mut Engineer::new(), &evs, &cfg(Normal), t);
        assert_eq!(normal, ["Sector two, 4 tenths up."]);
        let chatty = say(&mut Engineer::new(), &evs[..1], &cfg(Chatty), t);
        assert_eq!(chatty, ["Sector one, 2 tenths up."]);
    }

    #[test]
    fn sector_pb_and_tiny_deltas() {
        let t = Instant::now();
        let mut e = Engineer::new();
        let got = say(&mut e, &[split(1, -50, -50, true)], &cfg(Normal), t);
        assert_eq!(got, ["Purple sector."]);
        // A few hundredths either way: quiet, even when chatty.
        assert!(say(&mut Engineer::new(), &[split(2, 40, 0, false)], &cfg(Chatty), t).is_empty());
    }

    #[test]
    fn category_toggle_beats_chatter_level() {
        let t = Instant::now();
        let mut c = cfg(CrewChief);
        c.callouts.sectors = false;
        c.callouts.pace = false;
        let evs = [split(1, -400, -400, true), split(2, -400, -800, false), split(3, -400, -1200, false)];
        assert!(say(&mut Engineer::new(), &evs, &c, t).is_empty());
    }

    #[test]
    fn sector_cooldown_prevents_spam() {
        let t = Instant::now();
        let mut e = Engineer::new();
        let c = cfg(Normal);
        assert_eq!(say(&mut e, &[split(1, 400, 400, false)], &c, t).len(), 1);
        assert!(say(&mut e, &[split(2, 400, 800, false)], &c, t + Duration::from_secs(10)).is_empty());
        assert_eq!(say(&mut e, &[split(3, 400, 1200, false)], &c, t + Duration::from_secs(31)).len(), 1);
    }

    #[test]
    fn pace_recovery_and_good_lap() {
        let t = Instant::now();
        let mut e = Engineer::new();
        let c = cfg(Chatty);
        say(&mut e, &[split(1, 500, 500, false)], &c, t);
        let got = say(&mut e, &[split(3, -450, 50, false)], &c, t + Duration::from_secs(40));
        assert!(got.iter().any(|s| s.contains("Good recovery")), "{got:?}");
    }

    fn lap(no: i16, ms: u32) -> CompletedLap {
        CompletedLap {
            lap_no: no,
            time_ms: ms,
            is_pit: false,
            is_valid: true,
            invalid_reason: None,
            sectors_ms: vec![],
            fuel_used_pct: Some(10.0),
            max_speed: 200.0,
            length_m: 3000.0,
            start_pos: [0.0; 3],
            samples: vec![],
        }
    }

    fn ctx(next_lap: i16, total: i16, fuel_pct: f32, per_lap: &[f32]) -> LapContext {
        let mut f = FuelModel::default();
        for p in per_lap {
            f.record_lap(*p);
        }
        let strategy = f.strategy(fuel_pct, next_lap, total);
        let tyres = TyreEstimate::default();
        let plan = strategy.as_ref().and_then(|s| crate::session::strategy::plan(s, &tyres, next_lap, total));
        LapContext {
            session_best_before: Some(90_000),
            reference_before: Some(90_000),
            laps_remaining: Some((total - next_lap + 1) as i32),
            strategy,
            tyres,
            plan,
            next_lap,
            total_laps: total,
        }
    }

    #[test]
    fn lap_validity_wording() {
        let t = Instant::now();
        let c = cfg(Normal);
        let mut joined = lap(1, 95_000);
        joined.is_valid = false;
        joined.invalid_reason = Some(InvalidReason::JoinedMidLap);
        let got = say(&mut Engineer::new(), &[EngineEvent::LapCompleted(Box::new(joined), ctx(2, 0, 50.0, &[]))], &c, t);
        assert!(got.is_empty(), "{got:?}");
        let mut reset = lap(2, 95_000);
        reset.is_valid = false;
        reset.invalid_reason = Some(InvalidReason::RewindOrReset);
        let got = say(&mut Engineer::new(), &[EngineEvent::LapCompleted(Box::new(reset), ctx(3, 0, 50.0, &[]))], &c, t);
        assert_eq!(got, ["1 35 point 0. That lap won't count."]);
    }

    #[test]
    fn first_fuel_estimate_and_window() {
        let t = Instant::now();
        let mut e = Engineer::new();
        let got = say(&mut e, &[EngineEvent::LapCompleted(Box::new(lap(2, 90_500)), ctx(3, 12, 78.0, &[9.8]))], &cfg(Normal), t);
        assert!(got.contains(&"Fuel usage is 9.8 percent per lap. One stop expected.".to_string()), "{got:?}");
        assert!(got.iter().any(|s| s.starts_with("Pit window laps 3 to 9. Lap 8 is the target.")), "{got:?}");
    }

    #[test]
    fn window_moves_forward_then_box() {
        let t = Instant::now();
        let mut e = Engineer::new();
        let c = cfg(Normal);
        say(&mut e, &[EngineEvent::LapCompleted(Box::new(lap(2, 90_500)), ctx(3, 12, 78.0, &[9.8]))], &c, t);
        let got = say(&mut e, &[EngineEvent::LapCompleted(Box::new(lap(3, 90_500)), ctx(4, 12, 65.0, &[9.8, 13.0]))], &c, t + Duration::from_secs(90));
        assert!(got.iter().any(|s| s.contains("moved forward to lap") || s.contains("Target stop is now lap")), "{got:?}");
        let got = say(&mut e, &[EngineEvent::LapCompleted(Box::new(lap(7, 90_500)), ctx(8, 12, 14.0, &[11.0]))], &c, t + Duration::from_secs(400));
        assert!(got.iter().any(|s| s.starts_with("Box, box.")), "{got:?}");
    }

    #[test]
    fn fuel_off_means_silence() {
        let t = Instant::now();
        let mut e = Engineer::new();
        let got = say(&mut e, &[EngineEvent::LapCompleted(Box::new(lap(2, 90_500)), ctx(3, 0, 100.0, &[0.0, 0.0]))], &cfg(CrewChief), t);
        assert!(got.iter().all(|s| !s.contains("Fuel") && !s.contains("Pit")), "{got:?}");
    }
}