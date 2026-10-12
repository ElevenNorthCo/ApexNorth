import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { clock } from "../lib/format";
import { api } from "../lib/api";
import { checkForUpdate, RELEASES_URL, type Offer } from "../lib/updates";
import { useStore, type View } from "../lib/store";
import { Brand } from "./Brand";
import { Cog } from "./icons";

const TABS: { label: string; target: View; match: View["name"][] }[] = [
  { label: "Dashboard", target: { name: "dashboard" }, match: ["dashboard"] },
  { label: "Activity", target: { name: "activity" }, match: ["activity", "session", "lap"] },
  { label: "Connect", target: { name: "connect" }, match: ["connect"] },
];

/** App toolbar: wordmark, view tabs, link status, clock and settings. */
export function TopBar() {
  const view = useStore((s) => s.view);
  const status = useStore((s) => s.status);
  const paused = useStore((s) => !!s.live?.paused);
  const go = useStore((s) => s.go);
  const [now, setNow] = useState(() => clock());
  useEffect(() => {
    const id = setInterval(() => setNow(clock()), 5000);
    return () => clearInterval(id);
  }, []);
  // A few seconds after launch, then every 6 hours: is there a newer release?
  const [update, setUpdate] = useState<Offer | null>(null);
  useEffect(() => {
    const check = () => checkForUpdate().then((u) => u && setUpdate(u));
    const t = setTimeout(check, 4000);
    const id = setInterval(check, 6 * 60 * 60 * 1000);
    return () => {
      clearTimeout(t);
      clearInterval(id);
    };
  }, []);

  const [text, color] =
    status.state === "live"
      ? paused
        ? ["PAUSED", "#ff9f1a"]
        : [`LIVE ${Math.round(status.hz)}Hz`, "#39ff7a"]
      : status.state === "idle"
        ? ["OFFLINE", "#8b9b96"]
        : status.state === "error"
          ? ["ERROR", "#ff3b4a"]
          : status.state === "lost"
            ? ["SIGNAL LOST", "#ff9f1a"]
            : ["SEARCHING", "#ffc81a"];

  return (
    <header className="relative z-40 flex h-[68px] shrink-0 items-stretch border-b border-accent/25 bg-[#050807]/95 backdrop-blur">
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-px bg-gradient-to-r from-accent/70 via-accent/10 to-accent/40" />
      <button onClick={() => go({ name: "dashboard" })} className="flex items-center px-6" title="Dashboard">
        <Brand scale={0.85} />
      </button>

      <nav className="ml-2 flex items-stretch">
        <div className="my-3 w-px skew-x-[-20deg] bg-accent/30" />
        {TABS.map((t) => {
          const active = t.match.includes(view.name);
          return (
            <button
              key={t.label}
              onClick={() => go(t.target)}
              className={`tech relative px-5 text-[15px] font-medium transition ${active ? "text-accent" : "text-white/75 hover:text-white"}`}
            >
              {t.label}
              {active && <span className="absolute inset-x-4 bottom-0 h-[3px] rounded-t bg-accent shadow-[0_0_10px_#39ff7a]" />}
            </button>
          );
        })}
      </nav>

      <div className="ml-auto flex items-center gap-6 pr-3">
        {update && <UpdateButton update={update} />}
        <div className="h-full w-px skew-x-[-20deg] bg-accent/30" />
        <button onClick={() => go({ name: "connect" })} className="num flex items-center gap-2 text-[14px] font-bold" style={{ color }} title={status.ip ?? "Connect"}>
          <span className={`h-[9px] w-[9px] rounded-full ${status.state === "live" && !paused ? "pulse-dot" : ""}`} style={{ background: color }} />
          {text}
        </button>
        <span className="num text-[15px] font-bold text-white/90">{now}</span>
        <button
          onClick={() => go({ name: "settings" })}
          title="Settings"
          className={`flex h-[68px] w-[64px] items-center justify-center border-l border-white/10 transition ${
            view.name === "settings" ? "text-accent" : "text-white/80 hover:text-white"
          }`}
        >
          <Cog width={24} height={24} />
        </button>
      </div>
    </header>
  );
}

type Install = { step: "ready" } | { step: "downloading"; pct: number | null } | { step: "installing" } | { step: "failed"; error: string };

/**
 * "Update v0.3.0": click to download (the button fills as it goes), install
 * and restart. If that fails, the next click opens the download page.
 */
function UpdateButton({ update }: { update: Offer }) {
  const [state, setState] = useState<Install>({ step: "ready" });

  const start = () => {
    if (state.step === "failed" || update.manual) {
      openUrl(RELEASES_URL);
      return;
    }
    if (state.step !== "ready") return;
    setState({ step: "downloading", pct: null });
    api
      .installUpdate((p) => {
        if (p.event === "installing") setState({ step: "installing" });
        else if (p.event === "downloading")
          setState({ step: "downloading", pct: p.total ? Math.min(100, Math.round((p.done / p.total) * 100)) : null });
      })
      .catch((e) => setState({ step: "failed", error: String(e) }));
  };

  const pct = state.step === "downloading" ? state.pct : state.step === "installing" ? 100 : 0;
  const busy = state.step === "downloading" || state.step === "installing";
  const label =
    state.step === "ready"
      ? `Update v${update.version}`
      : state.step === "downloading"
        ? state.pct == null
          ? "Downloading…"
          : `Downloading ${state.pct}%`
        : state.step === "installing"
          ? "Installing…"
          : "Update failed · Download";
  const title =
    state.step === "ready"
      ? update.manual
        ? `ApexNorth ${update.version} is out. Opens the download page.`
        : `ApexNorth ${update.version} is out. Click to download and install it; the app restarts when it's done.`
      : state.step === "failed"
        ? `${state.error}\nClick to open the download page instead.`
        : "Keep ApexNorth open; it restarts when the update is installed.";
  const failed = state.step === "failed";

  return (
    <button
      onClick={start}
      disabled={busy}
      title={title}
      className={`tech relative min-w-[150px] overflow-hidden rounded-full border px-3 py-1 text-[12px] font-semibold shadow-[0_0_10px_rgba(57,255,122,0.35)] ${
        failed ? "border-[#ff9f1a]/70 bg-[#ff9f1a]/10 text-[#ff9f1a] hover:bg-[#ff9f1a]/20" : "border-accent/70 bg-accent/10 text-accent hover:bg-accent/20"
      } ${busy ? "cursor-default" : ""}`}
    >
      {busy && (
        <span
          className={`absolute inset-y-0 left-0 bg-accent/30 transition-[width] duration-300 ${state.step === "downloading" && state.pct == null ? "w-full animate-pulse" : ""}`}
          style={state.step === "downloading" && state.pct == null ? undefined : { width: `${pct}%` }}
        />
      )}
      <span className="relative tabular-nums">{label}</span>
    </button>
  );
}
