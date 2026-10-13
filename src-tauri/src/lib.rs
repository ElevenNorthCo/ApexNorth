mod catalog;
mod engineer;
mod session;
mod settings;
mod storage;
mod telemetry;
mod updater;
mod voices;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

use engineer::director::EngineerConfig;
use engineer::rules::Engineer;
use engineer::voice::{Callout, Priority, Voice};
use session::engine::{Engine, EngineEvent, RefLap};
use settings::Settings;
use storage::{Db, LapDetail, LapSummary, SessionSummary};
use telemetry::packet::Packet;
use telemetry::sim::Simulator;
use telemetry::udp::{ConnState, ConnStatus, Handler, Link};

const FRAME_INTERVAL: Duration = Duration::from_millis(33);

struct AppState {
    db: Arc<Mutex<Db>>,
    settings: Arc<RwLock<Settings>>,
    settings_path: PathBuf,
    voices_dir: PathBuf,
    /// Bundled voice engine (`lib/`, `espeak-ng-data/`, `kokoro/`), when present.
    builtin_tts: Option<PathBuf>,
    voice: Voice,
    status: Arc<Mutex<ConnStatus>>,
    link: Mutex<Option<Link>>,
    sim: Mutex<Option<Simulator>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LapEvent {
    session_id: i64,
    lap: LapSummary,
    track_name: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LapInvalid {
    lap_no: i16,
    reason: &'static str,
}

/// Receives packets on the link thread: runs the engine, records laps,
/// drives the engineer and pushes frames to the UI.
struct LiveHandler {
    app: AppHandle,
    db: Arc<Mutex<Db>>,
    settings: Arc<RwLock<Settings>>,
    settings_path: PathBuf,
    status: Arc<Mutex<ConnStatus>>,
    voice: Voice,
    engine: Engine,
    engineer: Engineer,
    session: Option<SessionCtx>,
    last_emit: Instant,
    awake: Option<keepawake::KeepAwake>,
}

struct SessionCtx {
    car_code: i32,
    total_laps: i16,
    started_at: i64,
    db_id: Option<i64>,
    track: Option<(i64, String)>,
    /// Race settings given to the engine, and the tyre fade per wear unit
    /// learned from earlier sessions here.
    conditions: Option<storage::SessionConditions>,
    fade_per_wear: Option<f32>,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

impl LiveHandler {
    fn speak(&self, callouts: Vec<Callout>) {
        for c in callouts {
            let _ = self.app.emit("engineer", &c.text);
            self.voice.say(c);
        }
    }

    fn record_lap(&mut self, lap: &session::models::CompletedLap) {
        let Some(ctx) = self.session.as_mut() else { return };
        let db = self.db.lock();
        let session_id = match ctx.db_id {
            Some(id) => id,
            None => match db.create_session(ctx.car_code, ctx.total_laps as i32, ctx.started_at) {
                Ok(id) => *ctx.db_id.insert(id),
                Err(e) => {
                    log::error!("create session: {e}");
                    return;
                }
            },
        };
        if ctx.track.is_none() {
            // A valid lap names the track (from the catalog, or a placeholder);
            // any other lap only when the catalog recognises it.
            let found = if lap.is_valid {
                db.identify_track(lap.start_pos, lap.length_m, &lap.samples).map(Some)
            } else {
                db.recognise_track(lap.start_pos, lap.length_m, &lap.samples)
            };
            if let Ok(Some(t)) = found {
                let _ = db.set_session_track(session_id, t.id);
                if let Ok(Some((time_ms, samples))) = db.best_lap_for(t.id, ctx.car_code) {
                    self.engine.set_reference(RefLap { time_ms, samples, standing_start: false });
                }
                // Same track, car and distance as an earlier race: assume it
                // was run the same way until the driver says otherwise.
                if let (Ok(None), Ok(Some(prev))) = (
                    db.session_conditions(session_id),
                    db.previous_conditions(session_id, t.id, ctx.car_code, ctx.total_laps as i32),
                ) {
                    let _ = db.set_session_conditions(session_id, Some(&prev));
                }
                ctx.fade_per_wear = db.tyre_fade_per_wear(session_id, t.id, ctx.car_code).unwrap_or(None);
                ctx.track = Some((t.id, t.name));
            }
        }
        // Conditions can be noted (or changed) on the Session page mid-race.
        let conditions = db.session_conditions(session_id).unwrap_or(None);
        if conditions != ctx.conditions {
            let wear = conditions.as_ref().and_then(|c| c.tyre_wear);
            let fuel_off = conditions.as_ref().and_then(|c| c.fuel_rate) == Some(0);
            self.engine.set_conditions(wear, ctx.fade_per_wear.zip(wear).map(|(f, w)| f * w as f32), fuel_off);
            ctx.conditions = conditions;
        }
        match db.insert_lap(session_id, lap) {
            Ok(id) => {
                let summary = LapSummary {
                    id,
                    session_id,
                    lap_no: lap.lap_no as i32,
                    time_ms: lap.time_ms as i64,
                    is_pit: lap.is_pit,
                    is_valid: lap.is_valid,
                    invalid_reason: lap.invalid_reason.map(|r| r.as_str().to_string()),
                    sectors_ms: lap.sectors_ms.clone(),
                    fuel_used_pct: lap.fuel_used_pct.map(|f| f as f64),
                    max_speed: lap.max_speed as f64,
                };
                let _ = self.app.emit(
                    "lap",
                    LapEvent { session_id, lap: summary, track_name: ctx.track.as_ref().map(|t| t.1.clone()) },
                );
            }
            Err(e) => log::error!("insert lap: {e}"),
        }
    }
}

impl Handler for LiveHandler {
    fn packet(&mut self, p: Packet) {
        let cfg = {
            let s = self.settings.read();
            EngineerConfig {
                chatter: s.voice.chatter,
                callouts: s.voice.callouts.clone(),
                driver: Some(s.driver_name.trim()).filter(|n| !n.is_empty()).map(str::to_string),
            }
        };
        let now = Instant::now();
        for ev in self.engine.on_packet(&p) {
            match &ev {
                EngineEvent::SessionStarted { car_code, total_laps } => {
                    self.voice.clear();
                    self.session = Some(SessionCtx {
                        car_code: *car_code,
                        total_laps: *total_laps,
                        started_at: now_ms(),
                        db_id: None,
                        track: None,
                        conditions: None,
                        fade_per_wear: None,
                    });
                }
                EngineEvent::SessionEnded => {
                    self.voice.clear();
                    self.session = None;
                }
                EngineEvent::LapCompleted(lap, _) => self.record_lap(lap),
                EngineEvent::LapRevoked { lap_no, reason } => {
                    if let Some(id) = self.session.as_ref().and_then(|c| c.db_id) {
                        if let Err(e) = self.db.lock().revoke_lap(id, *lap_no, *reason) {
                            log::error!("revoke lap: {e}");
                        }
                    }
                }
                EngineEvent::LapInvalidated { lap_no, reason } => {
                    let _ = self.app.emit("lap_invalid", LapInvalid { lap_no: *lap_no, reason: reason.as_str() });
                }
                _ => {}
            }
            let c = self.engineer.on_event(&ev, &cfg, now);
            self.voice.update_position(self.engine.live().position);
            self.speak(c);
        }
        self.voice.update_position(self.engine.live().position);
        let c = self.engineer.on_live(self.engine.live(), &cfg, now);
        self.speak(c);

        if self.last_emit.elapsed() >= FRAME_INTERVAL {
            self.last_emit = Instant::now();
            let _ = self.app.emit("telemetry", self.engine.live());
        }
    }

    fn status(&mut self, s: &ConnStatus) {
        // The link reports its state every second; only a drop out of Live
        // ends what the engineer was saying.
        let was_live = std::mem::replace(&mut *self.status.lock(), s.clone()).state == ConnState::Live;
        let _ = self.app.emit("connection", s);
        if s.state == ConnState::Live {
            // Remember the console so the next launch connects straight away.
            if let Some(ip) = s.ip.as_ref().filter(|ip| ip.as_str() != "127.0.0.1") {
                let mut cfg = self.settings.write();
                if cfg.last_ip.as_ref() != Some(ip) {
                    cfg.last_ip = Some(ip.clone());
                    let _ = cfg.save(&self.settings_path);
                }
            }
            if self.awake.is_none() {
                self.awake = keepawake::Builder::default()
                    .display(true)
                    .idle(true)
                    .reason("Live telemetry")
                    .app_name("ApexNorth")
                    .app_reverse_domain("com.elevennorth.apexnorth")
                    .create()
                    .ok();
            }
        } else {
            if was_live {
                self.voice.clear();
            }
            self.awake = None;
        }
    }
}

fn start_link(app: &AppHandle, state: &AppState, ip: Option<IpAddr>) {
    // Stop the old link first so it releases the UDP port.
    drop(state.link.lock().take());
    state.voice.clear();
    let handler = LiveHandler {
        app: app.clone(),
        db: state.db.clone(),
        settings: state.settings.clone(),
        settings_path: state.settings_path.clone(),
        status: state.status.clone(),
        voice: state.voice.clone(),
        engine: Engine::new(),
        engineer: Engineer::new(),
        session: None,
        last_emit: Instant::now(),
        awake: None,
    };
    *state.link.lock() = Some(Link::start(ip, handler));
}

fn parse_ip(ip: &str) -> Result<IpAddr, String> {
    ip.trim().parse().map_err(|_| format!("'{ip}' is not a valid IP address"))
}

#[tauri::command]
fn connect(app: AppHandle, state: State<AppState>, ip: Option<String>) -> Result<(), String> {
    let ip = ip.filter(|s| !s.trim().is_empty()).map(|s| parse_ip(&s)).transpose()?;
    drop(state.sim.lock().take());
    start_link(&app, &state, ip);
    Ok(())
}

#[tauri::command]
fn start_demo(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    drop(state.link.lock().take());
    let mut sim = state.sim.lock();
    if sim.is_none() {
        *sim = Some(Simulator::start().map_err(|e| format!("Could not start demo: {e}"))?);
    }
    drop(sim);
    start_link(&app, &state, Some(IpAddr::from([127, 0, 0, 1])));
    Ok(())
}

#[tauri::command]
fn disconnect(state: State<AppState>) {
    drop(state.link.lock().take());
    drop(state.sim.lock().take());
    *state.status.lock() = ConnStatus::idle();
}

#[tauri::command]
fn get_status(state: State<AppState>) -> ConnStatus {
    state.status.lock().clone()
}

#[tauri::command]
fn demo_running(state: State<AppState>) -> bool {
    state.sim.lock().is_some()
}

#[tauri::command]
fn local_ips() -> Vec<String> {
    local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, ip)| match ip {
            IpAddr::V4(v4) if v4.is_private() => Some(v4.to_string()),
            _ => None,
        })
        .collect()
}

#[tauri::command]
fn list_sessions(state: State<AppState>) -> Result<Vec<SessionSummary>, String> {
    state.db.lock().list_sessions()
}

#[tauri::command]
fn session_laps(state: State<AppState>, session_id: i64) -> Result<Vec<LapSummary>, String> {
    state.db.lock().session_laps(session_id)
}

#[tauri::command]
fn track_laps(state: State<AppState>, track_id: i64, car_code: i32) -> Result<Vec<LapSummary>, String> {
    state.db.lock().track_laps(track_id, car_code, 20)
}

#[tauri::command]
async fn get_lap(state: State<'_, AppState>, lap_id: i64) -> Result<LapDetail, String> {
    state.db.lock().lap_detail(lap_id)
}

#[tauri::command]
fn delete_session(state: State<AppState>, session_id: i64) -> Result<(), String> {
    state.db.lock().delete_session(session_id)
}

#[tauri::command]
fn set_session_kind(state: State<AppState>, session_id: i64, kind: Option<String>) -> Result<(), String> {
    state.db.lock().set_session_kind(session_id, kind.as_deref())
}

#[tauri::command]
fn set_session_conditions(state: State<AppState>, session_id: i64, conditions: Option<storage::SessionConditions>) -> Result<(), String> {
    state.db.lock().set_session_conditions(session_id, conditions.as_ref())
}

#[tauri::command]
fn rename_track(state: State<AppState>, track_id: i64, name: String) -> Result<(), String> {
    state.db.lock().rename_track(track_id, name.trim())
}

/// Every GT7 track layout, for picking a name when a track isn't recognised.
#[tauri::command]
fn track_catalog() -> Vec<&'static catalog::tracks::Layout> {
    catalog::tracks::catalog().layouts().iter().collect()
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.read().clone()
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    settings.save(&state.settings_path)?;
    state.voice.configure(settings.voice.clone());
    *state.settings.write() = settings;
    Ok(())
}

/// Voices to choose from: bundled Kokoro speakers, then voice files in the
/// voices folder (and next to the current one), then Piper voices.
#[tauri::command]
fn list_voices(state: State<AppState>) -> Vec<voices::VoiceOption> {
    let mut dirs = vec![state.voices_dir.clone()];
    let current = state.settings.read().voice.model_path.clone();
    if let Some(dir) = PathBuf::from(&current).parent().filter(|d| !d.as_os_str().is_empty()) {
        dirs.push(dir.to_path_buf());
    }
    voices::options(&dirs, state.builtin_tts.as_deref())
}

#[tauri::command]
fn voices_dir(state: State<AppState>) -> String {
    state.voices_dir.display().to_string()
}

#[tauri::command]
async fn test_voice(state: State<'_, AppState>, text: String) -> Result<(), String> {
    let v = state.voice.clone();
    let t = text.clone();
    tauri::async_runtime::spawn_blocking(move || v.render(t)).await.map_err(|e| e.to_string())??;
    // A test phrase isn't about a session, so nothing that clears the
    // engineer's calls (connecting, a session starting) cuts it off.
    let mut c = Callout::new(Priority::High, text);
    c.outlives_session = true;
    state.voice.say(c);
    Ok(())
}

/// Whether the app ships its own voice engine (no Piper install needed).
#[tauri::command]
fn voice_builtin(state: State<AppState>) -> bool {
    state.builtin_tts.is_some()
}

/// Which update channel (stable or beta) and build this app is.
#[tauri::command]
fn build_info() -> updater::BuildInfo {
    updater::build_info()
}

/// A newer signed release for this channel (stable or beta), if there is one.
#[tauri::command]
async fn check_update(app: AppHandle, pending: State<'_, updater::Pending>) -> Result<Option<updater::UpdateInfo>, String> {
    updater::check(&app, &pending).await
}

/// Download the update found by `check_update`, reporting progress, then
/// install it and restart.
#[tauri::command]
async fn install_update(
    app: AppHandle,
    pending: State<'_, updater::Pending>,
    progress: tauri::ipc::Channel<updater::Progress>,
) -> Result<(), String> {
    updater::install(&app, &pending, progress).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::Pending::default())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let config = app.path().app_config_dir()?;
            let cache = app.path().app_cache_dir()?;
            std::fs::create_dir_all(&data)?;
            let voices_dir = data.join("voices");
            std::fs::create_dir_all(&voices_dir)?;

            let db = Db::open(&data.join("apexnorth.db"))?;
            let _ = db.prune_empty_sessions();
            match db.revoke_short_grid_laps() {
                Ok(n) if n > 0 => log::info!("revoked {n} short grid lap(s)"),
                Err(e) => log::error!("revoke short grid laps: {e}"),
                _ => {}
            }
            let settings_path = config.join("settings.json");
            let mut settings = Settings::load(&settings_path);
            if settings.voice.piper_path.is_empty() {
                if let Some(p) = settings::find_piper() {
                    settings.voice.piper_path = p.display().to_string();
                }
            }
            if settings.voice.model_path.is_empty() {
                if let Some(v) = settings::find_voices(&[voices_dir.clone()]).into_iter().next() {
                    settings.voice.model_path = v;
                }
            }
            let builtin_tts = voices::builtin_dir(app.path().resource_dir().ok());
            log::info!("voice engine: {}", builtin_tts.as_ref().map_or("external Piper".into(), |d| d.display().to_string()));
            if let Some(dir) = &builtin_tts {
                // A missing voice (none chosen yet, or Ryan from an older
                // version) falls back to the default: Kokoro's Michael.
                if !voices::voice_available(&settings.voice.model_path, Some(dir)) {
                    if let Some(v) = voices::default_voice(dir) {
                        settings.voice.model_path = v;
                    }
                }
            }
            let voice_cache = cache.join("voice");
            let handle = app.handle().clone();
            let voice = Voice::start(settings.voice.clone(), builtin_tts.clone(), voice_cache, move |e| {
                let _ = handle.emit("voice_error", e);
            });
            let auto_ip = settings.auto_connect.then(|| settings.last_ip.clone()).flatten();

            let db = Arc::new(Mutex::new(db));
            {
                // Load the track catalog off the UI thread and name tracks
                // recorded before it existed.
                let db = db.clone();
                std::thread::spawn(move || {
                    catalog::tracks::catalog();
                    match db.lock().name_known_tracks() {
                        Ok(n) if n > 0 => log::info!("named {n} tracks from the catalog"),
                        Ok(_) => {}
                        Err(e) => log::error!("name tracks: {e}"),
                    }
                });
            }

            app.manage(AppState {
                db,
                settings: Arc::new(RwLock::new(settings)),
                settings_path,
                voices_dir,
                builtin_tts,
                voice,
                status: Arc::new(Mutex::new(ConnStatus::idle())),
                link: Mutex::new(None),
                sim: Mutex::new(None),
            });

            if let Some(ip) = auto_ip.and_then(|ip| parse_ip(&ip).ok()) {
                let handle = app.handle().clone();
                let state = app.state::<AppState>();
                start_link(&handle, &state, Some(ip));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            connect,
            start_demo,
            disconnect,
            get_status,
            demo_running,
            local_ips,
            list_sessions,
            session_laps,
            track_laps,
            get_lap,
            delete_session,
            set_session_kind,
            set_session_conditions,
            voice_builtin,
            rename_track,
            track_catalog,
            get_settings,
            save_settings,
            list_voices,
            voices_dir,
            test_voice,
            build_info,
            check_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}