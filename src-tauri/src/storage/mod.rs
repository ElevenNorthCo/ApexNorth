//! SQLite persistence for tracks, sessions and recorded laps.
//!
//! Lap samples are stored as a zstd-compressed bincode blob.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::catalog::{cars, tracks};
use crate::session::models::{CompletedLap, InvalidReason, Sample};
use crate::session::tyres::fade_per_wear;

pub type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRow {
    pub id: i64,
    pub name: String,
    pub length_m: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: i64,
    pub track_id: Option<i64>,
    pub track_name: Option<String>,
    pub car_code: i32,
    /// "Porsche 911 GT3 RS (992) '22", when the car code is in the catalog.
    pub car_name: Option<&'static str>,
    pub started_at: i64,
    pub total_laps: i32,
    pub lap_count: i64,
    pub best_ms: Option<i64>,
    /// Mean of the valid, non-pit laps.
    pub avg_ms: Option<f64>,
    /// The best of each sector across the valid laps, added up: the lap
    /// you'd do with every sector at its best.
    pub perfect_ms: Option<i64>,
    /// Track shape as [x, z] points, for thumbnails.
    pub outline: Vec<[f32; 2]>,
    /// Session type the user tagged ("practice", "qualifying", "race",
    /// "time_trial"), or None to infer it from the lap count.
    pub kind: Option<String>,
    /// Race settings the user noted (GT7 doesn't send them).
    pub conditions: Option<SessionConditions>,
    /// Catalog layout the track was recognised as, with its venue and country.
    pub layout_id: Option<String>,
    pub venue: Option<String>,
    pub country: Option<String>,
}

/// Settings a race was run with, tagged by the user. GT7 sends none of
/// these, so every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionConditions {
    /// "offline" (single player / career), "online" (Sport mode, lobby).
    pub mode: Option<String>,
    /// Balance of Performance on.
    pub bop: Option<bool>,
    /// Tyre wear multiplier (0 = off).
    pub tyre_wear: Option<u8>,
    /// Fuel consumption multiplier (0 = off).
    pub fuel_rate: Option<u8>,
}

/// Session types the user can tag a session with.
pub const SESSION_KINDS: [&str; 4] = ["practice", "qualifying", "race", "time_trial"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapSummary {
    pub id: i64,
    pub session_id: i64,
    pub lap_no: i32,
    pub time_ms: i64,
    pub is_pit: bool,
    pub is_valid: bool,
    /// Why the lap is invalid (see `InvalidReason`), for laps saved since Phase 3.5.
    pub invalid_reason: Option<String>,
    pub sectors_ms: Vec<u32>,
    pub fuel_used_pct: Option<f64>,
    pub max_speed: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapDetail {
    #[serde(flatten)]
    pub summary: LapSummary,
    pub car_code: i32,
    pub car_name: Option<&'static str>,
    pub track_id: Option<i64>,
    pub track_name: Option<String>,
    pub samples: Vec<Sample>,
}

fn encode(samples: &[Sample]) -> Result<Vec<u8>> {
    let raw = bincode::serialize(samples).map_err(err)?;
    zstd::encode_all(&raw[..], 3).map_err(err)
}

fn decode(blob: &[u8]) -> Result<Vec<Sample>> {
    let raw = zstd::decode_all(blob).map_err(err)?;
    bincode::deserialize(&raw).map_err(err)
}

/// The sum of the best time in each sector over `laps` (each a list of sector
/// times). None unless every sector has a time: a lap with fewer sectors
/// recorded can't stand in for the missing ones.
pub fn perfect_lap(laps: &[Vec<u32>]) -> Option<i64> {
    let n = laps.iter().map(Vec::len).max().filter(|&n| n > 0)?;
    (0..n)
        .map(|i| laps.iter().filter_map(|l| l.get(i)).filter(|&&ms| ms > 0).min().map(|&ms| ms as i64))
        .sum()
}

const LAP_COLS: &str = "l.id, l.session_id, l.lap_no, l.time_ms, l.is_pit, l.is_valid, l.sectors, l.fuel_used, l.max_speed, l.invalid_reason";

fn lap_row(r: &rusqlite::Row) -> rusqlite::Result<LapSummary> {
    let sectors: String = r.get(6)?;
    Ok(LapSummary {
        id: r.get(0)?,
        session_id: r.get(1)?,
        lap_no: r.get(2)?,
        time_ms: r.get(3)?,
        is_pit: r.get(4)?,
        is_valid: r.get(5)?,
        sectors_ms: serde_json::from_str(&sectors).unwrap_or_default(),
        fuel_used_pct: r.get(7)?,
        max_speed: r.get(8)?,
        invalid_reason: r.get(9)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path).map_err(err)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn memory() -> Result<Db> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS tracks (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                sf_x REAL NOT NULL, sf_y REAL NOT NULL, sf_z REAL NOT NULL,
                length_m REAL NOT NULL,
                outline TEXT NOT NULL DEFAULT '[]'
             );
             CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY,
                track_id INTEGER REFERENCES tracks(id) ON DELETE SET NULL,
                car_code INTEGER NOT NULL,
                started_at INTEGER NOT NULL,
                total_laps INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS laps (
                id INTEGER PRIMARY KEY,
                session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                lap_no INTEGER NOT NULL,
                time_ms INTEGER NOT NULL,
                is_pit INTEGER NOT NULL,
                is_valid INTEGER NOT NULL,
                sectors TEXT NOT NULL,
                fuel_used REAL,
                max_speed REAL NOT NULL,
                length_m REAL NOT NULL,
                samples BLOB NOT NULL
             );
             CREATE INDEX IF NOT EXISTS laps_session ON laps(session_id);",
        )
        .map_err(err)?;
        let has_reason: bool = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('laps') WHERE name = 'invalid_reason'", [], |r| r.get::<_, i64>(0))
            .map_err(err)?
            > 0;
        if !has_reason {
            conn.execute_batch("ALTER TABLE laps ADD COLUMN invalid_reason TEXT").map_err(err)?;
        }
        let has_layout: bool = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('tracks') WHERE name = 'layout_id'", [], |r| r.get::<_, i64>(0))
            .map_err(err)?
            > 0;
        if !has_layout {
            // The catalog layout (see catalog::tracks) a track was recognised as.
            conn.execute_batch("ALTER TABLE tracks ADD COLUMN layout_id TEXT").map_err(err)?;
        }
        let has_kind: bool = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'kind'", [], |r| r.get::<_, i64>(0))
            .map_err(err)?
            > 0;
        if !has_kind {
            // User-chosen session type (see SESSION_KINDS); NULL = inferred.
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN kind TEXT").map_err(err)?;
        }
        let has_conditions: bool = conn
            .query_row("SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'conditions'", [], |r| r.get::<_, i64>(0))
            .map_err(err)?
            > 0;
        if !has_conditions {
            // JSON SessionConditions; NULL = not noted.
            conn.execute_batch("ALTER TABLE sessions ADD COLUMN conditions TEXT").map_err(err)?;
        }
        Ok(Db { conn })
    }

    pub fn create_session(&self, car_code: i32, total_laps: i32, started_at: i64) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO sessions (car_code, started_at, total_laps) VALUES (?1, ?2, ?3)",
                params![car_code, started_at, total_laps],
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Remove sessions that never recorded a lap.
    pub fn prune_empty_sessions(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE id NOT IN (SELECT DISTINCT session_id FROM laps)", [])
            .map_err(err)?;
        Ok(())
    }

    /// GT7 does not report the track. A lap is first matched against the
    /// built-in catalog of GT7 layouts (by its driven line); failing that,
    /// tracks are fingerprinted by the start/finish position and lap length
    /// and get a placeholder name until renamed.
    pub fn identify_track(&self, start: [f32; 3], length_m: f32, samples: &[Sample]) -> Result<TrackRow> {
        self.track_with(recognise(length_m, samples), start, length_m, samples)
    }

    /// Like `identify_track`, but only for a lap the catalog recognises. Safe
    /// for laps that don't count (pit laps, laps with a reset): recognition
    /// needs the whole lap on a known layout's line.
    pub fn recognise_track(&self, start: [f32; 3], length_m: f32, samples: &[Sample]) -> Result<Option<TrackRow>> {
        match recognise(length_m, samples) {
            Some(l) => self.track_with(Some(l), start, length_m, samples).map(Some),
            None => Ok(None),
        }
    }

    fn track_with(
        &self,
        layout: Option<&tracks::Layout>,
        start: [f32; 3],
        length_m: f32,
        samples: &[Sample],
    ) -> Result<TrackRow> {
        if let Some(l) = layout {
            if let Some(t) = self.track_for_layout(&l.id)? {
                return Ok(t);
            }
        }

        let mut stmt = self
            .conn
            .prepare("SELECT id, name, length_m, sf_x, sf_y, sf_z, layout_id FROM tracks ORDER BY id")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    TrackRow { id: r.get(0)?, name: r.get(1)?, length_m: r.get(2)? },
                    [r.get::<_, f64>(3)?, r.get::<_, f64>(4)?, r.get::<_, f64>(5)?],
                    r.get::<_, Option<String>>(6)?,
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (mut track, sf, layout_id) = row.map_err(err)?;
            // Forward and reverse layouts often share a start line and length:
            // a track already tied to another layout is a different track.
            if layout_id.is_some() && layout.is_some() {
                continue;
            }
            let d = ((sf[0] - start[0] as f64).powi(2) + (sf[2] - start[2] as f64).powi(2)).sqrt();
            let len_ratio = (track.length_m - length_m as f64).abs() / track.length_m.max(1.0);
            if d < 60.0 && len_ratio < 0.04 {
                if let Some(l) = layout {
                    track.name = self.adopt_layout(track.id, &track.name, l)?;
                }
                return Ok(track);
            }
        }
        drop(stmt);

        let name = match layout {
            Some(l) => l.name.clone(),
            None => {
                let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).map_err(err)?;
                placeholder_name(count + 1, length_m)
            }
        };
        let step = (samples.len() / 200).max(1);
        let outline: Vec<[f32; 2]> = samples.iter().step_by(step).map(|s| [s.x, s.z]).collect();
        self.conn
            .execute(
                "INSERT INTO tracks (name, sf_x, sf_y, sf_z, length_m, outline, layout_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    name,
                    start[0],
                    start[1],
                    start[2],
                    length_m,
                    serde_json::to_string(&outline).map_err(err)?,
                    layout.map(|l| l.id.as_str()),
                ],
            )
            .map_err(err)?;
        Ok(TrackRow { id: self.conn.last_insert_rowid(), name, length_m: length_m as f64 })
    }

    fn track_for_layout(&self, layout_id: &str) -> Result<Option<TrackRow>> {
        self.conn
            .query_row(
                "SELECT id, name, length_m FROM tracks WHERE layout_id = ?1 ORDER BY id LIMIT 1",
                [layout_id],
                |r| Ok(TrackRow { id: r.get(0)?, name: r.get(1)?, length_m: r.get(2)? }),
            )
            .optional()
            .map_err(err)
    }

    /// Tie a track to a catalog layout, replacing a placeholder name (a name
    /// the user typed is kept). Returns the track's name.
    fn adopt_layout(&self, track_id: i64, name: &str, layout: &tracks::Layout) -> Result<String> {
        let name = if is_placeholder_name(name) { layout.name.clone() } else { name.to_string() };
        self.conn
            .execute("UPDATE tracks SET layout_id = ?1, name = ?2 WHERE id = ?3", params![layout.id, name, track_id])
            .map_err(err)?;
        Ok(name)
    }

    /// Name tracks recorded before the catalog existed, from their fastest
    /// valid lap, and give a track to sessions that never got one.
    /// Returns how many tracks/sessions were recognised.
    pub fn name_known_tracks(&self) -> Result<usize> {
        let unnamed: Vec<(i64, String, f64)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, name, length_m FROM tracks WHERE layout_id IS NULL")
                .map_err(err)?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(err)?;
            rows.collect::<rusqlite::Result<_>>().map_err(err)?
        };
        let mut named = 0;
        for (id, name, length_m) in unnamed {
            let blob: Option<Vec<u8>> = self
                .conn
                .query_row(
                    "SELECT l.samples FROM laps l JOIN sessions s ON s.id = l.session_id
                     WHERE s.track_id = ?1 AND l.is_valid = 1 ORDER BY l.time_ms LIMIT 1",
                    [id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(err)?;
            let Some(samples) = blob.map(|b| decode(&b)).transpose()? else { continue };
            let Some(layout) = recognise(length_m as f32, &samples) else { continue };
            match self.track_for_layout(&layout.id)? {
                // Already have this layout under another row: fold a
                // placeholder-named duplicate into it.
                Some(existing) if is_placeholder_name(&name) => {
                    self.conn
                        .execute("UPDATE sessions SET track_id = ?1 WHERE track_id = ?2", params![existing.id, id])
                        .map_err(err)?;
                    self.conn.execute("DELETE FROM tracks WHERE id = ?1", [id]).map_err(err)?;
                }
                _ => {
                    self.adopt_layout(id, &name, layout)?;
                }
            }
            named += 1;
        }

        // Sessions that never completed a valid lap were left without a track.
        let orphans: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM sessions WHERE track_id IS NULL")
                .map_err(err)?;
            let rows = stmt.query_map([], |r| r.get(0)).map_err(err)?;
            rows.collect::<rusqlite::Result<_>>().map_err(err)?
        };
        for session_id in orphans {
            let laps: Vec<(f64, Vec<u8>)> = {
                let mut stmt = self
                    .conn
                    .prepare("SELECT length_m, samples FROM laps WHERE session_id = ?1 ORDER BY length_m DESC LIMIT 3")
                    .map_err(err)?;
                let rows = stmt.query_map([session_id], |r| Ok((r.get(0)?, r.get(1)?))).map_err(err)?;
                rows.collect::<rusqlite::Result<_>>().map_err(err)?
            };
            for (length_m, blob) in laps {
                let samples = decode(&blob)?;
                let Some(first) = samples.first() else { continue };
                let start = [first.x, first.y, first.z];
                if let Some(t) = self.recognise_track(start, length_m as f32, &samples)? {
                    self.set_session_track(session_id, t.id)?;
                    named += 1;
                    break;
                }
            }
        }
        Ok(named)
    }

    pub fn set_session_track(&self, session_id: i64, track_id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE sessions SET track_id = ?1 WHERE id = ?2", params![track_id, session_id])
            .map_err(err)?;
        Ok(())
    }

    /// Rename a track. Picking a catalog name also ties the track to that
    /// layout, so later laps there are recognised even if geometry can't be.
    pub fn rename_track(&self, track_id: i64, name: &str) -> Result<()> {
        match tracks::catalog().by_name(name) {
            Some(l) => self.conn.execute(
                "UPDATE tracks SET name = ?1, layout_id = ?2 WHERE id = ?3",
                params![l.name, l.id, track_id],
            ),
            None => self.conn.execute("UPDATE tracks SET name = ?1 WHERE id = ?2", params![name, track_id]),
        }
        .map_err(err)?;
        Ok(())
    }

    pub fn insert_lap(&self, session_id: i64, lap: &CompletedLap) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO laps (session_id, lap_no, time_ms, is_pit, is_valid, sectors, fuel_used, max_speed, length_m, samples, invalid_reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session_id,
                    lap.lap_no,
                    lap.time_ms,
                    lap.is_pit,
                    lap.is_valid,
                    serde_json::to_string(&lap.sectors_ms).map_err(err)?,
                    lap.fuel_used_pct,
                    lap.max_speed,
                    lap.length_m,
                    encode(&lap.samples)?,
                    lap.invalid_reason.map(|r| r.as_str()),
                ],
            )
            .map_err(err)?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Best valid lap for this track + car, used as the live delta reference.
    pub fn best_lap_for(&self, track_id: i64, car_code: i32) -> Result<Option<(u32, Vec<Sample>)>> {
        let row: Option<(i64, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT l.time_ms, l.samples FROM laps l JOIN sessions s ON s.id = l.session_id
                 WHERE s.track_id = ?1 AND s.car_code = ?2 AND l.is_valid = 1
                 ORDER BY l.time_ms ASC LIMIT 1",
                params![track_id, car_code],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(err)?;
        row.map(|(t, blob)| Ok((t as u32, decode(&blob)?))).transpose()
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT s.id, s.track_id, t.name, s.car_code, s.started_at, s.total_laps,
                        COUNT(l.id), MIN(CASE WHEN l.is_valid = 1 THEN l.time_ms END), t.outline,
                        AVG(CASE WHEN l.is_valid = 1 AND l.is_pit = 0 THEN l.time_ms END), s.kind, t.layout_id, s.conditions
                 FROM sessions s
                 LEFT JOIN tracks t ON t.id = s.track_id
                 JOIN laps l ON l.session_id = s.id
                 GROUP BY s.id ORDER BY s.started_at DESC",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                let layout_id: Option<String> = r.get(11)?;
                let layout = layout_id.as_deref().and_then(|id| tracks::catalog().by_id(id));
                Ok(SessionSummary {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    track_name: r.get(2)?,
                    car_code: r.get(3)?,
                    car_name: cars::car_name(r.get(3)?),
                    started_at: r.get(4)?,
                    total_laps: r.get(5)?,
                    lap_count: r.get(6)?,
                    best_ms: r.get(7)?,
                    outline: r
                        .get::<_, Option<String>>(8)?
                        .and_then(|o| serde_json::from_str(&o).ok())
                        .unwrap_or_default(),
                    avg_ms: r.get(9)?,
                    perfect_ms: None,
                    kind: r.get(10)?,
                    conditions: r.get::<_, Option<String>>(12)?.and_then(|c| serde_json::from_str(&c).ok()),
                    venue: layout.map(|l| l.venue.clone()),
                    country: layout.map(|l| l.country.clone()),
                    layout_id,
                })
            })
            .map_err(err)?;
        let mut sessions: Vec<SessionSummary> = rows.collect::<rusqlite::Result<_>>().map_err(err)?;

        let mut sectors: HashMap<i64, Vec<Vec<u32>>> = HashMap::new();
        let mut stmt = self.conn.prepare("SELECT session_id, sectors FROM laps WHERE is_valid = 1").map_err(err)?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
        for row in rows {
            let (id, json) = row.map_err(err)?;
            if let Ok(s) = serde_json::from_str::<Vec<u32>>(&json) {
                sectors.entry(id).or_default().push(s);
            }
        }
        for s in &mut sessions {
            s.perfect_ms = sectors.get(&s.id).and_then(|laps| perfect_lap(laps));
        }
        Ok(sessions)
    }

    pub fn session_laps(&self, session_id: i64) -> Result<Vec<LapSummary>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {LAP_COLS} FROM laps l WHERE l.session_id = ?1 ORDER BY l.lap_no"))
            .map_err(err)?;
        let rows = stmt.query_map([session_id], lap_row).map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)
    }

    /// Valid laps with the same car on the same track (any session), fastest
    /// first, for comparison. Another car's lap isn't a fair reference.
    pub fn track_laps(&self, track_id: i64, car_code: i32, limit: i64) -> Result<Vec<LapSummary>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {LAP_COLS} FROM laps l JOIN sessions s ON s.id = l.session_id
                 WHERE s.track_id = ?1 AND s.car_code = ?2 AND l.is_valid = 1 ORDER BY l.time_ms LIMIT ?3"
            ))
            .map_err(err)?;
        let rows = stmt.query_map(params![track_id, car_code, limit], lap_row).map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)
    }

    pub fn lap_detail(&self, lap_id: i64) -> Result<LapDetail> {
        self.conn
            .query_row(
                &format!(
                    "SELECT {LAP_COLS}, s.car_code, s.track_id, t.name, l.samples
                     FROM laps l JOIN sessions s ON s.id = l.session_id
                     LEFT JOIN tracks t ON t.id = s.track_id WHERE l.id = ?1"
                ),
                [lap_id],
                |r| {
                    let blob: Vec<u8> = r.get(13)?;
                    Ok((lap_row(r)?, r.get(10)?, r.get(11)?, r.get(12)?, blob))
                },
            )
            .map_err(err)
            .and_then(|(summary, car_code, track_id, track_name, blob)| {
                Ok(LapDetail {
                    summary,
                    car_code,
                    car_name: cars::car_name(car_code),
                    track_id,
                    track_name,
                    samples: decode(&blob)?,
                })
            })
    }

    /// Note the race settings a session was run with (None clears them).
    pub fn set_session_conditions(&self, session_id: i64, c: Option<&SessionConditions>) -> Result<()> {
        if let Some(m) = c.and_then(|c| c.mode.as_deref()) {
            if !["offline", "online"].contains(&m) {
                return Err(format!("unknown race mode {m:?}"));
            }
        }
        let json = c.filter(|c| **c != SessionConditions::default()).map(serde_json::to_string).transpose().map_err(err)?;
        self.conn
            .execute("UPDATE sessions SET conditions = ?1 WHERE id = ?2", params![json, session_id])
            .map_err(err)?;
        Ok(())
    }

    pub fn session_conditions(&self, session_id: i64) -> Result<Option<SessionConditions>> {
        let json: Option<String> = self
            .conn
            .query_row("SELECT conditions FROM sessions WHERE id = ?1", params![session_id], |r| r.get(0))
            .optional()
            .map_err(err)?
            .flatten();
        Ok(json.and_then(|c| serde_json::from_str(&c).ok()))
    }

    /// Conditions of the latest earlier session that looks like the same
    /// event: same track, car and race distance.
    pub fn previous_conditions(&self, session_id: i64, track_id: i64, car_code: i32, total_laps: i32) -> Result<Option<SessionConditions>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT conditions FROM sessions
                 WHERE track_id = ?1 AND car_code = ?2 AND total_laps = ?3 AND id != ?4 AND conditions IS NOT NULL
                 ORDER BY started_at DESC LIMIT 1",
                params![track_id, car_code, total_laps, session_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        Ok(json.and_then(|c| serde_json::from_str(&c).ok()))
    }

    /// Tyre fade per unit of wear multiplier (ms per lap) for this track and
    /// car, from earlier sessions with tyre wear noted: the median of those
    /// with a clear trend.
    pub fn tyre_fade_per_wear(&self, session_id: i64, track_id: i64, car_code: i32) -> Result<Option<f32>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, conditions FROM sessions WHERE track_id = ?1 AND car_code = ?2 AND id != ?3 AND conditions IS NOT NULL")
            .map_err(err)?;
        let sessions: Vec<(i64, String)> = stmt
            .query_map(params![track_id, car_code, session_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(err)?
            .collect::<rusqlite::Result<_>>()
            .map_err(err)?;
        let mut rates = vec![];
        for (id, json) in sessions {
            let Some(wear) = serde_json::from_str::<SessionConditions>(&json).ok().and_then(|c| c.tyre_wear) else { continue };
            let laps: Vec<_> = self
                .session_laps(id)?
                .iter()
                .map(|l| (l.lap_no as i16, l.time_ms as u32, l.is_valid, l.is_pit, l.fuel_used_pct.map(|f| f as f32)))
                .collect();
            rates.extend(fade_per_wear(&laps, wear));
        }
        rates.sort_by(|a, b| a.total_cmp(b));
        Ok(rates.get(rates.len() / 2).copied())
    }

    /// Tag a session as practice, qualifying, race or time trial (None = infer).
    pub fn set_session_kind(&self, session_id: i64, kind: Option<&str>) -> Result<()> {
        if let Some(k) = kind {
            if !SESSION_KINDS.contains(&k) {
                return Err(format!("unknown session type {k:?}"));
            }
        }
        self.conn
            .execute("UPDATE sessions SET kind = ?1 WHERE id = ?2", params![kind, session_id])
            .map_err(err)?;
        Ok(())
    }

    /// Marks an already stored lap as not counting (see `EngineEvent::LapRevoked`).
    pub fn revoke_lap(&self, session_id: i64, lap_no: i16, reason: InvalidReason) -> Result<()> {
        self.conn
            .execute(
                "UPDATE laps SET is_valid = 0, invalid_reason = ?3 WHERE session_id = ?1 AND lap_no = ?2",
                params![session_id, lap_no, reason.as_str()],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Repairs laps saved before grid laps were checked: a valid first lap
    /// clearly shorter than the session's next valid lap started past the
    /// line, so it doesn't count. Returns how many laps were revoked.
    pub fn revoke_short_grid_laps(&self) -> Result<usize> {
        self.conn
            .execute(
                "UPDATE laps SET is_valid = 0, invalid_reason = ?2
                 WHERE lap_no = 1 AND is_valid = 1 AND length_m < ?1 * (
                    SELECT n.length_m FROM laps n
                    WHERE n.session_id = laps.session_id AND n.lap_no > 1 AND n.is_valid = 1
                    ORDER BY n.lap_no LIMIT 1)",
                params![crate::session::engine::GRID_LAP_MIN_FRACTION as f64, InvalidReason::InsufficientDistance.as_str()],
            )
            .map_err(err)
    }

    pub fn delete_session(&self, session_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id]).map_err(err)?;
        Ok(())
    }
}

fn recognise(length_m: f32, samples: &[Sample]) -> Option<&'static tracks::Layout> {
    let line: Vec<[f32; 2]> = samples.iter().map(|s| [s.x, s.z]).collect();
    tracks::catalog().identify(&line, length_m)
}

fn placeholder_name(n: i64, length_m: f32) -> String {
    format!("Track {n} ({:.2} km)", length_m / 1000.0)
}

/// Names like "Track 3 (3.70 km)" were made up by the app, not typed by the user.
fn is_placeholder_name(name: &str) -> bool {
    name.strip_prefix("Track ")
        .and_then(|rest| rest.split_once(" ("))
        .is_some_and(|(n, km)| n.parse::<i64>().is_ok() && km.ends_with(" km)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_lap_takes_the_best_of_each_sector() {
        let laps = vec![vec![13_400, 12_100, 12_600], vec![13_300, 12_300, 12_500], vec![13_500, 12_200]];
        assert_eq!(perfect_lap(&laps), Some(13_300 + 12_100 + 12_500));
        // A sector nobody recorded: no perfect lap.
        assert_eq!(perfect_lap(&[vec![13_000, 0]]), None);
        assert_eq!(perfect_lap(&[]), None);
    }

    fn lap(time_ms: u32, valid: bool) -> CompletedLap {
        CompletedLap {
            lap_no: 1,
            time_ms,
            is_pit: false,
            is_valid: valid,
            invalid_reason: (!valid).then_some(crate::session::models::InvalidReason::RewindOrReset),
            sectors_ms: vec![1, 2, 3, 4, 5],
            fuel_used_pct: Some(5.0),
            max_speed: 250.0,
            length_m: 3000.0,
            start_pos: [100.0, 0.0, 200.0],
            samples: vec![Sample { t: 10, d: 5.0, spd: 100.0, ..Default::default() }; 50],
        }
    }

    #[test]
    fn session_lap_round_trip() {
        let db = Db::memory().unwrap();
        let s = db.create_session(1234, 5, 1).unwrap();
        let t = db.identify_track([100.0, 0.0, 200.0], 3000.0, &[]).unwrap();
        assert_eq!(db.identify_track([110.0, 0.0, 195.0], 3020.0, &[]).unwrap().id, t.id);
        assert_ne!(db.identify_track([110.0, 0.0, 195.0], 5000.0, &[]).unwrap().id, t.id);
        db.set_session_track(s, t.id).unwrap();
        db.insert_lap(s, &lap(95_000, true)).unwrap();
        let id = db.insert_lap(s, &lap(90_000, false)).unwrap();

        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions[0].lap_count, 2);
        assert_eq!(sessions[0].best_ms, Some(95_000));
        assert_eq!(sessions[0].avg_ms, Some(95_000.0));
        assert_eq!(sessions[0].kind, None);
        db.set_session_kind(s, Some("qualifying")).unwrap();
        assert_eq!(db.list_sessions().unwrap()[0].kind.as_deref(), Some("qualifying"));
        assert!(db.set_session_kind(s, Some("warmup")).is_err());
        db.set_session_kind(s, None).unwrap();
        assert_eq!(db.list_sessions().unwrap()[0].kind, None);
        let c = SessionConditions { mode: Some("online".into()), bop: Some(true), tyre_wear: Some(3), fuel_rate: Some(2) };
        db.set_session_conditions(s, Some(&c)).unwrap();
        assert_eq!(db.list_sessions().unwrap()[0].conditions, Some(c.clone()));
        assert!(db.set_session_conditions(s, Some(&SessionConditions { mode: Some("split".into()), ..Default::default() })).is_err());
        assert_eq!(db.session_conditions(s).unwrap(), Some(c.clone()));
        let next = db.create_session(1234, 5, 2).unwrap();
        assert_eq!(db.previous_conditions(next, t.id, 1234, 5).unwrap(), Some(c));
        assert_eq!(db.previous_conditions(next, t.id, 1234, 10).unwrap(), None);
        db.set_session_conditions(s, Some(&SessionConditions::default())).unwrap();
        assert_eq!(db.list_sessions().unwrap()[0].conditions, None);
        assert_eq!(db.session_conditions(s).unwrap(), None);
        let detail = db.lap_detail(id).unwrap();
        assert_eq!(detail.summary.invalid_reason.as_deref(), Some("rewind_or_reset"));
        assert_eq!(detail.samples.len(), 50);
        assert_eq!(detail.summary.sectors_ms, vec![1, 2, 3, 4, 5]);
        assert_eq!(db.best_lap_for(t.id, 1234).unwrap().unwrap().0, 95_000);
        // Comparison laps: the same car on the same track only.
        let other_car = db.create_session(9999, 5, 3).unwrap();
        db.set_session_track(other_car, t.id).unwrap();
        db.insert_lap(other_car, &lap(80_000, true)).unwrap();
        let ids = |car| db.track_laps(t.id, car, 20).unwrap().iter().map(|l| l.time_ms).collect::<Vec<_>>();
        assert_eq!(ids(1234), [95_000]);
        assert_eq!(ids(9999), [80_000]);
        db.delete_session(other_car).unwrap();
        db.delete_session(s).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn tyre_fade_is_learned_per_wear_multiplier() {
        let db = Db::memory().unwrap();
        let t = db.identify_track([100.0, 0.0, 200.0], 3000.0, &[]).unwrap();
        let s = db.create_session(7, 8, 1).unwrap();
        db.set_session_track(s, t.id).unwrap();
        for i in 0..8 {
            let mut l = lap(if i == 0 { 95_000 } else { 90_000 + i * 400 }, true);
            l.lap_no = i as i16 + 1;
            l.fuel_used_pct = Some(0.0);
            db.insert_lap(s, &l).unwrap();
        }
        let live = db.create_session(7, 8, 2).unwrap();
        assert_eq!(db.tyre_fade_per_wear(live, t.id, 7).unwrap(), None);
        db.set_session_conditions(s, Some(&SessionConditions { tyre_wear: Some(4), ..Default::default() })).unwrap();
        let f = db.tyre_fade_per_wear(live, t.id, 7).unwrap().unwrap();
        assert!((f - 100.0).abs() < 2.0, "{f}");
        assert_eq!(db.tyre_fade_per_wear(live, t.id, 8).unwrap(), None);
    }

    #[test]
    fn short_grid_laps_are_revoked() {
        let db = Db::memory().unwrap();
        let mut add = |laps: &[(u32, f32)]| {
            let s = db.create_session(1, laps.len() as i32, 1).unwrap();
            for (i, (time_ms, length_m)) in laps.iter().enumerate() {
                db.insert_lap(s, &CompletedLap { lap_no: i as i16 + 1, length_m: *length_m, ..lap(*time_ms, true) }).unwrap();
            }
            s
        };
        let short = add(&[(85_000, 4225.0), (88_600, 4530.0)]);
        let full = add(&[(86_000, 4500.0), (88_600, 4530.0)]);
        assert_eq!(db.revoke_short_grid_laps().unwrap(), 1);
        let first = |s| db.session_laps(s).unwrap().remove(0);
        assert!(!first(short).is_valid);
        assert_eq!(first(short).invalid_reason.as_deref(), Some("insufficient_distance"));
        assert!(first(full).is_valid);
        assert_eq!(db.list_sessions().unwrap().iter().find(|x| x.id == short).unwrap().best_ms, Some(88_600));
        assert_eq!(db.revoke_short_grid_laps().unwrap(), 0);
    }

    fn samples_on(line: &[[f32; 2]]) -> Vec<Sample> {
        line.iter().step_by(2).map(|p| Sample { x: p[0], z: p[1], ..Default::default() }).collect()
    }

    #[test]
    fn catalog_tracks_get_real_names() {
        let db = Db::memory().unwrap();
        let line = tracks::catalog().reference_line("Alsace - Village");
        let start = [line[0][0], 0.0, line[0][1]];
        let t = db.identify_track(start, 0.0, &samples_on(&line)).unwrap();
        assert_eq!(t.name, "Alsace - Village");
        // Same start line and length, other way round: a different track.
        let rev: Vec<_> = line.iter().rev().copied().collect();
        let r = db.identify_track(start, 0.0, &samples_on(&rev)).unwrap();
        assert_eq!(r.name, "Alsace - Village (Reverse)");
        assert_ne!(r.id, t.id);
        assert_eq!(db.identify_track(start, 0.0, &samples_on(&line)).unwrap().id, t.id);

        let s = db.create_session(3442, 5, 1).unwrap();
        db.set_session_track(s, t.id).unwrap();
        db.insert_lap(s, &lap(95_000, true)).unwrap();
        let summary = &db.list_sessions().unwrap()[0];
        assert_eq!(summary.track_name.as_deref(), Some("Alsace - Village"));
        assert_eq!(summary.car_name, Some("Autobianchi A112 Abarth '85"));
    }

    #[test]
    fn old_placeholder_tracks_are_renamed_from_their_laps() {
        let db = Db::memory().unwrap();
        // A track saved before the catalog: placeholder name, no layout.
        db.conn
            .execute(
                "INSERT INTO tracks (name, sf_x, sf_y, sf_z, length_m) VALUES ('Track 1 (5.67 km)', 0, 0, 0, 5670)",
                [],
            )
            .unwrap();
        let track_id = db.conn.last_insert_rowid();
        let s = db.create_session(24, 3, 1).unwrap();
        db.set_session_track(s, track_id).unwrap();
        let mut l = lap(100_000, true);
        l.samples = samples_on(&tracks::catalog().reference_line("Autodromo Nazionale Monza No Chicane"));
        db.insert_lap(s, &l).unwrap();

        assert_eq!(db.name_known_tracks().unwrap(), 1);
        let summary = &db.list_sessions().unwrap()[0];
        assert_eq!(summary.track_name.as_deref(), Some("Autodromo Nazionale Monza No Chicane"));
        assert_eq!(summary.car_name, Some("Nissan 180SX Type X '96"));
        // Nothing left to do on the next launch.
        assert_eq!(db.name_known_tracks().unwrap(), 0);
    }

    #[test]
    fn sessions_without_a_valid_lap_still_get_a_known_track() {
        let db = Db::memory().unwrap();
        let s = db.create_session(24, 3, 1).unwrap();
        let mut l = lap(100_000, false);
        l.samples = samples_on(&tracks::catalog().reference_line("Suzuka Circuit"));
        l.length_m = 0.0;
        db.insert_lap(s, &l).unwrap();
        assert_eq!(db.list_sessions().unwrap()[0].track_name, None);

        assert_eq!(db.name_known_tracks().unwrap(), 1);
        assert_eq!(db.list_sessions().unwrap()[0].track_name.as_deref(), Some("Suzuka Circuit"));
        // An unrecognisable lap is left alone rather than given a made-up track.
        let s2 = db.create_session(24, 3, 2).unwrap();
        db.insert_lap(s2, &lap(100_000, false)).unwrap();
        db.name_known_tracks().unwrap();
        assert!(db.list_sessions().unwrap().iter().any(|x| x.id == s2 && x.track_id.is_none()));
    }

    #[test]
    fn user_names_are_kept_and_catalog_names_link_the_layout() {
        let db = Db::memory().unwrap();
        let t = db.identify_track([1.0, 0.0, 2.0], 3000.0, &[]).unwrap();
        assert_eq!(t.name, "Track 1 (3.00 km)");
        assert!(is_placeholder_name(&t.name));
        assert!(!is_placeholder_name("Track day at Fuji"));
        db.rename_track(t.id, "suzuka circuit").unwrap();
        let layout: Option<String> =
            db.conn.query_row("SELECT layout_id FROM tracks WHERE id = ?1", [t.id], |r| r.get(0)).unwrap();
        assert_eq!(layout.as_deref(), Some(tracks::catalog().by_name("Suzuka Circuit").unwrap().id.as_str()));
        let name: String = db.conn.query_row("SELECT name FROM tracks WHERE id = ?1", [t.id], |r| r.get(0)).unwrap();
        assert_eq!(name, "Suzuka Circuit");
        db.rename_track(t.id, "My home track").unwrap();
        let name: String = db.conn.query_row("SELECT name FROM tracks WHERE id = ?1", [t.id], |r| r.get(0)).unwrap();
        assert_eq!(name, "My home track");
    }
}
