//! Every GT7 track layout, and recognising which one a lap was driven on.
//!
//! GT7 sends no track ID, but world coordinates are fixed per track, so a lap's
//! (x, z) line is a fingerprint. Each layout with geometry carries a reference
//! racing line (one point every ~20 m, in driving order). A lap matches a
//! layout when:
//!
//! - its length is close to the reference line's,
//! - nearly all of the lap lies on the line *and* nearly all of the line was
//!   driven (two-way coverage, so a sub-layout doesn't match its parent),
//! - it went round the same way (reverse layouts are the forward line walked
//!   backwards).
//!
//! Layouts that share almost all their tarmac (Monza with and without the
//! chicane) are told apart by the longest stretch where the lap and the line
//! part company: the right one has none, the wrong one has the chicane.
//!
//! Source: jbhoorasingh/gt7-datalogger-track-data (CC0), which includes
//! captures from zetetos/gt-telemetry (MIT).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

const TRACKS_JSON: &str = include_str!("../../data/gt7_tracks.json");

/// Spacing the reference line is resampled to for nearest-point lookups.
const PATH_STEP_M: f32 = 5.0;
/// Spacing the lap is resampled to.
const LAP_STEP_M: f32 = 10.0;
/// Grid cell for spatial lookups.
const CELL_M: f32 = 20.0;
/// A point is "on" a line within this distance (wide enough for any racing line).
const ON_LINE_M: f32 = 25.0;
/// Distance at which a lap and a line count as having parted (chicane test).
const APART_M: f32 = 15.0;
/// Lap length vs reference line length.
const LENGTH_TOLERANCE: f32 = 0.12;
/// Minimum two-way coverage to accept a layout.
const MIN_SCORE: f32 = 0.80;
/// Layouts scoring within this of the best are tie-broken on divergence.
const CONTENDER_MARGIN: f32 = 0.05;
/// Share of steps that must go round the line's way (+1 all forward).
const MIN_DIRECTION: f32 = 0.5;
/// Sub-layouts without geometry: share of the lap that must lie on the venue's
/// known tarmac, and how close the lap must be to the official length.
const SUB_LAYOUT_COVER: f32 = 0.80;
const SUB_LAYOUT_LENGTH_TOLERANCE: f32 = 0.07;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLayout {
    id: String,
    name: String,
    venue: String,
    country: String,
    length_m: f32,
    reverse: bool,
    #[serde(default)]
    path: Option<Vec<[f32; 2]>>,
    #[serde(default)]
    reverse_of: Option<String>,
}

#[derive(Deserialize)]
struct RawDoc {
    layouts: Vec<RawLayout>,
}

/// One track layout ("Autodromo Nazionale Monza No Chicane").
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub id: String,
    pub name: String,
    pub venue: String,
    pub country: String,
    /// Official length in metres.
    pub length_m: f32,
    pub reverse: bool,
    #[serde(skip)]
    geo: Option<Geometry>,
}

#[derive(Debug)]
struct Geometry {
    /// Reference line resampled every `PATH_STEP_M`, in driving order.
    pts: Vec<[f32; 2]>,
    grid: Grid,
    len: f32,
    min: [f32; 2],
    max: [f32; 2],
}

/// Points bucketed into `CELL_M` squares.
#[derive(Debug)]
struct Grid(HashMap<(i32, i32), Vec<u32>>);

fn cell(p: [f32; 2]) -> (i32, i32) {
    ((p[0] / CELL_M).floor() as i32, (p[1] / CELL_M).floor() as i32)
}

impl Grid {
    fn build(pts: &[[f32; 2]]) -> Grid {
        let mut g: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, p) in pts.iter().enumerate() {
            g.entry(cell(*p)).or_default().push(i as u32);
        }
        Grid(g)
    }

    /// Index of the nearest point within `r` (r ≤ CELL_M).
    fn nearest(&self, pts: &[[f32; 2]], p: [f32; 2], r: f32) -> Option<u32> {
        let (cx, cz) = cell(p);
        let mut best = None;
        let mut best_d = r * r;
        for a in -1..=1 {
            for b in -1..=1 {
                for &i in self.0.get(&(cx + a, cz + b)).into_iter().flatten() {
                    let q = pts[i as usize];
                    let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
                    if d <= best_d {
                        best_d = d;
                        best = Some(i);
                    }
                }
            }
        }
        best
    }
}

fn polyline_len(pts: &[[f32; 2]]) -> f32 {
    pts.windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum()
}

/// Points every `step` metres along the polyline.
fn resample(pts: &[[f32; 2]], step: f32) -> Vec<[f32; 2]> {
    let Some(&first) = pts.first() else { return Vec::new() };
    let mut out = vec![first];
    let mut carry = 0.0;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = (b[0] - a[0]).hypot(b[1] - a[1]);
        if seg <= 0.0 {
            continue;
        }
        let mut t = step - carry;
        while t <= seg {
            let f = t / seg;
            out.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
            t += step;
        }
        carry = seg - (t - step);
    }
    out
}

impl Geometry {
    fn new(line: Vec<[f32; 2]>) -> Geometry {
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for p in &line {
            for k in 0..2 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        let len = polyline_len(&line);
        let pts = resample(&line, PATH_STEP_M);
        Geometry { grid: Grid::build(&pts), pts, len, min, max }
    }

    fn overlaps(&self, lap: &Lap) -> bool {
        let m = ON_LINE_M;
        lap.min[0] <= self.max[0] + m
            && lap.max[0] >= self.min[0] - m
            && lap.min[1] <= self.max[1] + m
            && lap.max[1] >= self.min[1] - m
    }
}

/// A lap prepared for matching.
struct Lap {
    pts: Vec<[f32; 2]>,
    grid: Grid,
    len: f32,
    min: [f32; 2],
    max: [f32; 2],
}

impl Lap {
    fn new(line: &[[f32; 2]], len: f32) -> Lap {
        let pts = resample(line, LAP_STEP_M);
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for p in &pts {
            for k in 0..2 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        let len = if len > 0.0 { len } else { polyline_len(line) };
        Lap { grid: Grid::build(&pts), pts, len, min, max }
    }
}

/// How well a lap fits one layout's line.
#[derive(Debug, Clone, Copy)]
struct Fit {
    /// Share of the lap on the line.
    on_line: f32,
    /// Share of the line the lap drove.
    driven: f32,
    /// +1 when the lap went round the line's way, -1 the other way.
    direction: f32,
}

impl Fit {
    fn score(&self) -> f32 {
        self.on_line.min(self.driven)
    }
}

fn fit(lap: &Lap, g: &Geometry) -> Fit {
    let hits: Vec<Option<u32>> = lap.pts.iter().map(|&p| g.grid.nearest(&g.pts, p, ON_LINE_M)).collect();
    let on_line = hits.iter().filter(|h| h.is_some()).count() as f32 / lap.pts.len().max(1) as f32;
    if on_line < 0.5 {
        return Fit { on_line, driven: 0.0, direction: 0.0 };
    }
    // Every other 5 m point = one per 10 m along the line.
    let line: Vec<_> = g.pts.iter().step_by(2).collect();
    let driven_n = line.iter().filter(|&&&p| lap.grid.nearest(&lap.pts, p, ON_LINE_M).is_some()).count();
    let driven = driven_n as f32 / line.len().max(1) as f32;

    // Walk the nearest line index along the lap: it advances when the lap goes
    // the line's way round. A figure-eight can't fool this (both arms of the
    // crossover are at different indices), unlike the loop's signed area.
    let idx: Vec<u32> = hits.into_iter().flatten().collect();
    let n = g.pts.len() as u32;
    let step = (idx.len() / 200).max(1);
    let (mut fw, mut bw) = (0u32, 0u32);
    let sub: Vec<u32> = idx.into_iter().step_by(step).collect();
    for w in sub.windows(2) {
        let d = (w[1] + n - w[0]) % n;
        if d == 0 {
            continue;
        }
        if d < n / 2 {
            fw += 1;
        } else {
            bw += 1;
        }
    }
    let direction = if fw + bw == 0 { 0.0 } else { (fw as f32 - bw as f32) / (fw + bw) as f32 };
    Fit { on_line, driven, direction }
}

/// Longest stretch (metres) where the lap and the line are more than
/// `APART_M` apart, in either direction.
fn divergence(lap: &Lap, g: &Geometry) -> f32 {
    let run = |pts: &mut dyn Iterator<Item = [f32; 2]>, near: &dyn Fn([f32; 2]) -> bool| {
        let (mut cur, mut best) = (0.0f32, 0.0f32);
        for p in pts {
            if near(p) {
                cur = 0.0;
            } else {
                cur += LAP_STEP_M;
                best = best.max(cur);
            }
        }
        best
    };
    let lap_off = run(&mut lap.pts.iter().copied(), &|p| g.grid.nearest(&g.pts, p, APART_M).is_some());
    let line_off = run(&mut g.pts.iter().step_by(2).copied(), &|p| lap.grid.nearest(&lap.pts, p, APART_M).is_some());
    lap_off.max(line_off)
}

pub struct Catalog {
    layouts: Vec<Layout>,
}

impl Catalog {
    fn load() -> Catalog {
        let doc: RawDoc = serde_json::from_str(TRACKS_JSON).expect("bundled gt7_tracks.json is valid");
        let paths: HashMap<String, Vec<[f32; 2]>> = doc
            .layouts
            .iter()
            .filter_map(|l| Some((l.id.clone(), l.path.clone()?)))
            .collect();
        let layouts = doc
            .layouts
            .into_iter()
            .map(|l| {
                let line = match (l.path, &l.reverse_of) {
                    (Some(p), _) => Some(p),
                    (None, Some(fwd)) => paths.get(fwd).map(|p| p.iter().rev().copied().collect()),
                    _ => None,
                };
                Layout {
                    id: l.id,
                    name: l.name,
                    venue: l.venue,
                    country: l.country,
                    length_m: l.length_m,
                    reverse: l.reverse,
                    geo: line.filter(|p| p.len() >= 8).map(Geometry::new),
                }
            })
            .collect();
        Catalog { layouts }
    }

    pub fn layouts(&self) -> &[Layout] {
        &self.layouts
    }

    pub fn by_id(&self, id: &str) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.name.eq_ignore_ascii_case(name.trim()))
    }

    /// The layout a lap was driven on. `line` is the lap's (x, z) positions in
    /// driving order; `length_m` its driven distance (0 = measure the line).
    /// None when nothing fits clearly; a wrong name is worse than no name.
    pub fn identify(&self, line: &[[f32; 2]], length_m: f32) -> Option<&Layout> {
        let lap = Lap::new(line, length_m);
        if lap.pts.len() < 20 {
            return None;
        }
        let mut fits: Vec<(&Layout, &Geometry, Fit)> = self
            .layouts
            .iter()
            .filter_map(|l| {
                let g = l.geo.as_ref()?;
                g.overlaps(&lap).then(|| (l, g, fit(&lap, g)))
            })
            .collect();

        let length_ok = |g: &Geometry| (g.len - lap.len).abs() / g.len.max(1.0) <= LENGTH_TOLERANCE;
        let best = fits
            .iter()
            .filter(|(_, g, f)| length_ok(g) && f.direction > MIN_DIRECTION && f.score() >= MIN_SCORE)
            .map(|(_, _, f)| f.score())
            .fold(None, |m: Option<f32>, s| Some(m.map_or(s, |m| m.max(s))));
        if let Some(best) = best {
            return fits
                .iter()
                .filter(|(_, g, f)| {
                    length_ok(g) && f.direction > MIN_DIRECTION && f.score() >= best - CONTENDER_MARGIN
                })
                .map(|(l, g, f)| (*l, divergence(&lap, g), f.score()))
                .min_by(|a, b| a.1.total_cmp(&b.1).then(b.2.total_cmp(&a.2)))
                .map(|(l, _, _)| l);
        }

        // A short layout we have no line for (Suzuka East, Nürburgring Sprint):
        // the lap is on a known venue's tarmac and matches the official length.
        fits.retain(|(_, _, f)| f.on_line >= SUB_LAYOUT_COVER && f.direction > MIN_DIRECTION);
        fits.sort_by(|a, b| b.2.on_line.total_cmp(&a.2.on_line));
        let venue = &fits.first()?.0.venue;
        self.layouts
            .iter()
            .filter(|l| l.geo.is_none() && &l.venue == venue)
            .map(|l| (l, (l.length_m - lap.len).abs() / l.length_m.max(1.0)))
            .filter(|(_, e)| *e <= SUB_LAYOUT_LENGTH_TOLERANCE)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l)
    }
}

#[cfg(test)]
impl Catalog {
    /// A layout's reference line every 5 m, for tests elsewhere.
    pub fn reference_line(&self, name: &str) -> Vec<[f32; 2]> {
        self.by_name(name).and_then(|l| l.geo.as_ref()).map(|g| g.pts.clone()).unwrap_or_default()
    }
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(Catalog::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A believable lap of `layout`: its line from a different start point,
    /// pushed sideways a few metres (a different racing line) plus jitter.
    fn drive(layout: &Layout, seed: u32) -> (Vec<[f32; 2]>, f32) {
        let g = layout.geo.as_ref().unwrap();
        let n = g.pts.len();
        let start = (seed as usize * 7919) % n;
        let mut rng = seed.wrapping_mul(2654435761).max(1);
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng % 1000) as f32 / 1000.0 - 0.5
        };
        let mut line = Vec::with_capacity(n / 2);
        for k in (0..n).step_by(2) {
            let i = (start + k) % n;
            let (a, b) = (g.pts[i], g.pts[(i + 1) % n]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let norm = dx.hypot(dz).max(1e-3);
            let offset = 4.0 * ((k as f32) / 300.0).sin();
            line.push([a[0] - dz / norm * offset + next() * 4.0, a[1] + dx / norm * offset + next() * 4.0]);
        }
        (line, g.len)
    }

    #[test]
    fn catalog_lists_every_layout() {
        let c = catalog();
        assert_eq!(c.layouts().len(), 121);
        assert!(c.layouts().iter().filter(|l| l.geo.is_some()).count() >= 110);
        assert!(c.by_name("suzuka circuit").is_some());
    }

    #[test]
    fn every_layout_with_geometry_recognises_itself() {
        let c = catalog();
        let mut wrong = Vec::new();
        for (i, l) in c.layouts().iter().enumerate().filter(|(_, l)| l.geo.is_some()) {
            let (line, len) = drive(l, i as u32 + 1);
            let got = c.identify(&line, len).map(|g| g.name.as_str());
            if got != Some(l.name.as_str()) {
                wrong.push(format!("{} -> {:?}", l.name, got));
            }
        }
        assert!(wrong.is_empty(), "misidentified:\n{}", wrong.join("\n"));
    }

    #[test]
    fn reverse_layouts_are_told_apart() {
        let c = catalog();
        let fwd = c.by_name("Alsace - Village").unwrap();
        let (mut line, len) = drive(fwd, 3);
        assert_eq!(c.identify(&line, len).unwrap().name, "Alsace - Village");
        line.reverse();
        assert_eq!(c.identify(&line, len).unwrap().name, "Alsace - Village (Reverse)");
    }

    #[test]
    fn chicane_variants_are_told_apart() {
        let c = catalog();
        for name in ["Autodromo Nazionale Monza", "Autodromo Nazionale Monza No Chicane"] {
            let (line, len) = drive(c.by_name(name).unwrap(), 11);
            assert_eq!(c.identify(&line, len).unwrap().name, name);
        }
    }

    #[test]
    fn short_layout_without_geometry_is_found_by_venue_and_length() {
        // The first ~3.6 km of the GP line stands in for a Sprint lap.
        let c = catalog();
        let gp = c.by_name("Nürburgring GP").unwrap().geo.as_ref().unwrap();
        let line: Vec<_> = gp.pts.iter().take((3629.0 / PATH_STEP_M) as usize).copied().collect();
        assert_eq!(c.identify(&line, 3629.0).unwrap().name, "Nürburgring Sprint");
    }

    #[test]
    fn unknown_geometry_is_not_named() {
        // A 3.7 km circle in the middle of nowhere, like the demo track.
        let line: Vec<[f32; 2]> = (0..=600)
            .map(|i| {
                let a = i as f32 / 600.0 * std::f32::consts::TAU;
                [50_000.0 + 590.0 * a.cos(), 50_000.0 + 590.0 * a.sin()]
            })
            .collect();
        assert!(catalog().identify(&line, 0.0).is_none());
        assert!(catalog().identify(&[], 0.0).is_none());
    }

    #[test]
    fn real_captured_laps_are_recognised() {
        // Single laps cut from independent PS5 captures (vthinsel/GT7Tracks, MIT),
        // not from the data the catalog was built from.
        let laps: HashMap<String, Vec<[f32; 2]>> =
            serde_json::from_str(include_str!("testdata/captured_laps.json")).unwrap();
        assert_eq!(laps.len(), 4);
        for (name, line) in &laps {
            let got = catalog().identify(line, 0.0).map(|l| l.name.as_str());
            assert_eq!(got, Some(name.as_str()), "captured lap of {name}");
        }
    }
}
