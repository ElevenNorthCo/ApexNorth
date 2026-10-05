//! Turns the raw 60 Hz packet stream into lap timing, recorded laps, fuel
//! strategy and the live dashboard state.

use serde::Serialize;

use super::fuel::{FuelModel, FuelState, PitWindow, RaceStrategy};
use super::incidents::{Incident, SpinDetector};
use super::models::{sector_times, time_at_distance, CompletedLap, InvalidReason, Sample, SECTOR_COUNT};
use super::strategy::{self, PitPlan};
use super::tyres::{TyreEstimate, TyreModel};
use crate::telemetry::packet::{flags, Packet};

const TICK_HZ: f32 = 60.0;
/// Record every Nth tick (60 Hz / 2 = 30 Hz).
const SAMPLE_EVERY: u32 = 2;
/// Packet-id gaps up to this many ticks (5 s) count as real elapsed time.
const MAX_GAP_TICKS: u32 = 300;
/// An older packet within this many ids of the newest is a late arrival.
const STALE_IDS: i32 = 600;
/// Slack added to the distance a car could plausibly cover between packets.
const JUMP_SLACK_M: f32 = 40.0;
/// Packets that must agree on the new position before a jump is believed.
const JUMP_CONFIRM: u32 = 3;
/// Packets (60 Hz) a new race position must hold before it's believed, so a
/// side-by-side moment that flicks back and forth isn't treated as a pass.
const POSITION_CONFIRM: u32 = 30;
/// An unresolved jump is believed after this many ticks anyway.
const JUMP_RESOLVE_TICKS: u32 = 60;
/// A lap shorter than this fraction of the reference lap didn't cover the track.
const MIN_LAP_FRACTION: f32 = 0.75;
/// A sector best has to beat the old one by this much to be called a best.
const PB_MARGIN_MS: u32 = 20;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    pub position: Option<i16>,
    pub num_cars: Option<i16>,
    pub lap: Option<i16>,
    pub total_laps: Option<i16>,
    pub gear: u8,
    pub in_gear: bool,
    pub suggested_gear: Option<u8>,
    pub speed_kph: f32,
    pub rpm: f32,
    pub rpm_alert_min: f32,
    pub rpm_alert_max: f32,
    pub throttle: u8,
    pub brake: u8,
    pub steer: Option<f32>,
    pub tyre_temp: [f32; 4],
    pub wheel_slip: [f32; 4],
    pub current_lap_ms: Option<u32>,
    pub last_lap_ms: Option<u32>,
    pub best_lap_ms: Option<u32>,
    pub delta_ms: Option<i32>,
    pub lap_distance: f32,
    /// Distance through the lap as a fraction of the reference lap, once known.
    pub lap_fraction: Option<f32>,
    /// Why the current lap won't count, if it won't.
    pub lap_invalid: Option<InvalidReason>,
    pub fuel_pct: Option<f32>,
    pub fuel_per_lap_pct: Option<f32>,
    pub fuel_laps_left: Option<f32>,
    pub fuel_state: FuelState,
    pub laps_remaining: Option<i32>,
    pub pit_window: Option<PitWindow>,
    pub strategy: Option<RaceStrategy>,
    pub pit_plan: Option<PitPlan>,
    pub tyre_estimate: TyreEstimate,
    pub g_lat: f32,
    pub g_lon: f32,
    pub boost: f32,
    pub water_temp: f32,
    pub oil_temp: f32,
    pub on_track: bool,
    pub paused: bool,
    pub rev_limit: bool,
    pub tcs: bool,
    pub asm: bool,
    pub car_code: i32,
}

#[derive(Debug, Clone)]
pub struct LapContext {
    pub session_best_before: Option<u32>,
    pub reference_before: Option<u32>,
    /// Laps left after the completed one.
    pub laps_remaining: Option<i32>,
    pub strategy: Option<RaceStrategy>,
    pub tyres: TyreEstimate,
    pub plan: Option<PitPlan>,
    /// The lap now being driven.
    pub next_lap: i16,
    pub total_laps: i16,
}

/// A sector boundary crossed during a lap. Sectors are fifths of the
/// reference lap's distance; sector 5 ends at the line.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorSplit {
    pub lap_no: i16,
    /// 1-based.
    pub sector: usize,
    pub sector_ms: u32,
    /// Against the best time for this sector (session or reference lap).
    pub delta_to_best_sector_ms: Option<i32>,
    /// Against the same sector of the reference (best) lap.
    pub delta_to_reference_sector_ms: Option<i32>,
    /// Whole-lap delta to the reference lap at this boundary.
    pub delta_to_reference_ms: Option<i32>,
    /// Beat a previous best for this sector.
    pub session_best: bool,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    SessionStarted { car_code: i32, total_laps: i16 },
    SessionEnded,
    RaceStart,
    LapCompleted(Box<CompletedLap>, LapContext),
    SectorCompleted(SectorSplit),
    /// The current lap just became invalid (also logged for diagnosis).
    LapInvalidated { lap_no: i16, reason: InvalidReason },
    Incident(Incident),
    PositionChanged { to: i16 },
    /// Race position changed during the race (confirmed over POSITION_CONFIRM packets).
    RacePositionChanged { from: i16, to: i16 },
}

#[derive(Debug, Clone)]
pub struct RefLap {
    pub time_ms: u32,
    pub samples: Vec<Sample>,
    /// A race's first lap from the grid: fine for the live delta, but its
    /// sectors are no benchmark.
    pub standing_start: bool,
}

impl RefLap {
    fn length(&self) -> Option<f32> {
        self.samples.last().map(|s| s.d).filter(|d| *d > 100.0)
    }
}

#[derive(Debug)]
struct LapBuf {
    lap_no: i16,
    ticks: u32,
    dist: f64,
    samples: Vec<Sample>,
    fuel_start: f32,
    refueled: bool,
    stopped_ticks: u32,
    invalid: Option<InvalidReason>,
    start_pos: [f32; 3],
    max_speed: f32,
    decim: u32,
    standing_start: bool,
    /// Lap length the sector boundaries are placed on.
    sector_len: Option<f32>,
    /// Reference lap's elapsed time at each boundary (the last is its lap time).
    ref_cum: Option<[u32; SECTOR_COUNT]>,
    /// Elapsed time at each boundary crossed so far.
    splits: Vec<u32>,
    /// How the car crossed into this lap from the previous one.
    entry: Option<Entry>,
}

/// The last packet of one lap and the first of the next. The line lies
/// between them; after a packet gap it can be well behind the first packet.
#[derive(Debug, Clone, Copy)]
struct Entry {
    from: [f32; 3],
    to: [f32; 3],
    from_speed: f32,
    to_speed: f32,
    gap_ticks: u32,
}

impl LapBuf {
    fn new(lap_no: i16, p: &Packet, seen_start: bool) -> Self {
        LapBuf {
            lap_no,
            ticks: 0,
            dist: 0.0,
            samples: Vec::with_capacity(4096),
            fuel_start: p.fuel,
            refueled: false,
            stopped_ticks: 0,
            invalid: (!seen_start).then_some(InvalidReason::JoinedMidLap),
            start_pos: p.position,
            max_speed: 0.0,
            decim: SAMPLE_EVERY, // record the first tick
            standing_start: false,
            sector_len: None,
            ref_cum: None,
            splits: Vec::with_capacity(SECTOR_COUNT),
            entry: None,
        }
    }

    fn elapsed_ms(&self) -> u32 {
        (self.ticks as f32 * 1000.0 / TICK_HZ).round() as u32
    }

    fn set_reference(&mut self, r: Option<&RefLap>) {
        self.sector_len = r.and_then(|r| r.length());
        self.ref_cum = match (self.sector_len, r) {
            (Some(len), Some(r)) if !r.standing_start => {
                let mut cum = [r.time_ms; SECTOR_COUNT];
                let mut ok = true;
                for (k, c) in cum.iter_mut().take(SECTOR_COUNT - 1).enumerate() {
                    match time_at_distance(&r.samples, len * (k + 1) as f32 / SECTOR_COUNT as f32) {
                        Some(t) => *c = t.round() as u32,
                        None => ok = false,
                    }
                }
                ok.then_some(cum)
            }
            _ => None,
        };
    }
}

struct Pending {
    buf: LapBuf,
    prev_last_lap: i32,
    end_fuel: f32,
    fuel_capacity: f32,
    wait: u32,
}

/// Last trusted car position, for telling real resets from glitches.
struct Anchor {
    pos: [f32; 3],
    speed: f32,
    /// Unpaused ticks since this position.
    ticks: u32,
}

/// A position jump waiting for more packets to confirm or contradict it.
struct Suspect {
    landed: [f32; 3],
    confirms: u32,
    ticks: u32,
}

enum Motion {
    Ok,
    /// Unconfirmed jump: keep timing, but don't trust this position.
    Hold,
    Reset(InvalidReason),
}

#[derive(Default)]
pub struct Engine {
    last: Option<Packet>,
    last_id: Option<i32>,
    session_active: bool,
    car_code: i32,
    lap: Option<LapBuf>,
    pending: Option<Pending>,
    fuel: FuelModel,
    tyres: TyreModel,
    spin: SpinDetector,
    strategy: Option<RaceStrategy>,
    plan: Option<PitPlan>,
    tyre_est: TyreEstimate,
    session_best: Option<u32>,
    best_sectors: [Option<u32>; SECTOR_COUNT],
    reference: Option<RefLap>,
    last_lap_ms: Option<u32>,
    anchor: Option<Anchor>,
    suspect: Option<Suspect>,
    paused_since_anchor: bool,
    g: [f32; 2],
    live: LiveState,
    /// Trusted race position, and a different one waiting for confirmation.
    race_pos: Option<i16>,
    pos_candidate: Option<(i16, u32)>,
}

impl Engine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn live(&self) -> &LiveState {
        &self.live
    }

    /// Install a stored best lap (same track + car) as the delta reference.
    pub fn set_reference(&mut self, r: RefLap) {
        if self.reference.as_ref().map_or(true, |cur| r.time_ms < cur.time_ms) {
            if !r.standing_start {
                self.merge_best_sectors(&sector_times(&r.samples, r.time_ms));
            }
            self.reference = Some(r);
            self.refresh_lap_reference();
        }
    }

    pub fn reference_ms(&self) -> Option<u32> {
        self.reference.as_ref().map(|r| r.time_ms)
    }

    fn merge_best_sectors(&mut self, secs: &[u32]) {
        if secs.len() != SECTOR_COUNT {
            return;
        }
        for (b, s) in self.best_sectors.iter_mut().zip(secs) {
            if *s > 0 && b.map_or(true, |b| *s < b) {
                *b = Some(*s);
            }
        }
    }

    /// Give the lap in progress the current reference, if it hasn't reached
    /// its first sector yet.
    fn refresh_lap_reference(&mut self) {
        if let Some(l) = self.lap.as_mut().filter(|l| l.splits.is_empty()) {
            l.set_reference(self.reference.as_ref());
        }
    }

    fn new_lap(&self, lap_no: i16, p: &Packet, seen_start: bool) -> LapBuf {
        let mut b = LapBuf::new(lap_no, p, seen_start);
        b.set_reference(self.reference.as_ref());
        b
    }

    fn start_session(&mut self, p: &Packet, ev: &mut Vec<EngineEvent>) {
        if self.session_active {
            ev.push(EngineEvent::SessionEnded);
        }
        self.session_active = true;
        self.car_code = p.car_code;
        self.fuel.reset();
        self.tyres.reset();
        self.tyre_est = TyreEstimate::default();
        self.strategy = None;
        self.plan = None;
        self.session_best = None;
        self.best_sectors = [None; SECTOR_COUNT];
        self.reference = None;
        self.last_lap_ms = None;
        self.pending = None;
        self.race_pos = None;
        self.pos_candidate = None;
        ev.push(EngineEvent::SessionStarted { car_code: p.car_code, total_laps: p.total_laps });
    }

    fn end_session(&mut self, ev: &mut Vec<EngineEvent>) {
        if self.session_active {
            ev.push(EngineEvent::SessionEnded);
        }
        self.session_active = false;
        self.lap = None;
        self.pending = None;
    }

    pub fn on_packet(&mut self, p: &Packet) -> Vec<EngineEvent> {
        let mut ev = Vec::new();
        let gap = self.last_id.map(|id| p.packet_id as i64 - id as i64);
        let dt_ticks = match gap {
            // Duplicates and late (reordered) packets would rewind the car.
            Some(g) if g <= 0 && g > -(STALE_IDS as i64) => return ev,
            Some(g) if g > 0 && g <= MAX_GAP_TICKS as i64 => g as u32,
            _ => 1,
        };
        self.last_id = Some(p.packet_id);
        let dt = dt_ticks as f32 / TICK_HZ;
        let paused = p.has(flags::PAUSED) || p.has(flags::LOADING);
        let prev = self.last.take();

        // G-forces from the change in velocity, in the car's frame.
        if let (Some(pr), false) = (&prev, paused) {
            let a = [
                (p.velocity[0] - pr.velocity[0]) / dt,
                (p.velocity[1] - pr.velocity[1]) / dt,
                (p.velocity[2] - pr.velocity[2]) / dt,
            ];
            let local = rotate_inverse(p.rotation, a);
            let k = 0.15;
            self.g[0] += k * (local[0] / 9.81 - self.g[0]);
            self.g[1] += k * (local[2] / 9.81 - self.g[1]);
        }

        // Pre-race grid position changes.
        if let Some(pr) = &prev {
            if p.lap == 0 && pr.pre_race_position > 0 && p.pre_race_position > 0 && pr.pre_race_position != p.pre_race_position {
                ev.push(EngineEvent::PositionChanged { to: p.pre_race_position });
            }
        }
        self.track_race_position(p, paused, &mut ev);

        if self.session_active && p.car_code != self.car_code {
            self.end_session(&mut ev);
        }

        if paused {
            self.paused_since_anchor = true;
        } else {
            self.track_laps(p, prev.as_ref(), dt_ticks, dt, &mut ev);
            if p.lap > 0 && p.has(flags::CAR_ON_TRACK) {
                let local = rotate_inverse(p.rotation, p.velocity);
                if let Some(i) = self.spin.update(p, local, dt_ticks) {
                    ev.push(EngineEvent::Incident(i));
                }
            }
        }
        self.settle_pending(p, dt_ticks, &mut ev);
        self.update_live(p, paused);
        self.last = Some(p.clone());
        ev
    }

    /// The grid slot is the baseline; during the race a new position must hold
    /// for POSITION_CONFIRM packets before it counts.
    fn track_race_position(&mut self, p: &Packet, paused: bool, ev: &mut Vec<EngineEvent>) {
        let pos = p.pre_race_position;
        if pos <= 0 || paused {
            return;
        }
        if p.lap == 0 {
            self.race_pos = Some(pos);
            self.pos_candidate = None;
            return;
        }
        let Some(cur) = self.race_pos else {
            self.race_pos = Some(pos);
            return;
        };
        if pos == cur {
            self.pos_candidate = None;
            return;
        }
        let c = self.pos_candidate.get_or_insert((pos, 0));
        if c.0 != pos {
            *c = (pos, 0);
        }
        c.1 += 1;
        if c.1 >= POSITION_CONFIRM {
            ev.push(EngineEvent::RacePositionChanged { from: cur, to: pos });
            self.race_pos = Some(pos);
            self.pos_candidate = None;
        }
    }

    /// Decide whether the car's new position is believable. A single wild
    /// packet is ignored; only a jump that several packets agree on, and that
    /// is impossible at the car's speed over the real elapsed time, counts.
    fn check_motion(&mut self, p: &Packet, dt_ticks: u32, fresh_lap: bool) -> Motion {
        let reach = |speed: f32, ticks: u32| speed.max(5.0) * (ticks as f32 / TICK_HZ) * 1.5 + JUMP_SLACK_M;
        let Some(a) = self.anchor.as_mut().filter(|_| !fresh_lap) else {
            self.trust(p);
            return Motion::Ok;
        };
        a.ticks += dt_ticks;
        let near_anchor = dist3(p.position, a.pos) <= reach(a.speed.max(p.speed), a.ticks);
        if let Some(s) = self.suspect.as_mut() {
            s.ticks += dt_ticks;
            if near_anchor {
                // The jump snapped back: it was a glitch.
                self.suspect = None;
                self.trust(p);
                return Motion::Ok;
            }
            let near_landing = dist3(p.position, s.landed) <= reach(p.speed, dt_ticks);
            s.landed = p.position;
            s.confirms = if near_landing { s.confirms + 1 } else { 0 };
            if s.confirms >= JUMP_CONFIRM || s.ticks >= JUMP_RESOLVE_TICKS {
                let reason = if self.paused_since_anchor { InvalidReason::RewindOrReset } else { InvalidReason::TelemetryJump };
                self.suspect = None;
                self.trust(p);
                return Motion::Reset(reason);
            }
            return Motion::Hold;
        }
        if near_anchor {
            self.trust(p);
            Motion::Ok
        } else {
            self.suspect = Some(Suspect { landed: p.position, confirms: 0, ticks: 0 });
            Motion::Hold
        }
    }

    fn trust(&mut self, p: &Packet) {
        self.anchor = Some(Anchor { pos: p.position, speed: p.speed, ticks: 0 });
        self.paused_since_anchor = false;
    }

    fn track_laps(&mut self, p: &Packet, prev: Option<&Packet>, dt_ticks: u32, dt: f32, ev: &mut Vec<EngineEvent>) {
        let prev_lap = prev.map(|x| x.lap);
        if p.lap <= 0 {
            // On the grid / pre-race. Leaving lap >= 1 for lap 0 means a restart.
            if self.lap.is_some() {
                self.end_session(ev);
            }
            self.anchor = None;
            self.suspect = None;
            return;
        }

        match self.lap.as_ref().map(|l| l.lap_no) {
            None => {
                let from_grid = prev_lap == Some(0);
                if !self.session_active || from_grid {
                    self.start_session(p, ev);
                }
                if from_grid {
                    ev.push(EngineEvent::RaceStart);
                }
                let mut lap = self.new_lap(p.lap, p, from_grid);
                lap.standing_start = from_grid;
                self.lap = Some(lap);
            }
            Some(cur) if p.lap == cur + 1 => {
                let buf = self.lap.take().unwrap();
                self.pending = Some(Pending {
                    buf,
                    prev_last_lap: prev.map_or(p.last_lap_ms, |x| x.last_lap_ms),
                    end_fuel: p.fuel,
                    fuel_capacity: p.fuel_capacity,
                    wait: 0,
                });
                let mut lap = self.new_lap(p.lap, p, true);
                if let Some(pr) = prev {
                    if dt_ticks > 6 {
                        log::info!("lap {} began after a {} ms packet gap", p.lap, dt_ticks as f32 * 1000.0 / TICK_HZ);
                    }
                    lap.entry = Some(Entry { from: pr.position, to: p.position, from_speed: pr.speed, to_speed: p.speed, gap_ticks: dt_ticks });
                }
                self.lap = Some(lap);
            }
            Some(cur) if p.lap != cur => {
                // Lap counter jumped: restart or rewind.
                if p.lap < cur && p.lap <= 1 {
                    self.start_session(p, ev);
                }
                log::info!("lap counter jumped {cur} -> {}: timing restarts mid-lap", p.lap);
                self.lap = Some(self.new_lap(p.lap, p, false));
            }
            Some(_) => {}
        }

        let fresh_lap = self.lap.as_ref().map_or(true, |l| l.ticks == 0);
        let motion = self.check_motion(p, dt_ticks, fresh_lap);
        let fuel_pct = (p.fuel_capacity > 0.0).then(|| p.fuel / p.fuel_capacity * 100.0);
        self.fuel.observe(fuel_pct, p.speed > 5.0, dt_ticks);

        let Some(buf) = self.lap.as_mut() else { return };
        if let Motion::Reset(reason) = motion {
            if buf.invalid.is_none() {
                log::warn!("lap {} invalid: {}", buf.lap_no, reason.as_str());
                buf.invalid = Some(reason);
                ev.push(EngineEvent::LapInvalidated { lap_no: buf.lap_no, reason });
            }
        }
        if let Some(pr) = prev {
            if p.fuel > pr.fuel + 0.1 {
                buf.refueled = true;
            }
        }
        let (d0, t0) = (buf.dist as f32, buf.elapsed_ms());
        buf.ticks += dt_ticks;
        if p.speed < 0.5 {
            buf.stopped_ticks += dt_ticks;
        }
        if matches!(motion, Motion::Hold) {
            return;
        }
        buf.dist += (p.speed * dt) as f64;
        let kph = p.speed_kph();
        buf.max_speed = buf.max_speed.max(kph);
        buf.decim += dt_ticks;
        if buf.decim >= SAMPLE_EVERY {
            buf.decim = 0;
            buf.samples.push(Sample {
                t: buf.elapsed_ms(),
                d: buf.dist as f32,
                x: p.position[0],
                y: p.position[1],
                z: p.position[2],
                spd: kph,
                thr: pct(p.throttle),
                brk: pct(p.brake),
                gear: p.gear,
                rpm: p.rpm,
                steer: p.wheel_rotation.unwrap_or(0.0),
                tt: p.tyre_temp,
                glat: self.g[0],
                glon: self.g[1],
            });
        }

        // Sector boundaries crossed this tick, timed by interpolation.
        let mut crossed = vec![];
        if let Some(len) = buf.sector_len {
            let (d1, t1) = (buf.dist as f32, buf.elapsed_ms());
            while buf.splits.len() < SECTOR_COUNT - 1 {
                let k = buf.splits.len() + 1;
                let boundary = len * k as f32 / SECTOR_COUNT as f32;
                if d1 < boundary {
                    break;
                }
                let f = ((boundary - d0) / (d1 - d0).max(1e-3)).clamp(0.0, 1.0);
                let t = (t0 as f32 + f * (t1 - t0) as f32).round() as u32;
                buf.splits.push(t);
                crossed.push(k);
            }
        }
        if buf.invalid.is_some() || crossed.is_empty() {
            return;
        }
        let (lap_no, splits, ref_cum) = (buf.lap_no, buf.splits.clone(), buf.ref_cum);
        for k in crossed {
            let split = self.sector_split(lap_no, k, &splits, splits[k - 1], ref_cum);
            ev.push(EngineEvent::SectorCompleted(split));
        }
    }

    /// Build the split for sector `k` ending at elapsed `cum`, and update the
    /// sector bests.
    fn sector_split(&mut self, lap_no: i16, k: usize, splits: &[u32], cum: u32, ref_cum: Option<[u32; SECTOR_COUNT]>) -> SectorSplit {
        let start = if k > 1 { splits[k - 2] } else { 0 };
        let sector_ms = cum.saturating_sub(start);
        let best = self.best_sectors[k - 1];
        let ref_sector = ref_cum.map(|r| r[k - 1].saturating_sub(if k > 1 { r[k - 2] } else { 0 }));
        if best.map_or(true, |b| sector_ms < b) {
            self.best_sectors[k - 1] = Some(sector_ms);
        }
        SectorSplit {
            lap_no,
            sector: k,
            sector_ms,
            delta_to_best_sector_ms: best.map(|b| sector_ms as i32 - b as i32),
            delta_to_reference_sector_ms: ref_sector.map(|r| sector_ms as i32 - r as i32),
            delta_to_reference_ms: ref_cum.map(|r| cum as i32 - r[k - 1] as i32),
            session_best: best.is_some_and(|b| sector_ms + PB_MARGIN_MS <= b),
        }
    }

    /// The game updates `last_lap_ms` on (or shortly after) the lap counter
    /// change, so laps are finalised once the official time arrives.
    fn settle_pending(&mut self, p: &Packet, dt_ticks: u32, ev: &mut Vec<EngineEvent>) {
        let Some(pend) = self.pending.as_mut() else { return };
        pend.wait += dt_ticks;
        let official_ready = p.last_lap_ms > 0 && p.last_lap_ms != pend.prev_last_lap;
        if !official_ready && pend.wait < 90 {
            return;
        }
        let pend = self.pending.take().unwrap();
        let mut buf = pend.buf;
        let computed = buf.elapsed_ms();
        let official = if official_ready { p.last_lap_ms as u32 } else { computed };

        let next = self.lap.as_mut().filter(|l| l.lap_no == buf.lap_no + 1);
        let joined = next.is_some_and(|next| join_at_line(&mut buf, next, official));
        if !joined && computed > 0 && official != computed {
            let scale = official as f32 / computed as f32;
            if (scale - 1.0).abs() < 0.03 {
                for s in &mut buf.samples {
                    s.t = (s.t as f32 * scale).round() as u32;
                }
            }
        }

        let is_pit = buf.refueled || buf.stopped_ticks > (TICK_HZ * 2.0) as u32;
        let length = buf.dist as f32;
        let expected_len = self.reference.as_ref().and_then(|r| r.length());
        let too_short = length < 200.0 || expected_len.is_some_and(|e| length < e * MIN_LAP_FRACTION);
        let invalid_reason = if is_pit {
            Some(InvalidReason::PitLap)
        } else if buf.invalid.is_some() {
            buf.invalid
        } else if official == 0 || too_short {
            Some(InvalidReason::InsufficientDistance)
        } else {
            None
        };
        let is_valid = invalid_reason.is_none();
        if let Some(r) = invalid_reason.filter(|r| *r != InvalidReason::PitLap) {
            log::info!("lap {} ({} ms, {:.0} m) does not count: {}", buf.lap_no, official, length, r.as_str());
        }

        // The last sector closes at the line, against the official time.
        if is_valid && buf.splits.len() == SECTOR_COUNT - 1 {
            let mut splits = buf.splits.clone();
            splits.push(official);
            let split = self.sector_split(buf.lap_no, SECTOR_COUNT, &splits, official, buf.ref_cum);
            ev.push(EngineEvent::SectorCompleted(split));
        }

        let fuel_used_pct = (!buf.refueled && pend.fuel_capacity > 0.0)
            .then(|| (buf.fuel_start - pend.end_fuel) / pend.fuel_capacity * 100.0)
            .filter(|u| *u >= 0.0);
        if let (Some(u), None) = (fuel_used_pct, buf.invalid) {
            self.fuel.record_lap(u);
        }
        self.tyres.record_lap(buf.lap_no, official, is_valid, is_pit, fuel_used_pct);

        let ctx_session_best = self.session_best;
        let ctx_reference = self.reference_ms();
        let sectors_ms = sector_times(&buf.samples, official);
        if is_valid {
            if self.session_best.map_or(true, |b| official < b) {
                self.session_best = Some(official);
            }
            if !buf.standing_start {
                self.merge_best_sectors(&sectors_ms);
            }
            if self.reference.as_ref().map_or(true, |r| official < r.time_ms) {
                self.reference = Some(RefLap { time_ms: official, samples: buf.samples.clone(), standing_start: buf.standing_start });
                self.refresh_lap_reference();
            }
        }
        self.last_lap_ms = Some(official);

        let fuel_pct = (p.fuel_capacity > 0.0).then(|| p.fuel / p.fuel_capacity * 100.0);
        let laps_remaining = (p.total_laps > 0).then(|| (p.total_laps - buf.lap_no) as i32);
        let next_lap = buf.lap_no + 1;
        self.strategy = fuel_pct.and_then(|f| self.fuel.strategy(f, next_lap, p.total_laps));
        let tyres = self.tyres.estimate();
        self.tyre_est = tyres.clone();
        self.plan = self.strategy.as_ref().and_then(|s| strategy::plan(s, &tyres, next_lap, p.total_laps));
        let ctx = LapContext {
            session_best_before: ctx_session_best,
            reference_before: ctx_reference,
            laps_remaining,
            strategy: self.strategy.clone(),
            tyres,
            plan: self.plan.clone(),
            next_lap,
            total_laps: p.total_laps,
        };
        ev.push(EngineEvent::LapCompleted(
            Box::new(CompletedLap {
                lap_no: buf.lap_no,
                time_ms: official,
                is_pit,
                is_valid,
                invalid_reason,
                sectors_ms,
                fuel_used_pct,
                max_speed: buf.max_speed,
                length_m: length,
                start_pos: buf.start_pos,
                samples: buf.samples,
            }),
            ctx,
        ));
    }

    fn update_live(&mut self, p: &Packet, paused: bool) {
        let fuel_pct = (p.fuel_capacity > 0.0).then(|| p.fuel / p.fuel_capacity * 100.0);
        let laps_remaining = (p.total_laps > 0 && p.lap > 0).then(|| (p.total_laps - p.lap + 1) as i32);
        let lap = self.lap.as_ref();
        let current_lap_ms = lap.map(|l| l.elapsed_ms());
        let lap_distance = lap.map_or(0.0, |l| l.dist as f32);
        let delta_ms = match (lap, &self.reference) {
            (Some(l), Some(r)) if l.invalid.is_none() => time_at_distance(&r.samples, lap_distance)
                .map(|t| l.elapsed_ms() as i32 - t.round() as i32),
            _ => None,
        };
        let game_best = (p.best_lap_ms > 0).then_some(p.best_lap_ms as u32);
        let best_lap_ms = match (game_best, self.session_best) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let speed = p.speed.max(0.0);
        let wheel_slip = std::array::from_fn(|i| {
            if speed < 3.0 {
                0.0
            } else {
                (p.wheel_rps[i].abs() * p.tyre_radius[i] - speed) / speed
            }
        });
        let fuel_on = self.fuel.active();
        let strategy = self.strategy.clone().filter(|_| fuel_on);

        self.live = LiveState {
            position: (p.pre_race_position > 0).then_some(p.pre_race_position),
            num_cars: (p.num_cars > 0).then_some(p.num_cars),
            lap: (p.lap > 0).then_some(p.lap),
            total_laps: (p.total_laps > 0).then_some(p.total_laps),
            gear: p.gear,
            in_gear: p.has(flags::IN_GEAR),
            suggested_gear: (p.suggested_gear != 15 && p.suggested_gear > 0).then_some(p.suggested_gear),
            speed_kph: p.speed_kph(),
            rpm: p.rpm,
            rpm_alert_min: p.rpm_alert_min as f32,
            rpm_alert_max: p.rpm_alert_max as f32,
            throttle: pct(p.throttle),
            brake: pct(p.brake),
            steer: p.wheel_rotation,
            tyre_temp: p.tyre_temp,
            wheel_slip,
            current_lap_ms,
            last_lap_ms: (p.last_lap_ms > 0).then_some(p.last_lap_ms as u32).or(self.last_lap_ms),
            best_lap_ms,
            delta_ms,
            lap_distance,
            lap_fraction: lap.and_then(|l| l.sector_len.map(|len| lap_distance / len)),
            lap_invalid: lap.and_then(|l| l.invalid),
            fuel_pct,
            fuel_per_lap_pct: self.fuel.per_lap(),
            fuel_laps_left: fuel_pct.and_then(|f| self.fuel.laps_left(f)),
            fuel_state: self.fuel.state(),
            laps_remaining,
            pit_window: strategy.as_ref().map(|s| s.window),
            pit_plan: self.plan.clone().filter(|_| fuel_on),
            strategy,
            tyre_estimate: self.tyre_est.clone(),
            g_lat: self.g[0],
            g_lon: self.g[1],
            boost: p.boost,
            water_temp: p.water_temp,
            oil_temp: p.oil_temp,
            on_track: p.has(flags::CAR_ON_TRACK),
            paused,
            rev_limit: p.has(flags::REV_LIMIT),
            tcs: p.has(flags::TCS),
            asm: p.has(flags::ASM),
            car_code: p.car_code,
        };
    }
}

/// Place the start/finish line between the last packet of `old` and the
/// first of `next`, using the official lap time to split the gap. `old` gets
/// a closing sample at the line, and `next` is re-based so it starts there
/// (t = 0, d = 0) instead of at its first packet. Without this a packet gap
/// at the line leaves a hole in both laps and shifts the next lap's timing.
fn join_at_line(old: &mut LapBuf, next: &mut LapBuf, official: u32) -> bool {
    let Some(e) = next.entry.take() else { return false };
    let (Some(&last), Some(&first)) = (old.samples.last(), next.samples.first()) else { return false };
    let gap_ms = e.gap_ticks as f32 * 1000.0 / TICK_HZ;
    let tau = official as f32 - old.elapsed_ms() as f32;
    if tau < 0.0 || tau > gap_ms + 50.0 {
        return false;
    }
    let avg = (e.from_speed + e.to_speed) / 2.0;
    if dist3(e.from, e.to) > avg.max(5.0) * gap_ms / 1000.0 * 1.5 + JUMP_SLACK_M {
        return false;
    }
    let f = (tau / gap_ms.max(1.0)).clamp(0.0, 1.0);
    let line: [f32; 3] = std::array::from_fn(|i| e.from[i] + (e.to[i] - e.from[i]) * f);
    let spd = (e.from_speed + (e.to_speed - e.from_speed) * f) * 3.6;

    // Close the old lap at the line.
    old.dist += (avg * tau / 1000.0) as f64;
    old.samples.push(Sample { t: official, d: old.dist as f32, x: line[0], y: line[1], z: line[2], spd, ..last });

    // Re-base the new lap: drop the part of the gap before the line.
    let tau_ticks = ((tau * TICK_HZ / 1000.0).round() as u32).min(e.gap_ticks).min(next.ticks);
    next.ticks -= tau_ticks;
    let shift_ms = (tau_ticks as f32 * 1000.0 / TICK_HZ).round() as u32;
    let true_d = avg * (gap_ms - tau).max(0.0) / 1000.0;
    let shift_d = first.d - true_d;
    next.dist = (next.dist - shift_d as f64).max(0.0);
    for s in &mut next.samples {
        s.t = s.t.saturating_sub(shift_ms);
        s.d = (s.d - shift_d).max(0.0);
    }
    for t in &mut next.splits {
        *t = t.saturating_sub(shift_ms);
    }
    next.samples.insert(0, Sample { t: 0, d: 0.0, x: line[0], y: line[1], z: line[2], spd, ..first });
    true
}

fn pct(v: u8) -> u8 {
    ((v as u32 * 100 + 127) / 255) as u8
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Rotate `v` by the inverse of unit quaternion `q` (x, y, z, w): world -> car frame.
fn rotate_inverse(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let (qx, qy, qz, w) = (-q[0], -q[1], -q[2], q[3]);
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let u = [qx, qy, qz];
    let t = cross(u, v).map(|c| c * 2.0);
    let ut = cross(u, t);
    [v[0] + w * t[0] + ut[0], v[1] + w * t[1] + ut[1], v[2] + w * t[2] + ut[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAP_M: f32 = 500.0;

    struct Drive {
        id: i32,
        fuel: f32,
        last_lap: i32,
    }

    /// Drive a straight 500 m "lap" at `speeds[lap - 1]` m/s for each lap,
    /// from the grid. `tweak(lap, tick, packet)` can edit a packet, or return
    /// false to drop it.
    fn drive(e: &mut Engine, speeds: &[f32], mut tweak: impl FnMut(i16, i32, &mut Packet) -> bool) -> Vec<EngineEvent> {
        let laps = speeds.len() as i16;
        let mut out = vec![];
        let mut d = Drive { id: 0, fuel: 100.0, last_lap: -1 };
        for _ in 0..30 {
            d.id += 1;
            out.extend(e.on_packet(&Packet { packet_id: d.id, lap: 0, fuel: d.fuel, fuel_capacity: 100.0, total_laps: laps, last_lap_ms: -1, ..Default::default() }));
        }
        for lap in 1..=laps + 1 {
            let v = speeds[(lap as usize - 1).min(speeds.len() - 1)];
            let lap_ticks = if lap > laps { 120 } else { (LAP_M / v * 60.0).round() as i32 };
            for i in 0..lap_ticks {
                d.id += 1;
                d.fuel -= 0.01;
                let mut p = Packet {
                    packet_id: d.id,
                    lap,
                    total_laps: laps,
                    speed: v,
                    position: [i as f32 * v / 60.0, 0.0, 0.0],
                    fuel: d.fuel,
                    fuel_capacity: 100.0,
                    last_lap_ms: d.last_lap,
                    flags: flags::CAR_ON_TRACK,
                    ..Default::default()
                };
                if tweak(lap, i, &mut p) {
                    out.extend(e.on_packet(&p));
                }
            }
            d.last_lap = (lap_ticks as f32 * 1000.0 / 60.0) as i32 + 7;
        }
        out
    }

    fn laps(ev: &[EngineEvent]) -> Vec<&CompletedLap> {
        ev.iter()
            .filter_map(|e| match e {
                EngineEvent::LapCompleted(l, _) => Some(&**l),
                _ => None,
            })
            .collect()
    }

    fn splits(ev: &[EngineEvent], lap_no: i16) -> Vec<&SectorSplit> {
        ev.iter()
            .filter_map(|e| match e {
                EngineEvent::SectorCompleted(s) if s.lap_no == lap_no => Some(s),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn records_laps_with_official_time() {
        let mut e = Engine::new();
        let ev = drive(&mut e, &[50.0; 3], |_, _, _| true);
        let laps = laps(&ev);
        assert!(ev.iter().any(|e| matches!(e, EngineEvent::RaceStart)));
        assert_eq!(laps.len(), 3);
        assert_eq!(laps[0].time_ms, 10_007);
        assert!(laps.iter().all(|l| l.is_valid && !l.is_pit && l.invalid_reason.is_none()));
        assert!((laps[0].length_m - 500.0).abs() < 1.0);
        assert_eq!(laps[0].sectors_ms.iter().sum::<u32>(), 10_007);
        assert!(laps[1].samples.len() > 250);
        // 6 % per lap of a 100 L tank.
        assert!((e.fuel.per_lap().unwrap() - 6.0).abs() < 0.2);
    }

    #[test]
    fn isolated_spike_does_not_invalidate() {
        let mut e = Engine::new();
        let ev = drive(&mut e, &[50.0; 3], |lap, i, p| {
            if lap == 2 && i == 300 {
                p.position[0] += 800.0;
            }
            true
        });
        assert!(laps(&ev).iter().all(|l| l.is_valid), "{:?}", laps(&ev).iter().map(|l| l.invalid_reason).collect::<Vec<_>>());
    }

    #[test]
    fn dropout_does_not_invalidate() {
        // A second of lost packets at speed moves the car 50 m between packets.
        let mut e = Engine::new();
        let ev = drive(&mut e, &[50.0; 3], |lap, i, _| !(lap == 2 && (200..260).contains(&i)));
        let l = laps(&ev);
        assert!(l.iter().all(|l| l.is_valid));
        assert!((l[1].length_m - 500.0).abs() < 5.0, "dropout distance {}", l[1].length_m);
    }

    #[test]
    fn late_packet_is_ignored() {
        // The last packet of lap 1 arrives after lap 2 has started.
        let mut e = Engine::new();
        let mut held: Option<Packet> = None;
        let ev = drive(&mut e, &[50.0; 3], |lap, i, p| {
            if lap == 1 && i == 599 {
                held = Some(p.clone());
                return false;
            }
            if lap == 2 && i == 1 {
                *p = held.take().unwrap();
            }
            true
        });
        let l = laps(&ev);
        assert_eq!(l.len(), 3);
        assert!(l.iter().all(|l| l.is_valid), "{:?}", l.iter().map(|l| l.invalid_reason).collect::<Vec<_>>());
    }

    #[test]
    fn real_reset_invalidates() {
        let mut e = Engine::new();
        let mut paused = false;
        let ev = drive(&mut e, &[50.0; 3], |lap, i, p| {
            if lap == 2 && (300..330).contains(&i) {
                p.flags |= flags::PAUSED;
                paused = true;
            }
            if lap == 2 && i >= 330 {
                // Rewound 200 m and carried on from there.
                p.position[0] -= 200.0;
            }
            true
        });
        assert!(paused);
        let l = laps(&ev);
        assert!(l[0].is_valid && l[2].is_valid);
        assert_eq!(l[1].invalid_reason, Some(InvalidReason::RewindOrReset));
        assert!(ev.iter().any(|e| matches!(e, EngineEvent::LapInvalidated { lap_no: 2, .. })));
    }

    #[test]
    fn joined_mid_lap_and_pit_reasons() {
        let mut e = Engine::new();
        // Join on lap 3 without seeing the grid.
        let mut ev = vec![];
        for i in 0..700 {
            let lap = if i < 300 { 3 } else { 4 };
            ev.extend(e.on_packet(&Packet {
                packet_id: i + 1,
                lap,
                total_laps: 8,
                speed: 50.0,
                position: [(i % 300) as f32, 0.0, 0.0],
                fuel: 50.0,
                fuel_capacity: 100.0,
                last_lap_ms: if lap == 4 { 61_000 } else { 60_000 },
                flags: flags::CAR_ON_TRACK,
                ..Default::default()
            }));
        }
        assert_eq!(laps(&ev)[0].invalid_reason, Some(InvalidReason::JoinedMidLap));

        let mut e = Engine::new();
        let ev = drive(&mut e, &[50.0; 3], |lap, i, p| {
            if lap == 2 && (100..300).contains(&i) {
                p.speed = 0.0;
            }
            true
        });
        let l = laps(&ev);
        assert!(l[1].is_pit);
        assert_eq!(l[1].invalid_reason, Some(InvalidReason::PitLap));
    }

    #[test]
    fn live_sectors_fire_once_and_add_up() {
        let mut e = Engine::new();
        // A slow standing-start lap, two identical laps, then a quicker one.
        let ev = drive(&mut e, &[45.0, 50.0, 50.0, 55.0], |_, _, _| true);
        // Lap 1 has no reference yet, so sectors start on lap 2.
        assert!(splits(&ev, 1).is_empty());
        for lap in [2, 3, 4] {
            let s = splits(&ev, lap);
            assert_eq!(s.iter().map(|s| s.sector).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
            let total: u32 = s.iter().map(|s| s.sector_ms).sum();
            assert_eq!(total, laps(&ev)[lap as usize - 1].time_ms);
        }
        // The standing start is no sector benchmark.
        assert!(splits(&ev, 2).iter().all(|s| s.delta_to_reference_sector_ms.is_none() && !s.session_best));
        // Identical lap 3: deltas near zero, no new bests.
        for s in splits(&ev, 3).iter().take(4) {
            assert!(s.delta_to_reference_sector_ms.unwrap().abs() <= 20, "{s:?}");
            assert!(!s.session_best);
        }
        // Lap 4 at 55 m/s: each 100 m sector is ~180 ms quicker, and a best.
        let s4 = splits(&ev, 4);
        for s in s4.iter().take(4) {
            let d = s.delta_to_reference_sector_ms.unwrap();
            assert!((-200..=-160).contains(&d), "{s:?}");
            assert!(s.session_best);
        }
        assert!(s4[3].delta_to_reference_ms.unwrap() < -700);
    }

    #[test]
    fn strategy_on_lap_completion() {
        let mut e = Engine::new();
        let ev = drive(&mut e, &[50.0; 3], |_, _, _| true);
        let ctx: Vec<_> = ev
            .iter()
            .filter_map(|e| match e {
                EngineEvent::LapCompleted(_, c) => Some(c),
                _ => None,
            })
            .collect();
        // After lap 2 of 3 with ~88 % left: no stop.
        assert_eq!(ctx[1].next_lap, 3);
        let s = ctx[1].strategy.as_ref().unwrap();
        assert_eq!(s.window, PitWindow::NoStop);
    }

    #[test]
    fn packet_gap_at_the_line_is_closed() {
        // A long straight: laps are 500 m of continuous road, and 160 packets
        // (2.7 s) go missing around each line crossing.
        let mut e = Engine::new();
        let v = 50.0;
        let mut ev = vec![];
        let mut id = 0;
        for _ in 0..30 {
            id += 1;
            ev.extend(e.on_packet(&Packet { packet_id: id, lap: 0, total_laps: 3, last_lap_ms: -1, fuel: 50.0, fuel_capacity: 100.0, ..Default::default() }));
        }
        let lap_ticks = 600; // 10 s a lap
        for tick in 0..lap_ticks * 3 + 200 {
            id += 1;
            let lap = (tick / lap_ticks + 1) as i16;
            let into = tick % lap_ticks;
            // Lost: the last 80 packets before each line and the first 80 after.
            if into >= lap_ticks - 80 || (lap > 1 && into < 80) {
                continue;
            }
            let last_lap_ms = if lap == 1 { -1 } else { 9_998 + lap as i32 };
            ev.extend(e.on_packet(&Packet {
                packet_id: id,
                lap,
                total_laps: 3,
                speed: v,
                position: [tick as f32 * v / 60.0, 0.0, 0.0],
                fuel: 50.0,
                fuel_capacity: 100.0,
                last_lap_ms,
                flags: flags::CAR_ON_TRACK,
                ..Default::default()
            }));
        }
        let l = laps(&ev);
        assert!(l.len() >= 2);
        let lap2 = l[1];
        let (a, b) = (lap2.samples.first().unwrap(), lap2.samples.last().unwrap());
        // Starts and ends on the line: x = 500 and x = 1000.
        assert_eq!((a.t, a.d), (0, 0.0));
        assert!((a.x - 500.0).abs() < 2.0, "start {}", a.x);
        assert!((b.x - 1000.0).abs() < 2.0, "end {}", b.x);
        assert_eq!(b.t, 10_001);
        assert!((b.d - 500.0).abs() < 2.0, "length {}", b.d);
        // The first real packet, 80 ticks after the line: 1.333 s, 66.7 m.
        let s1 = lap2.samples[1];
        assert!((s1.t as i32 - 1333).abs() <= 20 && (s1.d - 66.7).abs() < 2.0, "{s1:?}");
        assert!(lap2.is_valid);
    }

    #[test]
    fn identity_rotation() {
        assert_eq!(rotate_inverse([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }
}
