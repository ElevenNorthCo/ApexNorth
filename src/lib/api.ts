import { invoke } from "@tauri-apps/api/core";

export type ConnState = "idle" | "searching" | "connecting" | "live" | "lost" | "error";
export type Variant = "A" | "B" | "Tilde";
export type PitWindow = "none" | "no_stop" | "closed" | "open" | "box_now";

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
  fuelPct: number | null;
  fuelPerLapPct: number | null;
  fuelLapsLeft: number | null;
  lapsRemaining: number | null;
  pitWindow: PitWindow | null;
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
  startedAt: number;
  totalLaps: number;
  lapCount: number;
  bestMs: number | null;
  outline: [number, number][];
}

export interface LapSummary {
  id: number;
  sessionId: number;
  lapNo: number;
  timeMs: number;
  isPit: boolean;
  isValid: boolean;
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
  trackId: number | null;
  trackName: string | null;
  samples: Sample[];
}

export interface Callouts {
  lapTimes: boolean;
  fuel: boolean;
  pitWindow: boolean;
  tyres: boolean;
  wheelSlip: boolean;
  raceStart: boolean;
  position: boolean;
}

export interface Settings {
  lastIp: string | null;
  autoConnect: boolean;
  units: "metric" | "imperial";
  voice: {
    enabled: boolean;
    piperPath: string;
    modelPath: string;
    volume: number;
    lengthScale: number;
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
  renameTrack: (trackId: number, name: string) => invoke<void>("rename_track", { trackId, name }),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<void>("save_settings", { settings }),
  listVoices: () => invoke<string[]>("list_voices"),
  voicesDir: () => invoke<string>("voices_dir"),
  testVoice: (text: string) => invoke<void>("test_voice", { text }),
};
