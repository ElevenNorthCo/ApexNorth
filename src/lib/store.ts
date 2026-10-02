import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api, type ConnStatus, type LapSummary, type LiveState, type Settings } from "./api";

export type View =
  | { name: "connect" }
  | { name: "dashboard" }
  | { name: "activity" }
  | { name: "session"; sessionId: number }
  | { name: "lap"; lapId: number; sessionId?: number }
  | { name: "settings" };

interface Store {
  view: View;
  status: ConnStatus;
  live: LiveState | null;
  settings: Settings | null;
  engineer: { text: string; at: number } | null;
  voiceError: string | null;
  lastLap: LapSummary | null;
  /** Bumps whenever a lap is saved, so lists can refresh. */
  lapsVersion: number;
  go: (v: View) => void;
  setSettings: (s: Settings) => Promise<void>;
}

export const useStore = create<Store>((set) => ({
  view: { name: "dashboard" },
  status: { state: "idle", ip: null, variant: null, hz: 0, error: null },
  live: null,
  settings: null,
  engineer: null,
  voiceError: null,
  lastLap: null,
  lapsVersion: 0,
  go: (view) => set({ view }),
  setSettings: async (s) => {
    await api.saveSettings(s);
    set({ settings: s, voiceError: null });
  },
}));

let started = false;

/** Wire backend events into the store. Call once at startup. */
export async function startBridge() {
  if (started) return;
  started = true;
  const [status, settings] = await Promise.all([api.getStatus(), api.getSettings()]);
  useStore.setState({
    status,
    settings,
    view: status.state === "idle" && !settings.lastIp ? { name: "connect" } : { name: "dashboard" },
  });
  await listen<LiveState>("telemetry", (e) => useStore.setState({ live: e.payload }));
  await listen<ConnStatus>("connection", (e) => useStore.setState({ status: e.payload }));
  await listen<string>("voice_error", (e) => useStore.setState({ voiceError: e.payload }));
  await listen<string>("engineer", (e) => useStore.setState({ engineer: { text: e.payload, at: Date.now() } }));
  await listen<{ lap: LapSummary }>("lap", (e) =>
    useStore.setState((s) => ({ lastLap: e.payload.lap, lapsVersion: s.lapsVersion + 1 })),
  );
}
