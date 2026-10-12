import { invoke } from "@tauri-apps/api/core";

export type ConnState = "idle" | "searching" | "connecting" | "live" | "lost" | "error";
export type Variant = "A" | "B" | "Tilde";
export type PitWindow = "none" | "no_stop" | "closed" | "open" | "box_now";
export type FuelState = "unknown" | "active" | "inactive";
export type InvalidReason = "joined_mid_lap" | "rewind_or_reset" | "telemetry_jump" | "insufficient_distance" | "pit_lap";
export type TyrePerformance = "unknown" | "fresh" | "stable" | "declining" | "poor" | "critical";
export type ChatterLevel = "minimal" | "normal" | "chatty" | "crewChief";
export type SessionKind = "practice" | "qualifying" | "race" | "time_trial";

export interface RaceStrategy {
  fuelPerLapPct: number;
  fuelLapsRemaining: number;
  fuelRequiredToFinishPct: number | null;
  stopsRequired: number;
  earliestPitLap: number | null;
  targetPitLap: number | null;
  latestPitLap: number | null;
  projectedFinishFuelPct: number | null;
  fuelMarginLaps: number | null;
  window: PitWindow;
}

export interface PitPlan {
  lap: number;
  driver: "fuel" | "tyres" | "both";
  fuelTarget: number;
  fuelLatest: number;
}

export interface TyreEstimate {
  performance: TyrePerformance;
  estimatedLossPerLapMs: number | null;
  projectedProblemLap: number | null;
  confidence: number;
  stintLaps: number;
  /** The fade rate comes from past sessions at this tyre wear, not this stint yet. */
  fromHistory: boolean;
}

export interface ConnStatus {
  state: ConnState;
  ip: string | null;
  variant: Variant | null;
  hz: number;
  error: string | null;
}

export interface LiveState {
  position: number | null;
  numCars: number | null;
  lap: number | null;
  totalLaps: number | null;
  gear: number;
  inGear: boolean;
  suggestedGear: number | null;
  speedKph: number;
  rpm: number;
  rpmAlertMin: number;
  rpmAlertMax: number;
  throttle: number;
  brake: number;
  steer: number | null;
  tyreTemp: [number, number, number, number];
  wheelSlip: [number, number, number, number];
  currentLapMs: number | null;
  lastLapMs: number | null;
  bestLapMs: number | null;
  deltaMs: number | null;
  lapDistance: number;
  lapFraction: number | null;
  lapInvalid: InvalidReason | null;
  fuelPct: number | null;
  fuelPerLapPct: number | null;
  fuelLapsLeft: number | null;
  fuelState: FuelState;
  lapsRemaining: number | null;
  pitWindow: PitWindow | null;
  strategy: RaceStrategy | null;
  pitPlan: PitPlan | null;
  tyreEstimate: TyreEstimate;
  gLat: number;
  gLon: number;
  boost: number;
  waterTemp: number;
  oilTemp: number;
  onTrack: boolean;
  paused: boolean;
  revLimit: boolean;
  tcs: boolean;
  asm: boolean;
  carCode: number;
}

export interface SessionSummary {
  id: number;
  trackId: number | null;
  trackName: string | null;
  carCode: number;
  /** Full car name from the built-in GT7 car list, null for an unlisted code. */
  carName: string | null;
  startedAt: number;
  totalLaps: number;
  lapCount: number;
  bestMs: number | null;
  /** Mean of the valid, non-pit laps. */
  avgMs: number | null;
  outline: [number, number][];
  /** Type the user tagged, or null (inferred from totalLaps). */
  kind: SessionKind | null;
  /** Race settings the user noted, or null. */
  conditions: SessionConditions | null;
  layoutId: string | null;
  venue: string | null;
  country: string | null;
}

/** Race settings GT7 doesn't send; the user notes them per session. */
export interface SessionConditions {
  mode?: "offline" | "online" | null;
  /** Balance of Performance on. */
  bop?: boolean | null;
  /** Tyre wear multiplier, 0 = off. */
  tyreWear?: number | null;
  /** Fuel consumption multiplier, 0 = off. */
  fuelRate?: number | null;
}

export interface LapSummary {
  id: number;
  sessionId: number;
  lapNo: number;
  timeMs: number;
  isPit: boolean;
  isValid: boolean;
  invalidReason: InvalidReason | null;
  sectorsMs: number[];
  fuelUsedPct: number | null;
  maxSpeed: number;
}

export interface Sample {
  t: number;
  d: number;
  x: number;
  y: number;
  z: number;
  spd: number;
  thr: number;
  brk: number;
  gear: number;
  rpm: number;
  steer: number;
  tt: [number, number, number, number];
  glat: number;
  glon: number;
}

export interface LapDetail extends LapSummary {
  carCode: number;
  carName: string | null;
  trackId: number | null;
  trackName: string | null;
  samples: Sample[];
}

/** A GT7 track layout from the built-in catalog. */
export interface TrackLayout {
  id: string;
  name: string;
  venue: string;
  country: string;
  lengthM: number;
  reverse: boolean;
}

export interface Callouts {
  lapTimes: boolean;
  sectors: boolean;
  pace: boolean;
  fuel: boolean;
  pitStrategy: boolean;
  tyres: boolean;
  wheelSlip: boolean;
  incidents: boolean;
  raceStart: boolean;
  position: boolean;
}

export interface Settings {
  lastIp: string | null;
  autoConnect: boolean;
  units: "metric" | "imperial";
  /** What the engineer calls you. Empty = no name. */
  driverName: string;
  voice: {
    enabled: boolean;
    piperPath: string;
    /** A Piper .onnx path, "kokoro:<speaker>", or a Kokoro voice file (.pt/.npy/.bin). */
    modelPath: string;
    volume: number;
    lengthScale: number;
    chatter: ChatterLevel;
    callouts: Callouts;
  };
}

export const api = {
  connect: (ip?: string) => invoke<void>("connect", { ip: ip ?? null }),
  startDemo: () => invoke<void>("start_demo"),
  disconnect: () => invoke<void>("disconnect"),
  getStatus: () => invoke<ConnStatus>("get_status"),
  demoRunning: () => invoke<boolean>("demo_running"),
  localIps: () => invoke<string[]>("local_ips"),
  listSessions: () => invoke<SessionSummary[]>("list_sessions"),
  sessionLaps: (sessionId: number) => invoke<LapSummary[]>("session_laps", { sessionId }),
  trackLaps: (trackId: number) => invoke<LapSummary[]>("track_laps", { trackId }),
  getLap: (lapId: number) => invoke<LapDetail>("get_lap", { lapId }),
  deleteSession: (sessionId: number) => invoke<void>("delete_session", { sessionId }),
  setSessionKind: (sessionId: number, kind: SessionKind | null) => invoke<void>("set_session_kind", { sessionId, kind }),
  setSessionConditions: (sessionId: number, conditions: SessionConditions | null) =>
    invoke<void>("set_session_conditions", { sessionId, conditions }),
  renameTrack: (trackId: number, name: string) => invoke<void>("rename_track", { trackId, name }),
  trackCatalog: () => invoke<TrackLayout[]>("track_catalog"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listVoices: () => invoke<VoiceOption[]>("list_voices"),
  voicesDir: () => invoke<string>("voices_dir"),
  testVoice: (text: string) => invoke<void>("test_voice", { text }),
  /** True when the app ships its own voice engine (no Piper install needed). */
  voiceBuiltin: () => invoke<boolean>("voice_builtin"),
  voiceCatalog: () => invoke<CatalogVoice[]>("voice_catalog"),
  /** Downloads a catalog voice; progress arrives as "voice_download" events. Returns the model path. */
  downloadVoice: (id: string) => invoke<string>("download_voice", { id }),
};

/** A voice to choose in Settings. `value` goes into `settings.voice.modelPath`. */
export interface VoiceOption {
  value: string;
  label: string;
  /** Bundled Kokoro voices ("kokoro" are the featured three), Kokoro voice files the user added, or Piper voices. */
  group: "kokoro" | "kokoroMore" | "custom" | "piper";
}

/** A Piper voice from the built-in catalog (`src-tauri/voices.json`). */
export interface CatalogVoice {
  id: string;
  name: string;
  accent: string;
  sizeMb: number;
  bundled: boolean;
  /** Model path once it's on disk. */
  path: string | null;
}

export interface VoiceDownload {
  id: string;
  received: number;
  total: number | null;
}