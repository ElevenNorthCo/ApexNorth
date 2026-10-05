//! Combines the fuel plan with the tyre estimate into one pit recommendation.

use serde::Serialize;

use super::fuel::{PitWindow, RaceStrategy};
use super::tyres::{TyreEstimate, TyrePerformance};

/// Tyre data has to be at least this sure before it moves the stop.
pub const TYRE_CONFIDENCE: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PitDriver {
    Fuel,
    Tyres,
    Both,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PitPlan {
    /// Recommended pit lap.
    pub lap: i16,
    pub driver: PitDriver,
    pub fuel_target: i16,
    pub fuel_latest: i16,
}

impl PitPlan {
    pub fn box_this_lap(&self, current_lap: i16) -> bool {
        self.lap <= current_lap
    }
}

pub fn plan(fuel: &RaceStrategy, tyres: &TyreEstimate, current_lap: i16, total_laps: i16) -> Option<PitPlan> {
    if total_laps > 0 && current_lap >= total_laps {
        // A tyre stop at the end of the final lap cannot recover any time.
        return None;
    }
    if fuel.stops_required == 0 {
        if fuel.window == PitWindow::BoxNow {
            return Some(PitPlan { lap: current_lap, driver: PitDriver::Fuel, fuel_target: current_lap, fuel_latest: current_lap });
        }
        // Fuel can finish. Only a strong, sustained pace-loss estimate should
        // suggest a tyre stop, and it is advice rather than a fuel emergency.
        return (total_laps > current_lap + 1 && tyres.confidence >= 0.75 && tyres.performance >= TyrePerformance::Poor)
            .then_some(PitPlan { lap: current_lap, driver: PitDriver::Tyres, fuel_target: total_laps, fuel_latest: total_laps });
    }
    let (earliest, target, latest) = (fuel.earliest_pit_lap?, fuel.target_pit_lap?, fuel.latest_pit_lap?);
    let mut p = PitPlan { lap: target, driver: PitDriver::Fuel, fuel_target: target, fuel_latest: latest };
    if fuel.window == PitWindow::BoxNow {
        p.lap = current_lap;
    }
    if tyres.confidence < TYRE_CONFIDENCE {
        return Some(p);
    }
    let tyre_lap = if tyres.performance >= TyrePerformance::Poor {
        Some(current_lap)
    } else {
        tyres.projected_problem_lap
    };
    if let Some(t) = tyre_lap {
        let lap = t.clamp(earliest, latest);
        if lap < p.lap {
            p.lap = lap;
            p.driver = PitDriver::Tyres;
        } else if t <= p.lap {
            p.driver = PitDriver::Both;
        }
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::fuel::FuelModel;

    fn fuel() -> RaceStrategy {
        let mut f = FuelModel::default();
        f.record_lap(10.0);
        f.strategy(85.0, 2, 15).unwrap() // window 6..9, target 8
    }

    fn tyres(perf: TyrePerformance, problem: Option<i16>, confidence: f32) -> TyreEstimate {
        TyreEstimate { performance: perf, projected_problem_lap: problem, confidence, ..Default::default() }
    }

    #[test]
    fn fuel_only_when_tyres_unsure() {
        let p = plan(&fuel(), &tyres(TyrePerformance::Declining, Some(6), 0.2), 2, 15).unwrap();
        assert_eq!((p.lap, p.driver), (8, PitDriver::Fuel));
    }

    #[test]
    fn tyres_bring_the_stop_forward() {
        let p = plan(&fuel(), &tyres(TyrePerformance::Declining, Some(7), 0.8), 2, 15).unwrap();
        assert_eq!((p.lap, p.driver, p.fuel_target), (7, PitDriver::Tyres, 8));
        // Never earlier than the fuel window allows.
        let q = plan(&fuel(), &tyres(TyrePerformance::Declining, Some(3), 0.8), 2, 15).unwrap();
        assert_eq!(q.lap, 6);
    }

    #[test]
    fn both_agree() {
        let p = plan(&fuel(), &tyres(TyrePerformance::Declining, Some(8), 0.8), 2, 15).unwrap();
        assert_eq!((p.lap, p.driver), (8, PitDriver::Both));
    }

    #[test]
    fn tyre_stop_without_fuel_stop_requires_strong_evidence_and_time_to_recover() {
        let mut f = fuel();
        f.stops_required = 0;
        f.window = PitWindow::NoStop;
        assert!(plan(&f, &tyres(TyrePerformance::Poor, None, 0.5), 2, 15).is_none());
        let t = tyres(TyrePerformance::Poor, None, 0.8);
        assert_eq!(plan(&f, &t, 2, 15).unwrap().driver, PitDriver::Tyres);
        assert!(plan(&f, &t, 14, 15).is_none());
        assert!(plan(&f, &t, 15, 15).is_none());
    }
}