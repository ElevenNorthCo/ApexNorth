//! Demo mode: a fake PlayStation on 127.0.0.1 that answers heartbeats with
//! encrypted GT7 packets from a simple lap simulation. Lets the whole app
//! (link, dashboard, recording, analysis, voice) run without a console.

use std::f32::consts::TAU;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::crypto::{self, Variant};
use super::packet::{flags, Packet};
use super::udp::SEND_PORT;

const DT: f32 = 1.0 / 60.0;
const RACE_LAPS: i16 = 10;
const CAR_CODE: i32 = 3442;
const GEAR_TOP_KPH: [f32; 7] = [0.0, 75.0, 115.0, 155.0, 195.0, 240.0, 300.0];

struct Track {
    pts: Vec<[f32; 2]>,
    vmax: Vec<f32>,
    len: f32,
}

impl Track {
    /// A closed, irregular circuit of about 4 km with a speed profile from a
    /// forward/backward acceleration pass.
    fn build() -> Track {
        let n = 20_000;
        let raw: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let th = i as f32 / n as f32 * TAU;
                let r = 620.0 * (1.0 + 0.28 * (2.0 * th).sin() + 0.12 * (3.0 * th + 1.0).cos() + 0.07 * (5.0 * th + 0.4).sin());
                [r * th.cos(), r * th.sin() * 0.7]
            })
            .collect();
        // Resample at 1 m spacing.
        let mut pts = vec![raw[0]];
        let mut acc = 0.0;
        for w in 0..n {
            let a = raw[w];
            let b = raw[(w + 1) % n];
            acc += ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            if acc >= 1.0 {
                pts.push(b);
                acc -= 1.0;
            }
        }
        let m = pts.len();
        let curv: Vec<f32> = (0..m)
            .map(|i| {
                let p0 = pts[(i + m - 8) % m];
                let p1 = pts[i];
                let p2 = pts[(i + 8) % m];
                let a = ((p1[0] - p0[0]).powi(2) + (p1[1] - p0[1]).powi(2)).sqrt();
                let b = ((p2[0] - p1[0]).powi(2) + (p2[1] - p1[1]).powi(2)).sqrt();
                let c = ((p2[0] - p0[0]).powi(2) + (p2[1] - p0[1]).powi(2)).sqrt();
                let area2 = ((p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0])).abs();
                (2.0 * area2) / (a * b * c).max(1e-3)
            })
            .collect();
        let mut vmax: Vec<f32> = curv.iter().map(|k| (13.0 / k.max(1e-4)).sqrt().min(82.0)).collect();
        for _ in 0..2 {
            for i in 0..m {
                let v = vmax[i];
                let a = 7.5 * (1.0 - v / 90.0) + 0.8;
                let next = (i + 1) % m;
                vmax[next] = vmax[next].min((v * v + 2.0 * a).sqrt());
            }
            for i in (0..m).rev() {
                let next = (i + 1) % m;
                vmax[i] = vmax[i].min((vmax[next].powi(2) + 2.0 * 15.0).sqrt());
            }
        }
        Track { len: m as f32, pts, vmax }
    }

    fn at(&self, s: f32) -> ([f32; 2], [f32; 2], f32) {
        let m = self.pts.len();
        let i = (s.max(0.0) as usize) % m;
        let j = (i + 1) % m;
        let f = s.fract();
        let (a, b) = (self.pts[i], self.pts[j]);
        let pos = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
        let t = [b[0] - a[0], b[1] - a[1]];
        let n = (t[0] * t[0] + t[1] * t[1]).sqrt().max(1e-6);
        (pos, [t[0] / n, t[1] / n], self.vmax[i] + (self.vmax[j] - self.vmax[i]) * f)
    }
}

/// Small deterministic PRNG so the demo needs no extra crates.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

struct Car {
    track: Track,
    rng: Rng,
    packet_id: i32,
    s: f32,
    v: f32,
    lap: i16,
    lap_ticks: u32,
    grid_ticks: u32,
    pace: f32,
    best: i32,
    last: i32,
    fuel: f32,
    tyre: [f32; 4],
    pit_stop_ticks: u32,
    pit_this_lap: bool,
    tod_ms: i32,
    /// Laps on this set of tyres: pace fades and temperatures climb.
    stint_laps: u32,
    /// Scripted spin: ticks left and heading offset.
    spin_ticks: u32,
    spun: bool,
    yaw_offset: f32,
    prev_throttle: f32,
    prev_brake: f32,
    rear_slip_ticks: u32,
    front_lock_ticks: u32,
}

impl Car {
    fn new() -> Car {
        Car {
            track: Track::build(),
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            packet_id: 0,
            s: 0.0,
            v: 0.0,
            lap: 0,
            lap_ticks: 0,
            grid_ticks: 0,
            pace: 0.97,
            best: -1,
            last: -1,
            fuel: 45.0,
            tyre: [45.0; 4],
            pit_stop_ticks: 0,
            pit_this_lap: false,
            tod_ms: 14 * 3_600_000,
            stint_laps: 0,
            spin_ticks: 0,
            spun: false,
            yaw_offset: 0.0,
            prev_throttle: 0.0,
            prev_brake: 0.0,
            rear_slip_ticks: 0,
            front_lock_ticks: 0,
        }
    }

    fn reset_race(&mut self) {
        self.s = 0.0;
        self.v = 0.0;
        self.lap = 0;
        self.lap_ticks = 0;
        self.grid_ticks = 0;
        self.best = -1;
        self.last = -1;
        self.fuel = 45.0;
        self.stint_laps = 0;
        self.spun = false;
        self.pace = 0.97;
        self.tyre = [45.0; 4];
    }

    fn step(&mut self) -> Packet {
        self.packet_id += 1;
        self.tod_ms += 16;
        let (_, _, vprof) = self.track.at(self.s);
        let mut throttle = 0.0f32;
        let mut brake = 0.0f32;

        if self.lap == 0 {
            // Grid: 4 seconds of waiting, then lights out.
            self.grid_ticks += 1;
            if self.grid_ticks > 240 {
                self.lap = 1;
            }
        } else if self.pit_this_lap && self.s < 60.0 {
            // Pit box: crawl in, stop and refuel.
            if self.v > 0.5 && self.pit_stop_ticks == 0 {
                self.v = (self.v - 15.0 * DT).max(0.0);
                brake = 0.8;
            } else {
                self.v = 0.0;
                self.pit_stop_ticks += 1;
                self.fuel = (self.fuel + 0.35).min(100.0);
                if self.pit_stop_ticks > 300 {
                    self.pit_this_lap = false;
                    self.pit_stop_ticks = 0;
                    self.s = 60.0;
                    // Fresh tyres, a little cooler than the old ones.
                    self.stint_laps = 0;
                    self.tyre = [62.0; 4];
                }
            }
            self.lap_ticks += 1;
        } else if self.spin_ticks > 0 {
            // A half-second of oversteer and a full rotation, scrubbing speed.
            self.spin_ticks -= 1;
            self.yaw_offset += TAU / 90.0;
            self.v = (self.v - 22.0 * DT).max(6.0);
            brake = 0.2;
            self.s += self.v * DT;
            self.lap_ticks += 1;
            self.tyre[2] += 0.3;
            self.tyre[3] += 0.3;
            if self.spin_ticks == 0 {
                self.yaw_offset = 0.0;
            }
        } else {
            if self.lap == 8 && !self.spun && self.s > 1800.0 {
                self.spun = true;
                self.spin_ticks = 90;
            }
            let target = vprof * self.pace;
            let accel = 7.5 * (1.0 - self.v / 90.0) + 0.8;
            if target > self.v + 0.3 {
                self.v = (self.v + accel * DT).min(target);
                throttle = 1.0;
            } else if target < self.v - 0.3 {
                let decel = ((self.v - target) / DT).min(15.0);
                self.v -= decel * DT;
                brake = (decel / 15.0).clamp(0.1, 1.0);
            } else {
                self.v = target;
                throttle = 0.45 + 0.2 * self.rng.next();
            }
            self.s += self.v * DT;
            self.lap_ticks += 1;
            self.fuel = (self.fuel - (0.0024 * throttle + 0.0003)).max(0.0);

            if self.s >= self.track.len {
                self.s -= self.track.len;
                self.last = (self.lap_ticks as f32 * 1000.0 / 60.0).round() as i32;
                if self.best < 0 || self.last < self.best {
                    self.best = self.last;
                }
                self.lap_ticks = 0;
                self.lap += 1;
                self.stint_laps += 1;
                // Tyre wear costs almost one percent a lap, plus a little noise.
                self.pace = 0.99 - 0.008 * self.stint_laps as f32 - 0.002 * self.rng.next();
                // Pit with about two laps of fuel left, like a driver following the plan.
                self.pit_this_lap = self.fuel < 13.0;
                if self.lap > RACE_LAPS {
                    self.reset_race();
                }
            }
        }

        let (pos, tan, _) = self.track.at(self.s);
        let kph = self.v * 3.6;
        let gear = if self.v < 0.1 && self.lap == 0 {
            1
        } else {
            (1..GEAR_TOP_KPH.len()).find(|&g| kph < GEAR_TOP_KPH[g]).unwrap_or(6) as u8
        };
        let lo = GEAR_TOP_KPH[gear as usize - 1] * 0.7;
        let hi = GEAR_TOP_KPH[gear as usize];
        let rpm = (2200.0 + (kph - lo).max(0.0) / (hi - lo) * 6200.0).min(8500.0);

        // Tyres warm with speed and braking; fronts run hotter.
        let load = kph / 300.0 + brake * 0.6;
        for (i, t) in self.tyre.iter_mut().enumerate() {
            let front = if i < 2 { 8.0 } else { 0.0 };
            let target = 52.0 + 45.0 * load + front + 2.0 * self.stint_laps as f32;
            *t += (target - *t) * 0.004;
        }

        // Now and then, a big slide on exit or a lock-up into a corner.
        if throttle > 0.9 && self.prev_throttle < 0.9 && gear <= 3 && self.rng.next() < 0.2 {
            self.rear_slip_ticks = 30;
        }
        if brake > 0.5 && self.prev_brake < 0.1 && self.rng.next() < 0.12 {
            self.front_lock_ticks = 20;
        }
        self.prev_throttle = throttle;
        self.prev_brake = brake;
        self.rear_slip_ticks = self.rear_slip_ticks.saturating_sub(1);
        self.front_lock_ticks = self.front_lock_ticks.saturating_sub(1);
        let radius = 0.33;
        let rear_slip = if self.rear_slip_ticks > 0 && throttle > 0.5 {
            0.45
        } else if throttle > 0.9 && gear <= 2 {
            0.12
        } else {
            0.02 * throttle
        };
        let front_slip = if self.front_lock_ticks > 0 && brake > 0.3 {
            -0.5
        } else if brake > 0.9 {
            -0.15
        } else {
            0.0
        };
        let wheel = |slip: f32| self.v * (1.0 + slip) / radius;
        let yaw = tan[0].atan2(tan[1]) + self.yaw_offset;

        let mut fl = flags::CAR_ON_TRACK | flags::IN_GEAR;
        if rpm > 8200.0 {
            fl |= flags::REV_LIMIT;
        }
        Packet {
            position: [pos[0], 0.0, pos[1]],
            velocity: [tan[0] * self.v, 0.0, tan[1] * self.v],
            rotation: [0.0, (yaw / 2.0).sin(), 0.0, (yaw / 2.0).cos()],
            angular_velocity: [0.0, if self.spin_ticks > 0 { TAU / 1.5 } else { 0.0 }, 0.0],
            rpm,
            fuel: self.fuel,
            fuel_capacity: 100.0,
            speed: self.v,
            water_temp: 88.0,
            oil_temp: 102.0,
            tyre_temp: self.tyre,
            packet_id: self.packet_id,
            lap: self.lap,
            total_laps: RACE_LAPS,
            best_lap_ms: self.best,
            last_lap_ms: self.last,
            time_of_day_ms: self.tod_ms,
            pre_race_position: if self.lap == 0 { 5 } else { -1 },
            num_cars: if self.lap == 0 { 16 } else { -1 },
            rpm_alert_min: 7600,
            rpm_alert_max: 8300,
            max_speed_kph: 300,
            flags: fl,
            gear,
            suggested_gear: 15,
            throttle: (throttle * 255.0) as u8,
            brake: (brake * 255.0) as u8,
            wheel_rps: [wheel(front_slip), wheel(front_slip), wheel(rear_slip), wheel(rear_slip)],
            tyre_radius: [radius; 4],
            car_code: CAR_CODE,
            wheel_rotation: Some(0.0),
            ..Default::default()
        }
    }
}

pub struct Simulator {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Simulator {
    pub fn start() -> std::io::Result<Simulator> {
        let sock = UdpSocket::bind(("127.0.0.1", SEND_PORT))?;
        sock.set_read_timeout(Some(Duration::from_millis(1)))?;
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let handle = std::thread::Builder::new().name("gt7-sim".into()).spawn(move || serve(sock, s))?;
        Ok(Simulator { stop, handle: Some(handle) })
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn serve(sock: UdpSocket, stop: Arc<AtomicBool>) {
    let mut car = Car::new();
    let mut peer: Option<(SocketAddr, Variant, Instant)> = None;
    let mut buf = [0u8; 16];
    let start = Instant::now();
    let mut ticks: u64 = 0;
    while !stop.load(Ordering::Relaxed) {
        while let Ok((n, mut src)) = sock.recv_from(&mut buf) {
            let variant = match &buf[..n] {
                b"~" => Variant::Tilde,
                b"B" => Variant::B,
                _ => Variant::A,
            };
            src.set_port(super::udp::RECV_PORT);
            peer = Some((src, variant, Instant::now()));
        }
        let Some((addr, variant, seen)) = peer else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        if seen.elapsed() > Duration::from_secs(5) {
            peer = None;
            continue;
        }
        let p = car.step();
        let enc = crypto::encrypt(&p.to_bytes(variant), car.rng.next().to_bits(), variant);
        let _ = sock.send_to(&enc, addr);
        ticks += 1;
        let due = start + Duration::from_secs_f64(ticks as f64 / 60.0);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::engine::{Engine, EngineEvent};

    #[test]
    fn simulated_race_records_laps_and_pit() {
        let mut car = Car::new();
        let mut engine = Engine::new();
        let mut laps = vec![];
        for _ in 0..60 * 60 * 14 {
            for e in engine.on_packet(&car.step()) {
                if let EngineEvent::LapCompleted(l, _) = e {
                    laps.push(*l);
                }
            }
            if laps.len() >= 7 {
                break;
            }
        }
        assert!(laps.len() >= 7, "only {} laps", laps.len());
        let len = laps[1].length_m;
        assert!(len > 3000.0 && len < 6000.0, "lap length {len}");
        assert!(laps.iter().any(|l| l.is_pit), "expected a pit stop");
        assert!(laps.iter().filter(|l| l.is_valid).count() >= 5);
    }

    /// Run a whole demo race through the engine and engineer and return what
    /// gets said, with the lap it was said on.
    fn transcript(level: crate::settings::ChatterLevel) -> Vec<(i16, String)> {
        use crate::engineer::director::EngineerConfig;
        use crate::engineer::rules::Engineer;
        let cfg = EngineerConfig { chatter: level, callouts: Default::default() };
        let mut car = Car::new();
        let mut engine = Engine::new();
        let mut eng = Engineer::new();
        let t0 = Instant::now();
        let mut said = vec![];
        let mut laps = 0;
        for tick in 0..60 * 60 * 25 {
            let now = t0 + Duration::from_secs_f64(tick as f64 / 60.0);
            let p = car.step();
            for e in engine.on_packet(&p) {
                if matches!(e, EngineEvent::LapCompleted(..)) {
                    laps += 1;
                }
                said.extend(eng.on_event(&e, &cfg, now).into_iter().map(|c| (p.lap, c.text)));
            }
            said.extend(eng.on_live(engine.live(), &cfg, now).into_iter().map(|c| (p.lap, c.text)));
            if laps >= RACE_LAPS {
                break;
            }
        }
        said
    }

    #[test]
    fn simulated_race_engineer_transcript() {
        use crate::settings::ChatterLevel;
        let crew = transcript(ChatterLevel::CrewChief);
        let normal = transcript(ChatterLevel::Normal);
        let minimal = transcript(ChatterLevel::Minimal);
        for (l, t) in &crew {
            println!("L{l}  {t}");
        }
        println!("crew chief {} / normal {} / minimal {} calls", crew.len(), normal.len(), minimal.len());
        let has = |v: &[(i16, String)], s: &str| v.iter().any(|(_, t)| t.contains(s));
        assert!(has(&crew, "Lights out"));
        assert!(has(&crew, "spin"), "spin detected");
        assert!(has(&crew, "Fuel usage is"));
        assert!(has(&crew, "Sector"));
        assert!(has(&crew, "Box"));
        assert!(!has(&crew, "won't count"), "no demo lap should be thrown out");
        assert!(crew.len() > normal.len() && normal.len() > minimal.len());
        assert!(!has(&minimal, "Sector"));
    }
}
