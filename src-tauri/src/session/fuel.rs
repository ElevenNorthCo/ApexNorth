//! Fuel model and pit-window strategy, in percent of tank.

use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PitWindow {
    /// Not a lap-limited race, or not enough data yet.
    None,
    /// Enough fuel to finish.
    NoStop,
    /// A stop is needed, but pitting this lap is too early.
    Closed,
    /// Pitting this lap works: between the earliest and latest pit lap.
    Open,
    /// This lap is the last one the fuel allows.
    BoxNow,
}

/// Laps of fuel held back as a safety margin.
pub const RESERVE_LAPS: f32 = 0.3;

/// Fuel and pit plan for the rest of the race. Lap numbers are race laps;
/// the "pit lap" is the lap you dive into the pits at the end of.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RaceStrategy {
    pub fuel_per_lap_pct: f32,
    pub fuel_laps_remaining: f32,
    /// Fuel needed to finish from here (lap-limited races only).
    pub fuel_required_to_finish_pct: Option<f32>,
    pub stops_required: u8,
    pub earliest_pit_lap: Option<i16>,
    pub target_pit_lap: Option<i16>,
    pub latest_pit_lap: Option<i16>,
    /// Fuel left at the flag if you don't stop (no-stop races only).
    pub projected_finish_fuel_pct: Option<f32>,
    /// Spare laps of fuel at the flag (no-stop races only).
    pub fuel_margin_laps: Option<f32>,
    pub window: PitWindow,
}

/// Whether the game is burning fuel at all. Time trials and some modes don't.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FuelState {
    #[default]
    Unknown,
    Active,
    Inactive,
}

/// Rolling fuel consumption model.
#[derive(Debug, Default, Clone)]
pub struct FuelModel {
    history: VecDeque<f32>,
    state: FuelState,
    /// Fuel level the "is it moving?" watch started from, and driving ticks since.
    watch: Option<(f32, u32)>,
    flat_laps: u32,
    /// The driver noted fuel use as off (×0) for this session.
    noted_off: bool,
}

impl FuelModel {
    const WINDOW: usize = 3;
    /// Less than this per lap counts as "no fuel use".
    const FLAT_PCT: f32 = 0.05;
    /// Driving this long (60 Hz ticks) with no fuel change means fuel is off.
    const FLAT_TICKS: u32 = 60 * 45;

    pub fn reset(&mut self) {
        *self = FuelModel::default();
    }

    pub fn state(&self) -> FuelState {
        if self.noted_off {
            FuelState::Inactive
        } else {
            self.state
        }
    }

    /// Fuel use noted as off for this session: no fuel plan, straight away.
    pub fn set_noted_off(&mut self, off: bool) {
        self.noted_off = off;
    }

    pub fn active(&self) -> bool {
        self.state() == FuelState::Active
    }

    /// Watch the live fuel level while driving so fuel-off modes are spotted
    /// within a lap. `fuel_pct` is None when the game sends no tank size.
    pub fn observe(&mut self, fuel_pct: Option<f32>, moving: bool, dt_ticks: u32) {
        let Some(f) = fuel_pct else {
            self.state = FuelState::Inactive;
            return;
        };
        let (start, ticks) = self.watch.get_or_insert((f, 0));
        if f > *start + 0.5 {
            // Refuelled: restart the watch.
            *start = f;
            *ticks = 0;
        } else if *start - f > 0.2 {
            if self.state == FuelState::Inactive {
                self.history.clear();
            }
            self.state = FuelState::Active;
            self.flat_laps = 0;
            *start = f;
            *ticks = 0;
        } else if moving {
            *ticks += dt_ticks;
            if *ticks >= Self::FLAT_TICKS {
                self.state = FuelState::Inactive;
            }
        }
    }

    pub fn record_lap(&mut self, used_pct: f32) {
        if used_pct < Self::FLAT_PCT {
            self.flat_laps += 1;
            if self.flat_laps >= 2 {
                self.state = FuelState::Inactive;
            }
            return;
        }
        self.flat_laps = 0;
        self.state = FuelState::Active;
        self.history.push_back(used_pct);
        while self.history.len() > Self::WINDOW {
            self.history.pop_front();
        }
    }

    /// Rolling average, but a lap that differs a lot from the ones before it
    /// gets half the weight straight away, so real changes show up quickly.
    pub fn per_lap(&self) -> Option<f32> {
        if !self.active() || self.history.is_empty() {
            return None;
        }
        let n = self.history.len();
        let last = *self.history.back()?;
        if n >= 2 {
            let base = self.history.iter().take(n - 1).sum::<f32>() / (n - 1) as f32;
            if (last - base).abs() / base > 0.12 {
                return Some((base + last) / 2.0);
            }
        }
        Some(self.history.iter().sum::<f32>() / n as f32)
    }

    pub fn laps_left(&self, fuel_pct: f32) -> Option<f32> {
        self.per_lap().map(|p| fuel_pct / p)
    }

    /// Plan the rest of the race. `current_lap` is the lap being driven
    /// (1-based); `total_laps` is 0 for races that aren't lap-limited.
    pub fn strategy(&self, fuel_pct: f32, current_lap: i16, total_laps: i16) -> Option<RaceStrategy> {
        let per_lap = self.per_lap()?;
        Some(plan(per_lap, fuel_pct, current_lap, total_laps))
    }
}

fn plan(per_lap: f32, fuel_pct: f32, current: i16, total: i16) -> RaceStrategy {
    let laps_of_fuel = fuel_pct / per_lap;
    let usable = laps_of_fuel - RESERVE_LAPS;
    let mut s = RaceStrategy {
        fuel_per_lap_pct: per_lap,
        fuel_laps_remaining: laps_of_fuel,
        fuel_required_to_finish_pct: None,
        stops_required: 0,
        earliest_pit_lap: None,
        target_pit_lap: None,
        latest_pit_lap: None,
        projected_finish_fuel_pct: None,
        fuel_margin_laps: None,
        window: PitWindow::None,
    };
    // Laps you can still complete, counting the current one.
    let can_complete = usable.max(0.0).floor() as i16;
    let latest = (current - 1 + can_complete).max(current);

    if total <= 0 || current > total {
        if laps_of_fuel < 1.15 {
            s.window = PitWindow::BoxNow;
            s.latest_pit_lap = Some(current);
            s.target_pit_lap = Some(current);
        }
        return s;
    }

    let remaining = (total - current + 1) as f32;
    s.fuel_required_to_finish_pct = Some(remaining * per_lap);
    if usable >= remaining {
        s.window = PitWindow::NoStop;
        s.projected_finish_fuel_pct = Some(fuel_pct - remaining * per_lap);
        s.fuel_margin_laps = Some(laps_of_fuel - remaining);
        return s;
    }

    let tank_laps = (100.0 / per_lap - RESERVE_LAPS).max(1.0).floor() as i32;
    let deficit = remaining - usable;
    let mut stops = (deficit / tank_laps as f32).ceil().max(1.0) as i32;
    // Pitting at the end of lap L leaves (total - L) laps for `stops` full tanks.
    let earliest_for = |stops: i32| ((total as i32) - stops * tank_laps).max(current as i32) as i16;
    while earliest_for(stops) > latest && stops < 20 {
        stops += 1;
    }
    let earliest = earliest_for(stops).min(latest);
    // One lap short of the limit, so a thirsty lap doesn't strand you.
    let target = (latest - 1).clamp(earliest, latest);

    s.stops_required = stops as u8;
    s.earliest_pit_lap = Some(earliest);
    s.target_pit_lap = Some(target);
    s.latest_pit_lap = Some(latest);
    s.window = if current >= latest {
        PitWindow::BoxNow
    } else if current >= earliest {
        PitWindow::Open
    } else {
        PitWindow::Closed
    };
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(laps: &[f32]) -> FuelModel {
        let mut f = FuelModel::default();
        for l in laps {
            f.record_lap(*l);
        }
        f
    }

    #[test]
    fn no_data_no_strategy() {
        let f = FuelModel::default();
        assert!(f.strategy(50.0, 2, 10).is_none());
    }

    #[test]
    fn no_fuel_use_disables_strategy() {
        let mut f = model(&[0.0, 0.0]);
        assert_eq!(f.state(), FuelState::Inactive);
        assert!(f.strategy(100.0, 3, 10).is_none());
        // Live watch: 45 s of driving with a flat gauge also turns it off.
        let mut g = FuelModel::default();
        for _ in 0..(60 * 46) {
            g.observe(Some(100.0), true, 1);
        }
        assert_eq!(g.state(), FuelState::Inactive);
        // Fuel starts dropping again: back on.
        g.observe(Some(99.5), true, 1);
        assert_eq!(g.state(), FuelState::Active);
        f.record_lap(8.0);
        assert!(f.strategy(90.0, 3, 10).is_some());
    }

    #[test]
    fn no_stop_race() {
        let s = model(&[10.0, 10.0]).strategy(80.0, 3, 9).unwrap();
        assert_eq!(s.window, PitWindow::NoStop);
        assert_eq!(s.stops_required, 0);
        assert!((s.fuel_margin_laps.unwrap() - 1.0).abs() < 1e-3);
        assert!((s.projected_finish_fuel_pct.unwrap() - 10.0).abs() < 1e-3);
    }

    #[test]
    fn one_stop_window() {
        // 10 %/lap, 15 lap race, lap 2 with 85 %: fuel lasts to lap 9.
        let s = model(&[10.0, 10.0]).strategy(85.0, 2, 15).unwrap();
        assert_eq!(s.stops_required, 1);
        assert_eq!(s.latest_pit_lap, Some(9)); // 8.2 usable laps from lap 2
        assert_eq!(s.earliest_pit_lap, Some(6)); // 15 - 9 laps per tank
        assert_eq!(s.target_pit_lap, Some(8));
        assert_eq!(s.window, PitWindow::Closed);
        let open = model(&[10.0, 10.0]).strategy(45.0, 6, 15).unwrap();
        assert_eq!(open.window, PitWindow::Open);
    }

    #[test]
    fn multi_stop_race() {
        let s = model(&[10.0]).strategy(100.0, 1, 30).unwrap();
        assert_eq!(s.stops_required, 3);
        assert_eq!(s.latest_pit_lap, Some(9));
        assert_eq!(s.earliest_pit_lap, Some(3)); // 30 - 3 * 9
    }

    #[test]
    fn window_moves_with_consumption() {
        let base = model(&[10.0, 10.0, 10.0]).strategy(70.0, 4, 15).unwrap();
        let thirsty = model(&[10.0, 10.0, 13.0]).strategy(70.0, 4, 15).unwrap();
        let frugal = model(&[10.0, 10.0, 8.5]).strategy(70.0, 4, 15).unwrap();
        assert!(thirsty.latest_pit_lap < base.latest_pit_lap);
        assert!(thirsty.target_pit_lap < base.target_pit_lap);
        assert!(frugal.latest_pit_lap > base.latest_pit_lap);
    }

    #[test]
    fn latest_safe_lap_and_box_now() {
        // 25 % at 10 %/lap: 2.2 usable laps, so laps 5 and 6 can be completed.
        let s = model(&[10.0]).strategy(25.0, 5, 12).unwrap();
        assert_eq!(s.latest_pit_lap, Some(6));
        let b = model(&[10.0]).strategy(14.0, 6, 12).unwrap();
        assert_eq!(b.window, PitWindow::BoxNow);
        assert_eq!(b.latest_pit_lap, Some(6));
        assert_eq!(b.target_pit_lap, Some(6));
    }

    #[test]
    fn reacts_to_big_change() {
        let f = model(&[10.0, 10.0, 14.0]);
        assert!((f.per_lap().unwrap() - 12.0).abs() < 1e-3);
        let g = model(&[10.0, 10.0, 10.5]);
        assert!((g.per_lap().unwrap() - 10.1667).abs() < 1e-3);
    }
}
