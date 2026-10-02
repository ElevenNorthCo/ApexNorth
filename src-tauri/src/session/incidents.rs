//! Spin detection from the car's heading against its direction of travel,
//! backed up by yaw rate and speed loss. Wheelspin alone is never a spin.

use std::collections::VecDeque;

use crate::telemetry::packet::Packet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncidentKind {
    Spin,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Incident {
    pub kind: IncidentKind,
    pub speed_before_kph: f32,
    pub speed_loss_kph: f32,
    /// Big speed loss or nearly stopped.
    pub major: bool,
}

/// Slip angle (radians) that counts as a spin on its own: about 70°.
const SPIN_SLIP: f32 = 1.22;
/// A smaller slip angle counts when the car is also rotating fast.
const YAW_SLIP: f32 = 0.6;
const YAW_RATE: f32 = 2.0;
const MIN_SPEED: f32 = 8.0;

#[derive(Debug)]
pub struct SpinDetector {
    /// Learned sign of the car's forward axis (+z or -z), so the detector
    /// works whichever way the game's frame points.
    forward: f32,
    calibrated: u32,
    /// How often fast driving looks "straight ahead" in the car frame. If the
    /// rotation data doesn't make sense, this stays low and detection is off.
    frame_quality: f32,
    spin_ticks: u32,
    calm_ticks: u32,
    in_spin: bool,
    /// Recent speeds in km/h, one per tick, for the speed-loss check.
    speeds: VecDeque<f32>,
}

impl Default for SpinDetector {
    fn default() -> Self {
        SpinDetector { forward: 1.0, calibrated: 0, frame_quality: 0.0, spin_ticks: 0, calm_ticks: 0, in_spin: false, speeds: VecDeque::new() }
    }
}

impl SpinDetector {
    pub fn update(&mut self, p: &Packet, local_vel: [f32; 3], dt_ticks: u32) -> Option<Incident> {
        let kph = p.speed_kph();
        self.speeds.push_back(kph);
        while self.speeds.len() > 150 {
            self.speeds.pop_front();
        }
        if p.gear == 0 {
            // Reversing on purpose.
            self.spin_ticks = 0;
            return None;
        }
        let (lx, lz) = (local_vel[0], local_vel[2]);
        let planar = (lx * lx + lz * lz).sqrt();
        // Learn which way is forward while driving straight.
        if planar > 15.0 {
            let straight = lx.abs() < 0.3 * lz.abs();
            self.frame_quality += 0.005 * (if straight { 1.0 } else { 0.0 } - self.frame_quality);
            if straight {
                self.forward += 0.02 * (lz.signum() - self.forward);
                self.calibrated = self.calibrated.saturating_add(dt_ticks);
            }
        }
        if self.calibrated < 120 || self.frame_quality < 0.6 || self.forward.abs() < 0.5 || planar < MIN_SPEED {
            self.spin_ticks = 0;
            return None;
        }
        let slip = lx.abs().atan2(lz * self.forward.signum());
        let yaw = p.angular_velocity[1].abs();
        let spinning = slip > SPIN_SLIP || (slip > YAW_SLIP && yaw > YAW_RATE);
        if spinning {
            self.calm_ticks = 0;
            self.spin_ticks += dt_ticks;
            if self.spin_ticks >= 9 && !self.in_spin {
                self.in_spin = true;
                let before = self.speeds.iter().copied().fold(0.0, f32::max);
                let loss = (before - kph).max(0.0);
                return Some(Incident {
                    kind: IncidentKind::Spin,
                    speed_before_kph: before,
                    speed_loss_kph: loss,
                    // Measured early in the spin, so the full loss is larger
                    // still. Any spin at high speed is a big one.
                    major: loss > 30.0 || kph < 60.0 || before > 150.0,
                });
            }
        } else {
            self.spin_ticks = 0;
            if slip < 0.25 {
                self.calm_ticks += dt_ticks;
                if self.calm_ticks > 60 {
                    self.in_spin = false;
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(speed: f32) -> Packet {
        Packet { speed, gear: 3, ..Default::default() }
    }

    #[test]
    fn spin_fires_once_and_slide_does_not() {
        let mut d = SpinDetector::default();
        for _ in 0..600 {
            assert!(d.update(&pkt(40.0), [0.0, 0.0, 40.0], 1).is_none());
        }
        // A 30° drift is not a spin.
        for _ in 0..60 {
            assert!(d.update(&pkt(35.0), [17.0, 0.0, 30.0], 1).is_none());
        }
        // Travelling sideways then backwards: one spin.
        let mut hits = 0;
        for i in 0..60 {
            let a = 1.3 + i as f32 * 0.03;
            let v = 30.0 - i as f32 * 0.3;
            if d.update(&pkt(v), [v * a.sin(), 0.0, v * a.cos()], 1).is_some() {
                hits += 1;
            }
        }
        assert_eq!(hits, 1);
    }

    #[test]
    fn learns_reversed_forward_axis() {
        let mut d = SpinDetector::default();
        for _ in 0..600 {
            assert!(d.update(&pkt(40.0), [0.0, 0.0, -40.0], 1).is_none());
        }
        assert!(d.forward < -0.9);
    }
}
