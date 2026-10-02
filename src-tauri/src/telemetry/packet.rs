//! Decoded GT7 telemetry packet. Offsets follow Nenkai/PDTools `SimulatorPacket`.

use super::crypto::{Variant, MAGIC};

#[allow(dead_code)] // Full flag set documented for future features.
pub mod flags {
    pub const CAR_ON_TRACK: u16 = 1 << 0;
    pub const PAUSED: u16 = 1 << 1;
    pub const LOADING: u16 = 1 << 2;
    pub const IN_GEAR: u16 = 1 << 3;
    pub const HAS_TURBO: u16 = 1 << 4;
    pub const REV_LIMIT: u16 = 1 << 5;
    pub const HANDBRAKE: u16 = 1 << 6;
    pub const LIGHTS: u16 = 1 << 7;
    pub const HIGH_BEAM: u16 = 1 << 8;
    pub const LOW_BEAM: u16 = 1 << 9;
    pub const ASM: u16 = 1 << 10;
    pub const TCS: u16 = 1 << 11;
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Packet {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    /// Quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub angular_velocity: [f32; 3],
    pub body_height: f32,
    pub rpm: f32,
    pub fuel: f32,
    pub fuel_capacity: f32,
    /// Metres per second.
    pub speed: f32,
    pub boost: f32,
    pub oil_pressure: f32,
    pub water_temp: f32,
    pub oil_temp: f32,
    /// FL, FR, RL, RR in °C.
    pub tyre_temp: [f32; 4],
    pub packet_id: i32,
    pub lap: i16,
    pub total_laps: i16,
    pub best_lap_ms: i32,
    pub last_lap_ms: i32,
    pub time_of_day_ms: i32,
    pub pre_race_position: i16,
    pub num_cars: i16,
    pub rpm_alert_min: i16,
    pub rpm_alert_max: i16,
    pub max_speed_kph: i16,
    pub flags: u16,
    pub gear: u8,
    pub suggested_gear: u8,
    pub throttle: u8,
    pub brake: u8,
    pub road_plane: [f32; 3],
    pub road_plane_distance: f32,
    /// Wheel angular speed, rad/s.
    pub wheel_rps: [f32; 4],
    pub tyre_radius: [f32; 4],
    pub suspension: [f32; 4],
    pub clutch: f32,
    pub clutch_engagement: f32,
    pub rpm_clutch: f32,
    pub top_speed_ratio: f32,
    pub gear_ratios: [f32; 8],
    pub car_code: i32,
    // --- "B" and later ---
    /// Steering wheel rotation, radians.
    pub wheel_rotation: Option<f32>,
    pub sway: Option<f32>,
    pub heave: Option<f32>,
    pub surge: Option<f32>,
    // --- "~" ---
    pub raw_throttle: Option<u8>,
    pub raw_brake: Option<u8>,
    pub energy_recovery: Option<f32>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn i32(&self, o: usize) -> i32 {
        i32::from_le_bytes(self.0[o..o + 4].try_into().unwrap())
    }
    fn i16(&self, o: usize) -> i16 {
        i16::from_le_bytes(self.0[o..o + 2].try_into().unwrap())
    }
    fn u16(&self, o: usize) -> u16 {
        u16::from_le_bytes(self.0[o..o + 2].try_into().unwrap())
    }
    fn arr<const N: usize>(&self, o: usize) -> [f32; N] {
        std::array::from_fn(|i| self.f32(o + i * 4))
    }
}

impl Packet {
    /// Parse a decrypted packet.
    pub fn parse(buf: &[u8]) -> Option<Packet> {
        if buf.len() < 0x128 {
            return None;
        }
        let r = Reader(buf);
        if r.i32(0) as u32 != MAGIC {
            return None;
        }
        let gear_byte = buf[0x90];
        let has_b = buf.len() >= 0x13C;
        let has_tilde = buf.len() >= 0x158;
        Some(Packet {
            position: r.arr(0x04),
            velocity: r.arr(0x10),
            rotation: r.arr(0x1C),
            angular_velocity: r.arr(0x2C),
            body_height: r.f32(0x38),
            rpm: r.f32(0x3C),
            fuel: r.f32(0x44),
            fuel_capacity: r.f32(0x48),
            speed: r.f32(0x4C),
            boost: r.f32(0x50),
            oil_pressure: r.f32(0x54),
            water_temp: r.f32(0x58),
            oil_temp: r.f32(0x5C),
            tyre_temp: r.arr(0x60),
            packet_id: r.i32(0x70),
            lap: r.i16(0x74),
            total_laps: r.i16(0x76),
            best_lap_ms: r.i32(0x78),
            last_lap_ms: r.i32(0x7C),
            time_of_day_ms: r.i32(0x80),
            pre_race_position: r.i16(0x84),
            num_cars: r.i16(0x86),
            rpm_alert_min: r.i16(0x88),
            rpm_alert_max: r.i16(0x8A),
            max_speed_kph: r.i16(0x8C),
            flags: r.u16(0x8E),
            gear: gear_byte & 0x0F,
            suggested_gear: gear_byte >> 4,
            throttle: buf[0x91],
            brake: buf[0x92],
            road_plane: r.arr(0x94),
            road_plane_distance: r.f32(0xA0),
            wheel_rps: r.arr(0xA4),
            tyre_radius: r.arr(0xB4),
            suspension: r.arr(0xC4),
            clutch: r.f32(0xF4),
            clutch_engagement: r.f32(0xF8),
            rpm_clutch: r.f32(0xFC),
            top_speed_ratio: r.f32(0x100),
            gear_ratios: r.arr(0x104),
            car_code: r.i32(0x124),
            wheel_rotation: has_b.then(|| r.f32(0x128)),
            sway: has_b.then(|| r.f32(0x130)),
            heave: has_b.then(|| r.f32(0x134)),
            surge: has_b.then(|| r.f32(0x138)),
            raw_throttle: has_tilde.then(|| buf[0x13C]),
            raw_brake: has_tilde.then(|| buf[0x13D]),
            energy_recovery: has_tilde.then(|| r.f32(0x150)),
        })
    }

    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }

    pub fn speed_kph(&self) -> f32 {
        self.speed * 3.6
    }

    /// Serialize back to a plaintext packet. Used by the demo simulator.
    pub fn to_bytes(&self, variant: Variant) -> Vec<u8> {
        let mut b = vec![0u8; variant.size()];
        let mut put = |o: usize, bytes: &[u8]| b[o..o + bytes.len()].copy_from_slice(bytes);
        put(0, &MAGIC.to_le_bytes());
        for (i, v) in self.position.iter().enumerate() {
            put(0x04 + i * 4, &v.to_le_bytes());
        }
        for (i, v) in self.velocity.iter().enumerate() {
            put(0x10 + i * 4, &v.to_le_bytes());
        }
        for (i, v) in self.rotation.iter().enumerate() {
            put(0x1C + i * 4, &v.to_le_bytes());
        }
        for (i, v) in self.angular_velocity.iter().enumerate() {
            put(0x2C + i * 4, &v.to_le_bytes());
        }
        put(0x3C, &self.rpm.to_le_bytes());
        put(0x44, &self.fuel.to_le_bytes());
        put(0x48, &self.fuel_capacity.to_le_bytes());
        put(0x4C, &self.speed.to_le_bytes());
        put(0x58, &self.water_temp.to_le_bytes());
        put(0x5C, &self.oil_temp.to_le_bytes());
        for (i, v) in self.tyre_temp.iter().enumerate() {
            put(0x60 + i * 4, &v.to_le_bytes());
        }
        put(0x70, &self.packet_id.to_le_bytes());
        put(0x74, &self.lap.to_le_bytes());
        put(0x76, &self.total_laps.to_le_bytes());
        put(0x78, &self.best_lap_ms.to_le_bytes());
        put(0x7C, &self.last_lap_ms.to_le_bytes());
        put(0x80, &self.time_of_day_ms.to_le_bytes());
        put(0x84, &self.pre_race_position.to_le_bytes());
        put(0x86, &self.num_cars.to_le_bytes());
        put(0x88, &self.rpm_alert_min.to_le_bytes());
        put(0x8A, &self.rpm_alert_max.to_le_bytes());
        put(0x8C, &self.max_speed_kph.to_le_bytes());
        put(0x8E, &self.flags.to_le_bytes());
        put(0x90, &[(self.gear & 0x0F) | (self.suggested_gear << 4)]);
        put(0x91, &[self.throttle]);
        put(0x92, &[self.brake]);
        for (i, v) in self.wheel_rps.iter().enumerate() {
            put(0xA4 + i * 4, &v.to_le_bytes());
        }
        for (i, v) in self.tyre_radius.iter().enumerate() {
            put(0xB4 + i * 4, &v.to_le_bytes());
        }
        put(0x124, &self.car_code.to_le_bytes());
        if variant != Variant::A {
            put(0x128, &self.wheel_rotation.unwrap_or(0.0).to_le_bytes());
        }
        if variant == Variant::Tilde {
            put(0x13C, &[self.raw_throttle.unwrap_or(self.throttle)]);
            put(0x13D, &[self.raw_brake.unwrap_or(self.brake)]);
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::crypto;

    #[test]
    fn encrypt_parse_round_trip() {
        let p = Packet {
            position: [10.0, 2.0, -30.0],
            rpm: 6500.0,
            fuel: 40.0,
            fuel_capacity: 100.0,
            speed: 50.0,
            tyre_temp: [70.0, 71.0, 72.0, 73.0],
            packet_id: 999,
            lap: 3,
            total_laps: 10,
            best_lap_ms: 94_700,
            last_lap_ms: 95_100,
            pre_race_position: 4,
            rpm_alert_min: 7000,
            rpm_alert_max: 7500,
            flags: flags::CAR_ON_TRACK | flags::IN_GEAR,
            gear: 4,
            suggested_gear: 15,
            throttle: 255,
            brake: 0,
            car_code: 3442,
            wheel_rotation: Some(0.25),
            ..Default::default()
        };
        for v in Variant::ALL {
            let enc = crypto::encrypt(&p.to_bytes(v), 42, v);
            let (dec, _) = crypto::decrypt(&enc).unwrap();
            let q = Packet::parse(&dec).unwrap();
            assert_eq!(q.lap, 3);
            assert_eq!(q.gear, 4);
            assert_eq!(q.suggested_gear, 15);
            assert_eq!(q.car_code, 3442);
            assert_eq!(q.tyre_temp, p.tyre_temp);
            assert_eq!(q.best_lap_ms, 94_700);
            assert!((q.speed_kph() - 180.0).abs() < 0.01);
            assert!(q.has(flags::CAR_ON_TRACK));
            assert_eq!(q.wheel_rotation.is_some(), v != Variant::A);
        }
    }
}
