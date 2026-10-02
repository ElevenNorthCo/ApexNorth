mod engineer;
mod session;
mod settings;
mod storage;
mod telemetry;

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

use engineer::rules::Engineer;
use engineer::voice::{self, Callout, Priority, Voice};
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
    voice_cache: PathBuf,
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
        if ctx.track.is_none() && lap.is_valid {
            if let Ok(t) = db.identify_track(lap.start_pos, lap.length_m, &lap.samples) {
                let _ = db.set_session_track(session_id, t.id);
                if let Ok(Some((time_ms, samples))) = db.best_lap_for(t.id, ctx.car_code) {
                    self.engine.set_reference(RefLap { time_ms, samples });
                }
                ctx.track = Some((t.id, t.name));
            }
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
        let callouts_cfg = self.settings.read().voice.callouts.clone();
        for ev in self.engine.on_packet(&p) {
            match &ev {
                EngineEvent::SessionStarted { car_code, total_laps } => {
                    self.session = Some(SessionCtx {
                        car_code: *car_code,
                        total_laps: *total_laps,
                        started_at: now_ms(),
                        db_id: None,
                        track: None,
                    });
                }
                EngineEvent::SessionEnded => self.session = None,
                EngineEvent::LapCompleted(lap, _) => self.record_lap(lap),
                _ => {}
            }
            let c = self.engineer.on_event(&ev, &callouts_cfg);
            self.speak(c);
        }
        let c = self.engineer.on_live(self.engine.live(), &callouts_cfg);
        self.speak(c);

        if self.last_emit.elapsed() >= FRAME_INTERVAL {
            self.last_emit = Instant::now();
            let _ = self.app.emit("telemetry", self.engine.live());
        }
    }

    fn status(&mut self, s: &ConnStatus) {
        *self.status.lock() = s.clone();
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
            self.awake = None;
        }
    }
}

fn start_link(app: &AppHandle, state: &AppState, ip: Option<IpAddr>) {
    // Stop the old link first so it releases the UDP port.
    drop(state.link.lock().take());
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
fn track_laps(state: State<AppState>, track_id: i64) -> Result<Vec<LapSummary>, String> {
    state.db.lock().track_laps(track_id, 20)
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
fn rename_track(state: State<AppState>, track_id: i64, name: String) -> Result<(), String> {
    state.db.lock().rename_track(track_id, name.trim())
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

#[tauri::command]
fn list_voices(state: State<AppState>) -> Vec<String> {
    let mut dirs = vec![state.voices_dir.clone()];
    let current = state.settings.read().voice.model_path.clone();
    if let Some(dir) = PathBuf::from(&current).parent() {
        dirs.push(dir.to_path_buf());
    }
    settings::find_voices(&dirs)
}

#[tauri::command]
fn voices_dir(state: State<AppState>) -> String {
    state.voices_dir.display().to_string()
}

#[tauri::command]
async fn test_voice(state: State<'_, AppState>, text: String) -> Result<(), String> {
    let cfg = state.settings.read().voice.clone();
    let cache = state.voice_cache.clone();
    let t = text.clone();
    tauri::async_runtime::spawn_blocking(move || voice::synthesize(&cfg, &cache, &t))
        .await
        .map_err(|e| e.to_string())??;
    state.voice.say(Callout::new(Priority::High, text));
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let config = app.path().app_config_dir()?;
            let cache = app.path().app_cache_dir()?;
            std::fs::create_dir_all(&data)?;
            let voices_dir = data.join("voices");
            std::fs::create_dir_all(&voices_dir)?;

            let db = Db::open(&data.join("apexnorth.db"))?;
            let _ = db.prune_empty_sessions();
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
            let voice_cache = cache.join("voice");
            let handle = app.handle().clone();
            let voice = Voice::start(settings.voice.clone(), voice_cache.clone(), move |e| {
                let _ = handle.emit("voice_error", e);
            });
            let auto_ip = settings.auto_connect.then(|| settings.last_ip.clone()).flatten();

            app.manage(AppState {
                db: Arc::new(Mutex::new(db)),
                settings: Arc::new(RwLock::new(settings)),
                settings_path,
                voices_dir,
                voice_cache,
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
            rename_track,
            get_settings,
            save_settings,
            list_voices,
            voices_dir,
            test_voice,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
