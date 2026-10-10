//! Estimated tyre performance. GT7 sends no wear data, so this reads the
//! stint's lap-time trend (corrected for the car getting lighter as fuel
//! burns) and reports how fast pace is fading, with a confidence.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TyrePerformance {
    Unknown,
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
    /// The rate comes from past sessions at this tyre wear, not this stint yet.
    pub from_history: bool,
}

impl Default for TyreEstimate {
    fn default() -> Self {
        TyreEstimate {
            performance: TyrePerformance::Unknown,
            estimated_loss_per_lap_ms: None,
            projected_problem_lap: None,
            confidence: 0.0,
            stint_laps: 0,
            from_history: false,
        }
    }
}

/// Lap time gained per percent of tank burned, as a fraction of lap time.
const FUEL_GAIN_PER_PCT: f32 = 0.000_25;
/// Pace loss treated as "significant" for the projection.
const PROBLEM_MS: f32 = 2000.0;
/// Laps slower than the stint median by more than this are mistakes or traffic.
const OUTLIER_MS: f32 = 1500.0;
/// Confidence given to a fade rate learned from past sessions: enough to
/// shape the pit plan, not enough on its own to call a tyre-only stop.
const HISTORY_CONFIDENCE: f32 = 0.5;

#[derive(Debug, Default, Clone)]
pub struct TyreModel {
    /// (lap number, fuel-corrected lap time) for clean laps this stint.
    laps: Vec<(i16, f32)>,
    stint_laps: u32,
    last_lap: i16,
    fuel_burned: f32,
    /// Tyre wear multiplier noted for the session (0 = wear off).
    wear: Option<u8>,
    /// Expected fade (ms per lap) at that multiplier, from past sessions.
    prior_ms: Option<f32>,
}

impl TyreModel {
    /// New stint. The session's wear setting carries over.
    pub fn reset(&mut self) {
        *self = TyreModel { wear: self.wear, prior_ms: self.prior_ms, ..Default::default() };
    }

    /// New session: forget the stint and the wear setting.
    pub fn clear(&mut self) {
        *self = TyreModel::default();
    }

    /// The noted tyre wear multiplier, and the fade it caused before
    /// (ms per lap at this multiplier), when there is history for it.
    pub fn set_wear(&mut self, wear: Option<u8>, prior_ms: Option<f32>) {
        self.wear = wear;
        self.prior_ms = prior_ms.filter(|_| wear.is_some_and(|w| w > 0));
    }

    pub fn record_lap(&mut self, lap_no: i16, time_ms: u32, valid: bool, is_pit: bool, fuel_used_pct: Option<f32>) {
        if is_pit {
            // The packet cannot confirm a tyre change. Rebaseline after the
            // interruption, but report Unknown rather than claiming new tyres.
            self.reset();
            return;
        }
        self.stint_laps += 1;
        self.last_lap = lap_no;
        self.fuel_burned += fuel_used_pct.unwrap_or(0.0).max(0.0);
        // Skip the first lap of a stint: standing start or out lap.
        if valid && self.stint_laps >= 2 {
            let t = time_ms as f32;
            self.laps.push((lap_no, t * (1.0 + FUEL_GAIN_PER_PCT * self.fuel_burned)));
        }
    }

    pub fn estimate(&self) -> TyreEstimate {
        let mut est = TyreEstimate { stint_laps: self.stint_laps, ..Default::default() };
        if self.stint_laps <= 2 || self.wear == Some(0) {
            // Wear off: any pace fade is not the tyres.
            return est;
        }
        let measured = self.fit();
        let Some((slope, conf, first, last)) = measured.or_else(|| {
            self.prior_ms.map(|p| (p, 0.0, self.last_lap - (self.stint_laps as i16 - 2), self.last_lap))
        }) else {
            return est;
        };
        // History fills in until this stint's own trend is sure of itself.
        let (slope, conf) = match self.prior_ms {
            Some(p) if conf < HISTORY_CONFIDENCE => {
                est.from_history = true;
                (conf * slope + (1.0 - conf) * p, HISTORY_CONFIDENCE)
            }
            _ => (slope, conf),
        };
        est.confidence = conf;
        let loss = (slope * (last - first) as f32).max(0.0);
        est.estimated_loss_per_lap_ms = Some(slope.round() as i32);
        if measured.is_some() {
            est.performance = TyrePerformance::Stable;
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
        }
        if slope > 50.0 {
            let laps_to_problem = ((PROBLEM_MS - loss) / slope).ceil().max(1.0);
            est.projected_problem_lap = Some(last + laps_to_problem as i16);
        }
        est
    }

    /// Fade fitted to this stint's clean laps: (ms per lap, confidence,
    /// first lap, last lap), once there are three of them.
    fn fit(&self) -> Option<(f32, f32, i16, i16)> {
        let mut times: Vec<f32> = self.laps.iter().map(|l| l.1).collect();
        times.sort_by(|a, b| a.total_cmp(b));
        let median = times.get(times.len() / 2).copied()?;
        let pts: Vec<(f32, f32)> =
            self.laps.iter().filter(|l| l.1 - median < OUTLIER_MS).map(|l| (l.0 as f32, l.1)).collect();
        let n = pts.len();
        if n < 3 {
            return None;
        }
        let mx = pts.iter().map(|p| p.0).sum::<f32>() / n as f32;
        let my = pts.iter().map(|p| p.1).sum::<f32>() / n as f32;
        let sxx: f32 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
        if sxx <= 0.0 {
            return None;
        }
        let slope = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum::<f32>() / sxx;
        let resid = (pts.iter().map(|p| (p.1 - (my + slope * (p.0 - mx))).powi(2)).sum::<f32>() / n as f32).sqrt();
        let conf_n = ((n - 2) as f32 / 3.0).min(1.0);
        let conf_fit = 1.0 / (1.0 + resid / 400.0);
        Some((slope, (conf_n * conf_fit).clamp(0.0, 1.0), pts[0].0 as i16, pts[n - 1].0 as i16))
    }
}

/// Fade rate (ms per lap, fuel-corrected) per unit of tyre wear multiplier
/// in a past session, from its laps in order: (lap number, time, valid, pit,
/// fuel used). None when the session's own trend isn't sure enough.
pub fn fade_per_wear(laps: &[(i16, u32, bool, bool, Option<f32>)], wear: u8) -> Option<f32> {
    if wear == 0 {
        return None;
    }
    // The longest stint gives the clearest trend.
    let mut best: Option<(u32, f32)> = None;
    let mut m = TyreModel::default();
    let mut take = |m: &TyreModel| {
        if let Some((slope, ..)) = m.fit().filter(|f| f.1 >= HISTORY_CONFIDENCE) {
            if best.map_or(true, |b| m.stint_laps > b.0) {
                best = Some((m.stint_laps, slope.max(0.0) / wear as f32));
            }
        }
    };
    for &(lap_no, time_ms, valid, is_pit, fuel) in laps {
        if is_pit {
            take(&m);
        }
        m.record_lap(lap_no, time_ms, valid, is_pit, fuel);
    }
    take(&m);
    best.map(|b| b.1)
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
        assert_eq!(m.estimate().performance, TyrePerformance::Unknown);
    }

    #[test]
    fn wear_off_never_blames_the_tyres() {
        let mut m = TyreModel::default();
        m.set_wear(Some(0), None);
        m.record_lap(1, 95_000, true, false, None);
        for i in 0..6 {
            m.record_lap(i + 2, 90_000 + i as u32 * 500, true, false, None);
        }
        let e = m.estimate();
        assert_eq!((e.performance, e.projected_problem_lap, e.confidence), (TyrePerformance::Unknown, None, 0.0));
    }

    #[test]
    fn history_projects_before_the_stint_has_a_trend_and_survives_a_pit() {
        let mut m = TyreModel::default();
        m.set_wear(Some(3), Some(400.0));
        m.record_lap(1, 95_000, true, false, None);
        m.record_lap(2, 90_000, true, false, None);
        m.record_lap(3, 90_300, true, false, None);
        let e = m.estimate();
        assert!(e.from_history && e.confidence >= 0.5, "{e:?}");
        // Not claimed as measured fade yet.
        assert_eq!(e.performance, TyrePerformance::Unknown);
        assert_eq!(e.estimated_loss_per_lap_ms, Some(400));
        // 400 ms over one lap so far, 2 s reached four laps later.
        assert_eq!(e.projected_problem_lap, Some(7));
        m.record_lap(4, 110_000, false, true, None);
        for (i, t) in [95_000, 90_000, 90_400].iter().enumerate() {
            m.record_lap(i as i16 + 5, *t, true, false, None);
        }
        assert!(m.estimate().from_history);
    }

    #[test]
    fn a_sure_stint_trend_outweighs_history() {
        let mut m = TyreModel::default();
        m.set_wear(Some(3), Some(400.0));
        m.record_lap(1, 95_000, true, false, Some(0.0));
        for (i, t) in [90_100, 90_050, 90_200, 89_950, 90_100, 90_000].iter().enumerate() {
            m.record_lap(i as i16 + 2, *t, true, false, Some(0.0));
        }
        let e = m.estimate();
        assert!(!e.from_history);
        assert_eq!(e.performance, TyrePerformance::Stable);
    }

    #[test]
    fn fade_per_wear_reads_the_longest_stint() {
        let mut laps = vec![(1, 95_000, true, false, Some(0.0))];
        for i in 0..6 {
            laps.push((i + 2, 90_000 + i as u32 * 600, true, false, Some(0.0)));
        }
        laps.push((8, 110_000, false, true, Some(0.0)));
        laps.push((9, 95_000, true, false, Some(0.0)));
        let f = fade_per_wear(&laps, 3).unwrap();
        assert!((f - 200.0).abs() < 5.0, "{f}");
        assert_eq!(fade_per_wear(&laps, 0), None);
    }
}
