//! Estimated tyre performance. GT7 sends no wear data, so this reads the
//! stint's lap-time trend (corrected for the car getting lighter as fuel
//! burns) and reports how fast pace is fading, with a confidence.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TyrePerformance {
    Fresh,
    Stable,
    Declining,
    Poor,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TyreEstimate {
    pub performance: TyrePerformance,
    pub estimated_loss_per_lap_ms: Option<i32>,
    /// Lap where the fade is expected to cost about two seconds.
    pub projected_problem_lap: Option<i16>,
    /// 0 to 1.
    pub confidence: f32,
    pub stint_laps: u32,
}

impl Default for TyreEstimate {
    fn default() -> Self {
        TyreEstimate {
            performance: TyrePerformance::Fresh,
            estimated_loss_per_lap_ms: None,
            projected_problem_lap: None,
            confidence: 0.0,
            stint_laps: 0,
        }
    }
}

/// Lap time gained per percent of tank burned, as a fraction of lap time.
const FUEL_GAIN_PER_PCT: f32 = 0.000_25;
/// Pace loss treated as "significant" for the projection.
const PROBLEM_MS: f32 = 2000.0;
/// Laps slower than the stint median by more than this are mistakes or traffic.
const OUTLIER_MS: f32 = 1500.0;

#[derive(Debug, Default, Clone)]
pub struct TyreModel {
    /// (lap number, fuel-corrected lap time) for clean laps this stint.
    laps: Vec<(i16, f32)>,
    stint_laps: u32,
    fuel_burned: f32,
}

impl TyreModel {
    pub fn reset(&mut self) {
        *self = TyreModel::default();
    }

    pub fn record_lap(&mut self, lap_no: i16, time_ms: u32, valid: bool, is_pit: bool, fuel_used_pct: Option<f32>) {
        if is_pit {
            // New tyres (or at least a new stint) after a stop.
            self.reset();
            return;
        }
        self.stint_laps += 1;
        self.fuel_burned += fuel_used_pct.unwrap_or(0.0).max(0.0);
        // Skip the first lap of a stint: standing start or out lap.
        if valid && self.stint_laps >= 2 {
            let t = time_ms as f32;
            self.laps.push((lap_no, t * (1.0 + FUEL_GAIN_PER_PCT * self.fuel_burned)));
        }
    }

    pub fn estimate(&self) -> TyreEstimate {
        let mut est = TyreEstimate { stint_laps: self.stint_laps, ..Default::default() };
        if self.stint_laps <= 2 {
            return est;
        }
        est.performance = TyrePerformance::Stable;
        let mut times: Vec<f32> = self.laps.iter().map(|l| l.1).collect();
        times.sort_by(|a, b| a.total_cmp(b));
        let Some(median) = times.get(times.len() / 2).copied() else { return est };
        let pts: Vec<(f32, f32)> =
            self.laps.iter().filter(|l| l.1 - median < OUTLIER_MS).map(|l| (l.0 as f32, l.1)).collect();
        let n = pts.len();
        if n < 3 {
            return est;
        }
        let mx = pts.iter().map(|p| p.0).sum::<f32>() / n as f32;
        let my = pts.iter().map(|p| p.1).sum::<f32>() / n as f32;
        let sxx: f32 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
        if sxx <= 0.0 {
            return est;
        }
        let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f32>() / sxx;
        let resid = (pts.iter().map(|p| (p.1 - (my + slope * (p.0 - mx))).powi(2)).sum::<f32>() / n as f32).sqrt();
        let conf_n = ((n - 2) as f32 / 3.0).min(1.0);
        let conf_fit = 1.0 / (1.0 + resid / 400.0);
        est.confidence = (conf_n * conf_fit).clamp(0.0, 1.0);

        let first = pts[0].0;
        let last = pts[n - 1].0;
        let loss = (slope * (last - first)).max(0.0);
        est.estimated_loss_per_lap_ms = Some(slope.round() as i32);
        if est.confidence >= 0.3 {
            est.performance = if loss >= 3000.0 || slope >= 600.0 {
                TyrePerformance::Critical
            } else if loss >= 1500.0 || slope >= 300.0 {
                TyrePerformance::Poor
            } else if loss >= 500.0 || slope >= 120.0 {
                TyrePerformance::Declining
            } else {
                TyrePerformance::Stable
            };
        }
        if slope > 50.0 {
            let laps_to_problem = ((PROBLEM_MS - loss) / slope).ceil().max(1.0);
            est.projected_problem_lap = Some(last as i16 + laps_to_problem as i16);
        }
        est
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_pace_is_stable() {
        let mut m = TyreModel::default();
        for (i, t) in [92_000, 90_100, 90_050, 90_200, 89_950, 90_100].iter().enumerate() {
            m.record_lap(i as i16 + 1, *t, true, false, Some(0.0));
        }
        let e = m.estimate();
        assert_eq!(e.performance, TyrePerformance::Stable);
        assert!(e.estimated_loss_per_lap_ms.unwrap().abs() < 80);
    }

    #[test]
    fn fading_pace_is_detected_and_projected() {
        let mut m = TyreModel::default();
        // Out lap, then 400 ms a lap slower.
        m.record_lap(1, 95_000, true, false, Some(5.0));
        for i in 0..5 {
            m.record_lap(i + 2, 90_000 + i as u32 * 400, true, false, Some(5.0));
        }
        let e = m.estimate();
        assert!(e.performance >= TyrePerformance::Declining, "{e:?}");
        let loss = e.estimated_loss_per_lap_ms.unwrap();
        // 400 ms raw, plus the ~112 ms a lap the lighter car should have gained.
        assert!(loss > 480 && loss < 560, "fuel-corrected loss {loss}");
        assert!(e.confidence > 0.5);
        assert!(e.projected_problem_lap.unwrap() >= 7);
    }

    #[test]
    fn mistakes_are_ignored_and_pit_resets() {
        let mut m = TyreModel::default();
        m.record_lap(1, 95_000, true, false, None);
        for (i, t) in [90_000, 90_050, 96_000, 90_000, 90_100].iter().enumerate() {
            m.record_lap(i as i16 + 2, *t, true, false, None);
        }
        assert_eq!(m.estimate().performance, TyrePerformance::Stable);
        m.record_lap(7, 110_000, false, true, None);
        assert_eq!(m.estimate().performance, TyrePerformance::Fresh);
    }
}
