//! Turns the raw 60 Hz packet stream into lap timing, recorded laps, fuel
//! strategy and the live dashboard state.

use serde::Serialize;

use super::fuel::{FuelModel, PitWindow};
use super::models::{sector_times, time_at_distance, CompletedLap, Sample};
use crate::telemetry::packet::{flags, Packet};

const TICK_HZ: f32 = 60.0;
/// Record every Nth tick (60 Hz / 2 = 30 Hz).
const SAMPLE_EVERY: u32 = 2;
/// A position jump bigger than this in a single tick means a rewind/reset.
const TELEPORT_M: f32 = 40.0;

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
    pub fuel_pct: Option<f32>,
    pub fuel_per_lap_pct: Option<f32>,
    pub fuel_laps_left: Option<f32>,
    pub laps_remaining: Option<i32>,
    pub pit_window: Option<PitWindow>,
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
    pub laps_remaining: Option<i32>,
    pub fuel_laps_left: Option<f32>,
    pub pit_window: PitWindow,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    SessionStarted { car_code: i32, total_laps: i16 },
    SessionEnded,
    RaceStart,
    LapCompleted(Box<CompletedLap>, LapContext),
    PositionChanged { to: i16 },
}

#[derive(Debug, Clone)]
pub struct RefLap {
    pub time_ms: u32,
    pub samples: Vec<Sample>,
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
    seen_start: bool,
    invalid: bool,
    start_pos: [f32; 3],
    max_speed: f32,
    decim: u32,
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
            seen_start,
            invalid: false,
            start_pos: p.position,
            max_speed: 0.0,
            decim: SAMPLE_EVERY, // record the first tick
        }
    }

    fn elapsed_ms(&self) -> u32 {
        (self.ticks as f32 * 1000.0 / TICK_HZ).round() as u32
    }
}

struct Pending {
    buf: LapBuf,
    prev_last_lap: i32,
    end_fuel: f32,
    fuel_capacity: f32,
    wait: u32,
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
    session_best: Option<u32>,
    reference: Option<RefLap>,
    last_lap_ms: Option<u32>,
    g: [f32; 2],
    live: LiveState,
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
            self.reference = Some(r);
        }
    }

    pub fn reference_ms(&self) -> Option<u32> {
        self.reference.as_ref().map(|r| r.time_ms)
    }

    fn start_session(&mut self, p: &Packet, ev: &mut Vec<EngineEvent>) {
        if self.session_active {
            ev.push(EngineEvent::SessionEnded);
        }
        self.session_active = true;
        self.car_code = p.car_code;
        self.fuel.reset();
        self.session_best = None;
        self.reference = None;
        self.last_lap_ms = None;
        self.pending = None;
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
        let dt_ticks = match self.last_id {
            Some(id) if p.packet_id == id => return ev,
            Some(id) if p.packet_id > id && p.packet_id - id <= 30 => (p.packet_id - id) as u32,
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

        if self.session_active && p.car_code != self.car_code {
            self.end_session(&mut ev);
        }

        if !paused {
            self.track_laps(p, prev.as_ref(), dt_ticks, dt, &mut ev);
        }
        self.settle_pending(p, dt_ticks, &mut ev);
        self.update_live(p, paused);
        self.last = Some(p.clone());
        ev
    }

    fn track_laps(&mut self, p: &Packet, prev: Option<&Packet>, dt_ticks: u32, dt: f32, ev: &mut Vec<EngineEvent>) {
        let prev_lap = prev.map(|x| x.lap);
        if p.lap <= 0 {
            // On the grid / pre-race. Leaving lap >= 1 for lap 0 means a restart.
            if self.lap.is_some() {
                self.end_session(ev);
            }
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
                self.lap = Some(LapBuf::new(p.lap, p, from_grid));
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
                self.lap = Some(LapBuf::new(p.lap, p, true));
            }
            Some(cur) if p.lap != cur => {
                // Lap counter jumped: restart or rewind.
                if p.lap < cur && p.lap <= 1 {
                    self.start_session(p, ev);
                }
                self.lap = Some(LapBuf::new(p.lap, p, false));
            }
            Some(_) => {}
        }

        let Some(buf) = self.lap.as_mut() else { return };
        if let Some(pr) = prev {
            let jump = dist3(p.position, pr.position);
            if jump > TELEPORT_M.max(p.speed * dt * 4.0) && buf.ticks > 0 {
                buf.invalid = true;
            }
            if p.fuel > pr.fuel + 0.1 {
                buf.refueled = true;
            }
        }
        buf.ticks += dt_ticks;
        buf.dist += (p.speed * dt) as f64;
        if p.speed < 0.5 {
            buf.stopped_ticks += dt_ticks;
        }
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

        if computed > 0 && official != computed {
            let scale = official as f32 / computed as f32;
            if (scale - 1.0).abs() < 0.03 {
                for s in &mut buf.samples {
                    s.t = (s.t as f32 * scale).round() as u32;
                }
            }
        }

        let is_pit = buf.refueled || buf.stopped_ticks > (TICK_HZ * 2.0) as u32;
        let length = buf.dist as f32;
        let is_valid = !is_pit && buf.seen_start && !buf.invalid && official > 0 && length > 200.0;
        let fuel_used_pct = (!buf.refueled && pend.fuel_capacity > 0.0)
            .then(|| (buf.fuel_start - pend.end_fuel) / pend.fuel_capacity * 100.0)
            .filter(|u| *u >= 0.0);
        if let (Some(u), true) = (fuel_used_pct, buf.seen_start && !buf.invalid) {
            self.fuel.record_lap(u);
        }

        let ctx_session_best = self.session_best;
        let ctx_reference = self.reference_ms();
        if is_valid {
            if self.session_best.map_or(true, |b| official < b) {
                self.session_best = Some(official);
            }
            if self.reference.as_ref().map_or(true, |r| official < r.time_ms) {
                self.reference = Some(RefLap { time_ms: official, samples: buf.samples.clone() });
            }
        }
        self.last_lap_ms = Some(official);

        let fuel_pct = (p.fuel_capacity > 0.0).then(|| p.fuel / p.fuel_capacity * 100.0);
        let laps_remaining = (p.total_laps > 0).then(|| (p.total_laps - buf.lap_no) as i32);
        let ctx = LapContext {
            session_best_before: ctx_session_best,
            reference_before: ctx_reference,
            laps_remaining,
            fuel_laps_left: fuel_pct.and_then(|f| self.fuel.laps_left(f)),
            pit_window: fuel_pct.map_or(PitWindow::None, |f| self.fuel.pit_window(f, laps_remaining)),
        };
        let sectors_ms = sector_times(&buf.samples, official);
        ev.push(EngineEvent::LapCompleted(
            Box::new(CompletedLap {
                lap_no: buf.lap_no,
                time_ms: official,
                is_pit,
                is_valid,
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
            (Some(l), Some(r)) if l.seen_start && !l.invalid => time_at_distance(&r.samples, lap_distance)
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
            fuel_pct,
            fuel_per_lap_pct: self.fuel.per_lap(),
            fuel_laps_left: fuel_pct.and_then(|f| self.fuel.laps_left(f)),
            laps_remaining,
            pit_window: fuel_pct.map(|f| self.fuel.pit_window(f, laps_remaining)),
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

    /// Drive in a straight line at 50 m/s, crossing the line every `lap_ticks`.
    fn drive(engine: &mut Engine, laps: i16, lap_ticks: i32) -> Vec<EngineEvent> {
        let mut out = vec![];
        let mut id = 0;
        let mut fuel = 100.0;
        let mut last_lap = -1;
        // Grid
        for _ in 0..30 {
            id += 1;
            out.extend(engine.on_packet(&Packet { packet_id: id, lap: 0, fuel, fuel_capacity: 100.0, total_laps: laps, last_lap_ms: last_lap, ..Default::default() }));
        }
        for lap in 1..=laps + 1 {
            for i in 0..lap_ticks {
                id += 1;
                fuel -= 0.01;
                let x = (i as f32) * 50.0 / 60.0;
                out.extend(engine.on_packet(&Packet {
                    packet_id: id,
                    lap,
                    total_laps: laps,
                    speed: 50.0,
                    position: [x, 0.0, 0.0],
                    fuel,
                    fuel_capacity: 100.0,
                    last_lap_ms: last_lap,
                    flags: flags::CAR_ON_TRACK,
                    ..Default::default()
                }));
            }
            last_lap = (lap_ticks as f32 * 1000.0 / 60.0) as i32 + 7;
        }
        out
    }

    #[test]
    fn records_laps_with_official_time() {
        let mut e = Engine::new();
        let ev = drive(&mut e, 3, 600);
        let laps: Vec<_> = ev
            .iter()
            .filter_map(|e| match e {
                EngineEvent::LapCompleted(l, _) => Some(l),
                _ => None,
            })
            .collect();
        assert!(ev.iter().any(|e| matches!(e, EngineEvent::RaceStart)));
        assert_eq!(laps.len(), 3);
        assert_eq!(laps[0].time_ms, 10_007);
        assert!(laps.iter().all(|l| l.is_valid && !l.is_pit));
        assert!((laps[0].length_m - 500.0).abs() < 1.0);
        assert_eq!(laps[0].sectors_ms.iter().sum::<u32>(), 10_007);
        assert!(laps[1].samples.len() > 250);
        // 6 L per lap of a 100 L tank.
        assert!((e.fuel.per_lap().unwrap() - 6.0).abs() < 0.2);
    }

    #[test]
    fn identity_rotation() {
        assert_eq!(rotate_inverse([0.0, 0.0, 0.0, 1.0], [1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);
    }
}
