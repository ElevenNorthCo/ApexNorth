use serde::{Deserialize, Serialize};

/// One recorded telemetry sample inside a lap (~30 Hz).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    /// Milliseconds since lap start.
    pub t: u32,
    /// Metres since lap start.
    pub d: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// km/h
    pub spd: f32,
    /// 0-100
    pub thr: u8,
    /// 0-100
    pub brk: u8,
    pub gear: u8,
    pub rpm: f32,
    /// Steering wheel angle, radians (0 when the packet variant lacks it).
    pub steer: f32,
    /// Tyre temps FL, FR, RL, RR.
    pub tt: [f32; 4],
    pub glat: f32,
    pub glon: f32,
}

pub const SECTOR_COUNT: usize = 5;

/// Why a lap doesn't count. Only set on strong evidence, so a single bad
/// packet can't throw away a real lap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidReason {
    /// Timing started part-way through the lap.
    JoinedMidLap,
    /// The car was moved back or reset (rewind, restart, return to track).
    RewindOrReset,
    /// A confirmed, impossible position jump with no pause around it.
    TelemetryJump,
    /// The lap was much shorter than the track.
    InsufficientDistance,
    PitLap,
}

impl InvalidReason {
    pub fn as_str(self) -> &'static str {
        match self {
            InvalidReason::JoinedMidLap => "joined_mid_lap",
            InvalidReason::RewindOrReset => "rewind_or_reset",
            InvalidReason::TelemetryJump => "telemetry_jump",
            InvalidReason::InsufficientDistance => "insufficient_distance",
            InvalidReason::PitLap => "pit_lap",
        }
    }
}

/// A finished lap produced by the engine.
#[derive(Debug, Clone)]
pub struct CompletedLap {
    pub lap_no: i16,
    pub time_ms: u32,
    pub is_pit: bool,
    pub is_valid: bool,
    pub invalid_reason: Option<InvalidReason>,
    pub sectors_ms: Vec<u32>,
    pub fuel_used_pct: Option<f32>,
    pub max_speed: f32,
    pub length_m: f32,
    /// Position where the lap began (start/finish line).
    pub start_pos: [f32; 3],
    pub samples: Vec<Sample>,
}

/// Interpolate elapsed time at distance `d` in a lap's samples.
pub fn time_at_distance(samples: &[Sample], d: f32) -> Option<f32> {
    let last = samples.last()?;
    if d <= 0.0 {
        return Some(0.0);
    }
    if d >= last.d {
        return None;
    }
    let i = samples.partition_point(|s| s.d < d);
    if i == 0 {
        return Some(samples[0].t as f32);
    }
    let (a, b) = (&samples[i - 1], &samples[i]);
    let span = (b.d - a.d).max(1e-3);
    let f = (d - a.d) / span;
    Some(a.t as f32 + f * (b.t as f32 - a.t as f32))
}

/// Split a lap into equal-distance sectors. The final sector absorbs any
/// difference to the official lap time.
pub fn sector_times(samples: &[Sample], lap_ms: u32) -> Vec<u32> {
    let Some(last) = samples.last() else { return vec![] };
    let len = last.d;
    if len < 100.0 {
        return vec![];
    }
    let mut out = Vec::with_capacity(SECTOR_COUNT);
    let mut prev = 0u32;
    for k in 1..SECTOR_COUNT {
        let t = time_at_distance(samples, len * k as f32 / SECTOR_COUNT as f32)
            .map_or(prev, |t| (t.round() as u32).max(prev));
        out.push(t - prev);
        prev = t;
    }
    out.push(lap_ms.saturating_sub(prev));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(n: u32, v: f32) -> Vec<Sample> {
        (0..=n)
            .map(|i| Sample { t: i * 100, d: i as f32 * v, ..Default::default() })
            .collect()
    }

    #[test]
    fn interpolates_time() {
        let s = lap(100, 5.0); // 500 m in 10 s
        assert_eq!(time_at_distance(&s, 250.0), Some(5000.0));
        assert_eq!(time_at_distance(&s, 252.5), Some(5050.0));
        assert_eq!(time_at_distance(&s, 600.0), None);
    }

    #[test]
    fn sectors_sum_to_lap_time() {
        let s = lap(1000, 2.0);
        let secs = sector_times(&s, 100_050);
        assert_eq!(secs.len(), SECTOR_COUNT);
        assert_eq!(secs.iter().sum::<u32>(), 100_050);
        assert_eq!(secs[0], 20_000);
    }
}
