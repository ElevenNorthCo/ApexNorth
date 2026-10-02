//! SQLite persistence for tracks, sessions and recorded laps.
//!
//! Lap samples are stored as a zstd-compressed bincode blob.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;

use crate::session::models::{CompletedLap, Sample};

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
    pub started_at: i64,
    pub total_laps: i32,
    pub lap_count: i64,
    pub best_ms: Option<i64>,
    /// Track shape as [x, z] points, for thumbnails.
    pub outline: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LapSummary {
    pub id: i64,
    pub session_id: i64,
    pub lap_no: i32,
    pub time_ms: i64,
    pub is_pit: bool,
    pub is_valid: bool,
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

const LAP_COLS: &str = "l.id, l.session_id, l.lap_no, l.time_ms, l.is_pit, l.is_valid, l.sectors, l.fuel_used, l.max_speed";

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

    /// GT7 does not report the track, so tracks are fingerprinted by the
    /// start/finish position and the lap length.
    pub fn identify_track(&self, start: [f32; 3], length_m: f32, samples: &[Sample]) -> Result<TrackRow> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, length_m, sf_x, sf_y, sf_z FROM tracks")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    TrackRow { id: r.get(0)?, name: r.get(1)?, length_m: r.get(2)? },
                    [r.get::<_, f64>(3)?, r.get::<_, f64>(4)?, r.get::<_, f64>(5)?],
                ))
            })
            .map_err(err)?;
        for row in rows {
            let (track, sf) = row.map_err(err)?;
            let d = ((sf[0] - start[0] as f64).powi(2) + (sf[2] - start[2] as f64).powi(2)).sqrt();
            let len_ratio = (track.length_m - length_m as f64).abs() / track.length_m.max(1.0);
            if d < 60.0 && len_ratio < 0.04 {
                return Ok(track);
            }
        }
        let count: i64 = self.conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).map_err(err)?;
        let name = format!("Track {} ({:.2} km)", count + 1, length_m / 1000.0);
        let step = (samples.len() / 200).max(1);
        let outline: Vec<[f32; 2]> = samples.iter().step_by(step).map(|s| [s.x, s.z]).collect();
        self.conn
            .execute(
                "INSERT INTO tracks (name, sf_x, sf_y, sf_z, length_m, outline) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![name, start[0], start[1], start[2], length_m, serde_json::to_string(&outline).map_err(err)?],
            )
            .map_err(err)?;
        Ok(TrackRow { id: self.conn.last_insert_rowid(), name, length_m: length_m as f64 })
    }

    pub fn set_session_track(&self, session_id: i64, track_id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE sessions SET track_id = ?1 WHERE id = ?2", params![track_id, session_id])
            .map_err(err)?;
        Ok(())
    }

    pub fn rename_track(&self, track_id: i64, name: &str) -> Result<()> {
        self.conn
            .execute("UPDATE tracks SET name = ?1 WHERE id = ?2", params![name, track_id])
            .map_err(err)?;
        Ok(())
    }

    pub fn insert_lap(&self, session_id: i64, lap: &CompletedLap) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO laps (session_id, lap_no, time_ms, is_pit, is_valid, sectors, fuel_used, max_speed, length_m, samples)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                        COUNT(l.id), MIN(CASE WHEN l.is_valid = 1 THEN l.time_ms END), t.outline
                 FROM sessions s
                 LEFT JOIN tracks t ON t.id = s.track_id
                 JOIN laps l ON l.session_id = s.id
                 GROUP BY s.id ORDER BY s.started_at DESC",
            )
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SessionSummary {
                    id: r.get(0)?,
                    track_id: r.get(1)?,
                    track_name: r.get(2)?,
                    car_code: r.get(3)?,
                    started_at: r.get(4)?,
                    total_laps: r.get(5)?,
                    lap_count: r.get(6)?,
                    best_ms: r.get(7)?,
                    outline: r
                        .get::<_, Option<String>>(8)?
                        .and_then(|o| serde_json::from_str(&o).ok())
                        .unwrap_or_default(),
                })
            })
            .map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)
    }

    pub fn session_laps(&self, session_id: i64) -> Result<Vec<LapSummary>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {LAP_COLS} FROM laps l WHERE l.session_id = ?1 ORDER BY l.lap_no"))
            .map_err(err)?;
        let rows = stmt.query_map([session_id], lap_row).map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)
    }

    /// Valid laps on the same track (any session), fastest first, for comparison.
    pub fn track_laps(&self, track_id: i64, limit: i64) -> Result<Vec<LapSummary>> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {LAP_COLS} FROM laps l JOIN sessions s ON s.id = l.session_id
                 WHERE s.track_id = ?1 AND l.is_valid = 1 ORDER BY l.time_ms LIMIT ?2"
            ))
            .map_err(err)?;
        let rows = stmt.query_map(params![track_id, limit], lap_row).map_err(err)?;
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
                    let blob: Vec<u8> = r.get(12)?;
                    Ok((lap_row(r)?, r.get(9)?, r.get(10)?, r.get(11)?, blob))
                },
            )
            .map_err(err)
            .and_then(|(summary, car_code, track_id, track_name, blob)| {
                Ok(LapDetail { summary, car_code, track_id, track_name, samples: decode(&blob)? })
            })
    }

    pub fn delete_session(&self, session_id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id]).map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lap(time_ms: u32, valid: bool) -> CompletedLap {
        CompletedLap {
            lap_no: 1,
            time_ms,
            is_pit: false,
            is_valid: valid,
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
        let detail = db.lap_detail(id).unwrap();
        assert_eq!(detail.samples.len(), 50);
        assert_eq!(detail.summary.sectors_ms, vec![1, 2, 3, 4, 5]);
        assert_eq!(db.best_lap_for(t.id, 1234).unwrap().unwrap().0, 95_000);
        db.delete_session(s).unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
    }
}
