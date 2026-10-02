use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PitWindow {
    /// Not a lap-limited race, or not enough data yet.
    None,
    /// Enough fuel to finish.
    NoStop,
    /// A stop is needed, but pitting now would not reach the finish.
    Closed,
    /// Pitting this lap would get you to the finish on a full tank.
    Open,
    /// Fuel will not last another lap.
    BoxNow,
}

/// Rolling fuel consumption model, in percent of tank.
#[derive(Debug, Default, Clone)]
pub struct FuelModel {
    history: VecDeque<f32>,
}

impl FuelModel {
    const WINDOW: usize = 3;

    pub fn record_lap(&mut self, used_pct: f32) {
        if used_pct <= 0.01 {
            return;
        }
        self.history.push_back(used_pct);
        while self.history.len() > Self::WINDOW {
            self.history.pop_front();
        }
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }

    pub fn per_lap(&self) -> Option<f32> {
        (!self.history.is_empty()).then(|| self.history.iter().sum::<f32>() / self.history.len() as f32)
    }

    pub fn laps_left(&self, fuel_pct: f32) -> Option<f32> {
        self.per_lap().map(|p| fuel_pct / p)
    }

    /// `laps_remaining` counts the current lap.
    pub fn pit_window(&self, fuel_pct: f32, laps_remaining: Option<i32>) -> PitWindow {
        let (Some(per_lap), Some(remaining)) = (self.per_lap(), laps_remaining) else {
            return PitWindow::None;
        };
        if remaining <= 0 {
            return PitWindow::None;
        }
        let laps_left = fuel_pct / per_lap;
        if laps_left >= remaining as f32 {
            return PitWindow::NoStop;
        }
        if laps_left < 1.15 {
            return PitWindow::BoxNow;
        }
        // Pitting at the end of this lap leaves (remaining - 1) laps on a full tank.
        let full_tank_laps = 100.0 / per_lap;
        if (remaining - 1) as f32 <= full_tank_laps {
            PitWindow::Open
        } else {
            PitWindow::Closed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_states() {
        let mut f = FuelModel::default();
        assert_eq!(f.pit_window(50.0, Some(10)), PitWindow::None);
        for _ in 0..3 {
            f.record_lap(10.0);
        }
        assert_eq!(f.per_lap(), Some(10.0));
        assert_eq!(f.pit_window(100.0, Some(8)), PitWindow::NoStop);
        assert_eq!(f.pit_window(60.0, Some(15)), PitWindow::Closed); // 14 laps after stop > 10
        assert_eq!(f.pit_window(60.0, Some(10)), PitWindow::Open); // 9 laps after stop
        assert_eq!(f.pit_window(10.0, Some(10)), PitWindow::BoxNow);
    }
}
