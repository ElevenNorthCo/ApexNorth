//! Plain-text (Markdown) reports of sessions and laps, made to be pasted
//! into an AI chat (Claude, ChatGPT) for coaching. Small enough to paste:
//! lap tables, corners found from the speed trace, and a lap trace every
//! `TRACE_STEP_M` metres instead of the raw 60 Hz samples.

use std::fmt::Write;

use crate::session::models::Sample;
use crate::storage::{Db, LapDetail, LapSummary, Result, SessionSummary};

/// Distance between rows of the lap trace.
const TRACE_STEP_M: f32 = 50.0;
/// Resolution the corner finder works at.
const STEP_M: f32 = 5.0;
/// A corner: speed drops at least this much (km/h) and picks up again.
const CORNER_DROP_KPH: f32 = 12.0;
const CORNER_RISE_KPH: f32 = 8.0;
/// A corner on the other lap counts as the same one within this distance.
const MATCH_M: f32 = 120.0;

const PREAMBLE: &str = "\
Gran Turismo 7 telemetry exported from ApexNorth (apexnorth.app). \
Times are m:ss.mmm. The lap is split into 5 sectors of equal distance (S1-S5). \
Speeds in km/h, throttle and brake in percent (0-100), distances in metres from the start/finish line. \
Corners were found from the speed trace (each is a slow point: braking or lifting, then speed picks up). \
GT7 doesn't send other cars, tyre wear or car setup, so none of that is here.";

const ASK_SESSION: &str = "\
Please act as my race engineer and driving coach: look at my lap times, consistency and sectors, \
tell me where I'm losing the most time and give me 3 specific things to try on my next run.";

const ASK_LAP: &str = "\
Please act as my race engineer and driving coach: compare the two laps corner by corner \
(braking point, entry and minimum speed, when the throttle goes back to full), \
tell me where the time is lost or gained and what to change, starting with the biggest gains.";

pub fn lap_time(ms: i64) -> String {
    if ms <= 0 {
        return "-".into();
    }
    let m = ms / 60_000;
    let s = (ms % 60_000) as f64 / 1000.0;
    if m > 0 { format!("{m}:{s:06.3}") } else { format!("{s:.3}") }
}

fn secs(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

fn signed(ms: f64) -> String {
    format!("{}{:.3}", if ms < 0.0 { "-" } else { "+" }, ms.abs() / 1000.0)
}

/// "2026-11-02 18:40 UTC" without a date crate.
fn date(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y}-{m:02}-{d:02} {:02}:{:02} UTC", tod / 3600, tod % 3600 / 60)
}

fn kind_label(s: &SessionSummary) -> &'static str {
    match s.kind.as_deref() {
        Some("practice") => "Practice",
        Some("qualifying") => "Qualifying",
        Some("race") => "Race",
        Some("time_trial") => "Time trial",
        _ if s.total_laps > 0 => "Race (guessed)",
        _ => "Practice (guessed)",
    }
}

fn car(code: i32, name: Option<&str>) -> String {
    name.map_or_else(|| format!("Car #{code}"), str::to_string)
}

fn driver(rival: &Option<String>) -> String {
    match rival {
        Some(n) => format!("Rival: {n} (someone else's replay)"),
        None => "Me".into(),
    }
}

fn conditions(s: &SessionSummary) -> Option<String> {
    let c = s.conditions.as_ref()?;
    let mut out = vec![];
    if let Some(m) = &c.mode {
        out.push(if m == "online" { "online".to_string() } else { "single player".into() });
    }
    if let Some(b) = c.bop {
        out.push(format!("BoP {}", if b { "on" } else { "off" }));
    }
    let mult = |v: u8| if v == 0 { "off".to_string() } else { format!("x{v}") };
    if let Some(w) = c.tyre_wear {
        out.push(format!("tyre wear {}", mult(w)));
    }
    if let Some(f) = c.fuel_rate {
        out.push(format!("fuel use {}", mult(f)));
    }
    (!out.is_empty()).then(|| out.join(", "))
}

fn session(db: &Db, id: i64) -> Result<SessionSummary> {
    db.list_sessions()?.into_iter().find(|s| s.id == id).ok_or_else(|| format!("session {id} not found"))
}

fn session_header(out: &mut String, s: &SessionSummary) {
    let track = s.track_name.clone().unwrap_or_else(|| "Unknown track".into());
    let place = [s.venue.as_deref(), s.country.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(", ");
    let _ = writeln!(out, "- Track: {track}{}", if place.is_empty() { String::new() } else { format!(" ({place})") });
    let _ = writeln!(out, "- Car: {}", car(s.car_code, s.car_name));
    let _ = writeln!(out, "- Driver: {}", driver(&s.rival));
    let _ = writeln!(out, "- Date: {}", date(s.started_at));
    let _ = write!(out, "- Session: {}", kind_label(s));
    if s.total_laps > 0 {
        let _ = write!(out, ", {} lap race", s.total_laps);
    }
    let _ = writeln!(out);
    if let Some(c) = conditions(s) {
        let _ = writeln!(out, "- Settings: {c}");
    }
}

fn lap_notes(l: &LapSummary) -> String {
    if l.is_pit {
        return "pit lap".into();
    }
    match (l.is_valid, l.invalid_reason.as_deref()) {
        (true, _) => String::new(),
        (false, Some("joined_mid_lap")) => "not counted: timing joined mid-lap".into(),
        (false, Some("rewind_or_reset")) => "not counted: rewind or reset".into(),
        (false, Some("telemetry_jump")) => "not counted: car jumped position".into(),
        (false, Some("insufficient_distance")) => "not counted: lap too short (grid start)".into(),
        (false, _) => "not counted".into(),
    }
}

fn std_dev(v: &[f64]) -> Option<f64> {
    (v.len() >= 2).then(|| {
        let m = v.iter().sum::<f64>() / v.len() as f64;
        (v.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt()
    })
}

/// Everything about a session: laps, sectors, consistency, and the corners
/// of its best lap.
pub fn session_report(db: &Db, session_id: i64) -> Result<String> {
    let s = session(db, session_id)?;
    let laps = db.session_laps(session_id)?;
    let mut out = String::new();
    let _ = writeln!(out, "# ApexNorth session export\n\n{PREAMBLE}\n\n{ASK_SESSION}\n\n## Session");
    session_header(&mut out, &s);

    let valid: Vec<&LapSummary> = laps.iter().filter(|l| l.is_valid).collect();
    let racing: Vec<f64> = valid.iter().filter(|l| !l.is_pit).map(|l| l.time_ms as f64).collect();
    let best = valid.iter().min_by_key(|l| l.time_ms).copied();
    let _ = writeln!(out, "\n## Summary");
    let _ = writeln!(out, "- Laps recorded: {} ({} counted)", laps.len(), valid.len());
    if let Some(b) = best {
        let _ = writeln!(out, "- Best lap: {} (lap {})", lap_time(b.time_ms), b.lap_no);
    }
    if let Some(a) = s.avg_ms {
        let _ = writeln!(out, "- Average of counted laps (no pit laps): {}", lap_time(a.round() as i64));
    }
    if let Some(sd) = std_dev(&racing) {
        let _ = writeln!(out, "- Consistency (standard deviation): {:.3} s", sd / 1000.0);
    }
    if let (Some(p), Some(b)) = (s.perfect_ms, best) {
        let _ = writeln!(out, "- Perfect lap (best of each sector): {} ({} s under the best lap)", lap_time(p), secs(b.time_ms - p));
    }
    let fuel: Vec<f64> = valid.iter().filter(|l| !l.is_pit).filter_map(|l| l.fuel_used_pct).collect();
    if !fuel.is_empty() {
        let _ = writeln!(out, "- Fuel used per lap: {:.1}% of the tank", fuel.iter().sum::<f64>() / fuel.len() as f64);
    }
    if let Some(track_id) = s.track_id {
        let all = db.track_laps(track_id, s.car_code, 2000)?;
        if let Some(mine) = all.iter().find(|l| l.rival.is_none()) {
            let _ = writeln!(out, "- My all-time best here in this car: {}", lap_time(mine.time_ms));
        }
        if let Some(r) = all.iter().find(|l| l.rival.is_some()) {
            let _ = writeln!(out, "- Fastest rival lap here in this car: {} ({})", lap_time(r.time_ms), r.rival.as_deref().unwrap_or("Rival"));
        }
    }

    let _ = writeln!(out, "\n## Laps\n| Lap | Time | S1 | S2 | S3 | S4 | S5 | Fuel used | Top speed | Notes |\n|---|---|---|---|---|---|---|---|---|---|");
    for l in &laps {
        let sector = |i: usize| l.sectors_ms.get(i).map_or("-".into(), |&ms| secs(ms as i64));
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.0} | {} |",
            l.lap_no,
            lap_time(l.time_ms),
            sector(0),
            sector(1),
            sector(2),
            sector(3),
            sector(4),
            l.fuel_used_pct.map_or("-".into(), |f| format!("{f:.1}%")),
            l.max_speed,
            lap_notes(l)
        );
    }

    if let Some(b) = best {
        let lap = db.lap_detail(b.id)?;
        let corners = find_corners(&lap.samples);
        if !corners.is_empty() {
            let _ = writeln!(out, "\n## Corners on the best lap (lap {})", b.lap_no);
            corner_table(&mut out, &corners);
        }
    }
    Ok(out)
}

/// One lap in detail, against a second lap when given: sectors, each
/// corner side by side, and a trace with the running time gap.
pub fn lap_report(db: &Db, lap_id: i64, compare_id: Option<i64>) -> Result<String> {
    let lap = db.lap_detail(lap_id)?;
    let s = session(db, lap.summary.session_id)?;
    let cmp = compare_id.map(|id| db.lap_detail(id)).transpose()?;
    let mut out = String::new();
    let ask = if cmp.is_some() { ASK_LAP } else { ASK_SESSION };
    let _ = writeln!(out, "# ApexNorth lap export\n\n{PREAMBLE}\n\n{ask}\n\n## Session");
    session_header(&mut out, &s);

    let _ = writeln!(out, "\n## Laps");
    let describe = |out: &mut String, tag: &str, l: &LapDetail, rival: &Option<String>| {
        let _ = writeln!(
            out,
            "- {tag}: lap {} by {}, {} (car: {}){}",
            l.summary.lap_no,
            driver(rival),
            lap_time(l.summary.time_ms),
            car(l.car_code, l.car_name),
            if l.summary.is_valid { String::new() } else { format!(" [{}]", lap_notes(&l.summary)) }
        );
    };
    describe(&mut out, "Lap A", &lap, &lap.summary.rival);
    if let Some(c) = &cmp {
        describe(&mut out, "Lap B (reference)", c, &c.summary.rival);
        let _ = writeln!(out, "- Gap: Lap A is {} s against Lap B (+ = slower)", signed((lap.summary.time_ms - c.summary.time_ms) as f64));
    }

    let _ = writeln!(out, "\n## Sectors");
    let sector = |l: &LapDetail, i: usize| l.summary.sectors_ms.get(i).map(|&ms| ms as i64);
    match &cmp {
        None => {
            let _ = writeln!(out, "| Sector | Lap A |\n|---|---|");
            for i in 0..5 {
                let _ = writeln!(out, "| S{} | {} |", i + 1, sector(&lap, i).map_or("-".into(), secs));
            }
        }
        Some(c) => {
            let _ = writeln!(out, "| Sector | Lap A | Lap B | A - B |\n|---|---|---|---|");
            for i in 0..5 {
                let (a, b) = (sector(&lap, i), sector(c, i));
                let d = a.zip(b).map_or("-".into(), |(a, b)| signed((a - b) as f64));
                let _ = writeln!(out, "| S{} | {} | {} | {d} |", i + 1, a.map_or("-".into(), secs), b.map_or("-".into(), secs));
            }
        }
    }

    let corners = find_corners(&lap.samples);
    match &cmp {
        None => {
            if !corners.is_empty() {
                let _ = writeln!(out, "\n## Corners");
                corner_table(&mut out, &corners);
            }
        }
        Some(c) => {
            let scale = lap_len(&lap.samples) / lap_len(&c.samples).max(1.0);
            let theirs = find_corners(&c.samples);
            let _ = writeln!(
                out,
                "\n## Corners, Lap A vs Lap B\nDistances are Lap A's (Lap B's are scaled to the same lap length). \
                 \"Time A-B\" is how much the gap changed from the end of the previous corner to the end of this one (+ = Lap A lost time).\n\
                 | # | Slowest at m | Brake A / B (m) | Peak brake A / B | Entry A / B | Min speed A / B | Gear A / B | Full throttle A / B (m) | Time A-B |\n\
                 |---|---|---|---|---|---|---|---|---|"
            );
            let pairs = match_corners(&corners, &theirs, scale);
            let mut prev_end = 0.0f32;
            for (n, a) in corners.iter().enumerate() {
                let b = pairs[n].map(|j| &theirs[j]);
                let end = a.throttle_at.unwrap_or(a.min_at);
                let gain = match (gap_at(&lap.samples, &c.samples, scale, end), gap_at(&lap.samples, &c.samples, scale, prev_end)) {
                    (Some(e), Some(s)) => signed(e - s),
                    _ => "-".into(),
                };
                prev_end = end;
                let m = |v: Option<f32>, k: f32| v.map_or("-".into(), |v| format!("{:.0}", v * k));
                let _ = writeln!(
                    out,
                    "| {} | {:.0} | {} / {} | {} / {} | {:.0} / {} | {:.0} / {} | {} / {} | {} / {} | {gain} |",
                    n + 1,
                    a.min_at,
                    m(a.brake_at, 1.0),
                    m(b.and_then(|b| b.brake_at), scale),
                    a.peak_brake,
                    b.map_or("-".into(), |b| b.peak_brake.to_string()),
                    a.entry_kph,
                    m(b.map(|b| b.entry_kph), 1.0),
                    a.min_kph,
                    m(b.map(|b| b.min_kph), 1.0),
                    a.gear,
                    b.map_or("-".into(), |b| b.gear.to_string()),
                    m(a.throttle_at, 1.0),
                    m(b.and_then(|b| b.throttle_at), scale),
                );
            }
        }
    }

    let _ = writeln!(out, "\n## Trace every {TRACE_STEP_M:.0} m");
    let len = lap_len(&lap.samples);
    match &cmp {
        None => {
            let _ = writeln!(out, "| m | Speed | Throttle | Brake | Gear | Time s |\n|---|---|---|---|---|---|");
            let mut d = 0.0;
            while d <= len {
                if let Some(p) = at(&lap.samples, d) {
                    let _ = writeln!(out, "| {d:.0} | {:.0} | {:.0} | {:.0} | {} | {:.2} |", p.spd, p.thr, p.brk, p.gear, p.t / 1000.0);
                }
                d += TRACE_STEP_M;
            }
        }
        Some(c) => {
            let scale = len / lap_len(&c.samples).max(1.0);
            let _ = writeln!(
                out,
                "Gap = Lap A's time minus Lap B's at the same point (+ = Lap A behind).\n\
                 | m | Speed A / B | Throttle A / B | Brake A / B | Gear A / B | Gap s |\n|---|---|---|---|---|---|"
            );
            let mut d = 0.0;
            while d <= len {
                if let (Some(a), Some(b)) = (at(&lap.samples, d), at(&c.samples, d / scale)) {
                    let _ = writeln!(
                        out,
                        "| {d:.0} | {:.0} / {:.0} | {:.0} / {:.0} | {:.0} / {:.0} | {} / {} | {:+.2} |",
                        a.spd, b.spd, a.thr, b.thr, a.brk, b.brk, a.gear, b.gear, (a.t - b.t) / 1000.0
                    );
                }
                d += TRACE_STEP_M;
            }
        }
    }
    Ok(out)
}

fn lap_len(samples: &[Sample]) -> f32 {
    samples.last().map_or(0.0, |s| s.d)
}

/// A sample interpolated at distance `d` (times in ms as f32).
#[derive(Debug, Clone, Copy)]
struct Point {
    t: f32,
    spd: f32,
    thr: f32,
    brk: f32,
    gear: u8,
}

fn at(samples: &[Sample], d: f32) -> Option<Point> {
    let first = samples.first()?;
    if samples.len() < 2 || d < first.d || d > lap_len(samples) {
        return None;
    }
    let i = samples.partition_point(|s| s.d < d).clamp(1, samples.len() - 1);
    let (a, b) = (&samples[i - 1], &samples[i]);
    let f = if b.d > a.d { ((d - a.d) / (b.d - a.d)).clamp(0.0, 1.0) } else { 0.0 };
    let lerp = |x: f32, y: f32| x + (y - x) * f;
    Some(Point {
        t: lerp(a.t as f32, b.t as f32),
        spd: lerp(a.spd, b.spd),
        thr: lerp(a.thr as f32, b.thr as f32),
        brk: lerp(a.brk as f32, b.brk as f32),
        gear: if f < 0.5 { a.gear } else { b.gear },
    })
}

/// Lap A's time minus lap B's at distance `d` along A (ms).
fn gap_at(a: &[Sample], b: &[Sample], scale: f32, d: f32) -> Option<f64> {
    if d <= 0.0 {
        return Some(0.0);
    }
    Some((at(a, d)?.t - at(b, d / scale)?.t) as f64)
}

#[derive(Debug, Clone, PartialEq)]
struct Corner {
    /// Where the car is slowest (m).
    min_at: f32,
    min_kph: f32,
    /// Fastest point on the way in.
    entry_kph: f32,
    /// Where braking starts (None = a lift, no brake).
    brake_at: Option<f32>,
    peak_brake: u8,
    gear: u8,
    /// Where the throttle is back to full after the corner.
    throttle_at: Option<f32>,
}

/// Slow points of a lap: the speed dips by `CORNER_DROP_KPH` and climbs
/// back by `CORNER_RISE_KPH`.
fn find_corners(samples: &[Sample]) -> Vec<Corner> {
    let len = lap_len(samples);
    let pts: Vec<(f32, Point)> = (0..)
        .map(|k| k as f32 * STEP_M)
        .take_while(|&d| d <= len)
        .filter_map(|d| at(samples, d).map(|p| (d, p)))
        .collect();
    if pts.len() < 3 {
        return vec![];
    }
    // Hysteresis peak/valley search over speed.
    let mut minima: Vec<usize> = vec![];
    let mut peak = 0usize;
    let mut valley: Option<usize> = None;
    for i in 1..pts.len() {
        let v = pts[i].1.spd;
        match valley {
            None => {
                if v > pts[peak].1.spd {
                    peak = i;
                } else if pts[peak].1.spd - v >= CORNER_DROP_KPH {
                    valley = Some(i);
                }
            }
            Some(m) => {
                if v < pts[m].1.spd {
                    valley = Some(i);
                } else if v - pts[m].1.spd >= CORNER_RISE_KPH {
                    minima.push(m);
                    valley = None;
                    peak = i;
                }
            }
        }
    }
    let mut out = vec![];
    for (k, &m) in minima.iter().enumerate() {
        let from = if k == 0 { 0 } else { minima[k - 1] };
        let to = minima.get(k + 1).copied().unwrap_or(pts.len() - 1);
        let (entry_i, _) = pts[from..=m]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1 .1.spd.total_cmp(&b.1 .1.spd))
            .map(|(i, p)| (from + i, p))
            .unwrap();
        let brake_i = (entry_i..=m).find(|&i| pts[i].1.brk >= 10.0);
        let peak_brake = pts[entry_i..=m].iter().map(|p| p.1.brk).fold(0.0f32, f32::max);
        let throttle_i = (m..=to).find(|&i| pts[i].1.thr >= 95.0);
        out.push(Corner {
            min_at: pts[m].0,
            min_kph: pts[m].1.spd,
            entry_kph: pts[entry_i].1.spd,
            brake_at: brake_i.map(|i| pts[i].0),
            peak_brake: peak_brake.round() as u8,
            gear: pts[m].1.gear,
            throttle_at: throttle_i.map(|i| pts[i].0),
        });
    }
    out
}

/// Pair each corner of lap A with one of lap B's (B's distances times
/// `scale`), closest pairs first, each corner used once.
fn match_corners(a: &[Corner], b: &[Corner], scale: f32) -> Vec<Option<usize>> {
    let mut pairs: Vec<(f32, usize, usize)> = a
        .iter()
        .enumerate()
        .flat_map(|(i, ca)| b.iter().enumerate().map(move |(j, cb)| ((cb.min_at * scale - ca.min_at).abs(), i, j)))
        .filter(|p| p.0 <= MATCH_M)
        .collect();
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = vec![None; a.len()];
    let mut used = vec![false; b.len()];
    for (_, i, j) in pairs {
        if out[i].is_none() && !used[j] {
            out[i] = Some(j);
            used[j] = true;
        }
    }
    out
}

fn corner_table(out: &mut String, corners: &[Corner]) {
    let _ = writeln!(
        out,
        "| # | Slowest at m | Brake at m | Peak brake | Entry km/h | Min km/h | Gear | Full throttle at m |\n|---|---|---|---|---|---|---|---|"
    );
    for (n, c) in corners.iter().enumerate() {
        let _ = writeln!(
            out,
            "| {} | {:.0} | {} | {} | {:.0} | {:.0} | {} | {} |",
            n + 1,
            c.min_at,
            c.brake_at.map_or("lift".into(), |d| format!("{d:.0}")),
            c.peak_brake,
            c.entry_kph,
            c.min_kph,
            c.gear,
            c.throttle_at.map_or("-".into(), |d| format!("{d:.0}")),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 km lap at 30 Hz-ish with two corners: a braked hairpin at 600 m
    /// and a lift at 1500 m.
    fn lap(brake_shift: f32, slower: f32) -> Vec<Sample> {
        let mut out = vec![];
        let mut t = 0.0f32;
        let mut d = 0.0f32;
        while d <= 2000.0 {
            let (spd, thr, brk) = speed_at(d, brake_shift, slower);
            out.push(Sample { t: t as u32, d, spd, thr, brk, gear: if spd < 120.0 { 2 } else { 5 }, ..Default::default() });
            let step = 2.0;
            t += step / (spd / 3.6) * 1000.0;
            d += step;
        }
        out
    }

    fn speed_at(d: f32, shift: f32, slower: f32) -> (f32, u8, u8) {
        let hairpin = 600.0;
        let brake = 480.0 + shift;
        if d >= brake && d < hairpin {
            let f = (d - brake) / (hairpin - brake);
            (240.0 - f * (240.0 - 80.0 + slower), 0, 90)
        } else if d >= hairpin && d < 800.0 {
            let f = (d - hairpin) / 200.0;
            (80.0 - slower + f * (160.0 + slower), if f > 0.5 { 100 } else { 60 }, 0)
        } else if (1400.0..1500.0).contains(&d) {
            (240.0 - (d - 1400.0) * 0.3, 0, 0)
        } else if (1500.0..1600.0).contains(&d) {
            (210.0 + (d - 1500.0) * 0.3, 100, 0)
        } else {
            (240.0, 100, 0)
        }
    }

    #[test]
    fn finds_braked_and_lifted_corners() {
        let c = find_corners(&lap(0.0, 0.0));
        assert_eq!(c.len(), 2, "{c:?}");
        assert!((c[0].min_at - 600.0).abs() <= STEP_M * 2.0, "{c:?}");
        assert!((c[0].brake_at.unwrap() - 480.0).abs() <= STEP_M * 2.0, "{c:?}");
        assert!(c[0].min_kph < 85.0 && c[0].entry_kph > 235.0);
        assert_eq!(c[0].gear, 2);
        assert!(c[0].throttle_at.unwrap() > 650.0);
        assert_eq!(c[1].brake_at, None, "the second corner is a lift");
    }

    #[test]
    fn formats_times_and_dates() {
        assert_eq!(lap_time(94_700), "1:34.700");
        assert_eq!(lap_time(59_001), "59.001");
        assert_eq!(signed(-1234.0), "-1.234");
        assert_eq!(date(0), "1970-01-01 00:00 UTC");
        assert_eq!(date(1_793_000_000_000), "2026-10-26 07:33 UTC");
    }

    #[test]
    fn reports_a_session_and_a_rival_comparison() {
        use crate::session::models::CompletedLap;
        let db = Db::memory().unwrap();
        let mk = |samples: Vec<Sample>, valid: bool| CompletedLap {
            lap_no: 1,
            time_ms: samples.last().unwrap().t,
            is_pit: false,
            is_valid: valid,
            invalid_reason: None,
            sectors_ms: vec![10_000, 11_000, 12_000, 13_000, 14_000],
            fuel_used_pct: Some(2.0),
            max_speed: 240.0,
            length_m: 2000.0,
            start_pos: [0.0; 3],
            samples,
        };
        let me = db.create_session(1234, 0, 1_793_000_000_000).unwrap();
        let rival = db.create_session(1234, 0, 1_793_000_100_000).unwrap();
        let t = db.identify_track([0.0; 3], 2000.0, &[]).unwrap();
        db.set_session_track(me, t.id).unwrap();
        db.set_session_track(rival, t.id).unwrap();
        db.set_session_rival(rival, Some("  Kaz  ")).unwrap();
        let mine = db.insert_lap(me, &mk(lap(-40.0, 15.0), true)).unwrap();
        let theirs = db.insert_lap(rival, &mk(lap(0.0, 0.0), true)).unwrap();

        let r = session_report(&db, me).unwrap();
        assert!(r.contains("- Driver: Me"), "{r}");
        assert!(r.contains("Fastest rival lap here in this car"), "{r}");
        assert!(r.contains("## Corners on the best lap"), "{r}");

        let r = lap_report(&db, mine, Some(theirs)).unwrap();
        assert!(r.contains("Lap B (reference): lap 1 by Rival: Kaz"), "{r}");
        assert!(r.contains("## Corners, Lap A vs Lap B"), "{r}");
        // Lap A brakes 40 m early for the hairpin and loses time there.
        let row = r.lines().find(|l| l.starts_with("| 1 |")).unwrap();
        assert!(row.contains("440 / 480"), "{row}");
        assert!(row.trim_end().ends_with('|') && row.contains("| +"), "{row}");
        assert!(r.contains("## Trace every 50 m"));
    }
}
