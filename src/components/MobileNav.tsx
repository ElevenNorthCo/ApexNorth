import type { ReactNode } from "react";
import { useStore, type View } from "../lib/store";
import type { ConnStatus } from "../lib/api";
import { useUpdates } from "../lib/updates";
import { Cog, Gauge, List, Wifi } from "./icons";

/** Link state as the toolbar shows it: text and colour. */
export function linkBadge(status: ConnStatus, paused: boolean): [string, string] {
  if (status.state === "live") return paused ? ["PAUSED", "#ff9f1a"] : [`LIVE ${Math.round(status.hz)}Hz`, "#39ff7a"];
  if (status.state === "idle") return ["OFFLINE", "#8b9b96"];
  if (status.state === "error") return ["ERROR", "#ff3b4a"];
  if (status.state === "lost") return ["SIGNAL LOST", "#ff9f1a"];
  return ["SEARCHING", "#ffc81a"];
}

/**
 * Phone navigation: a tab bar along the bottom edge, in thumb reach, in
 * place of the desktop TopBar. Same views, same colours.
 */
export function MobileNav({ slim = false }: { slim?: boolean }) {
  const view = useStore((s) => s.view);
  const status = useStore((s) => s.status);
  const paused = useStore((s) => !!s.live?.paused);
  const go = useStore((s) => s.go);
  const updateReady = useUpdates((s) => !!s.offer);
  const [text, color] = linkBadge(status, paused);

  const tab = (label: string, icon: ReactNode, target: View, match: View["name"][], extra?: ReactNode) => {
    const active = match.includes(view.name);
    return (
      <button
        key={label}
        onClick={() => go(target)}
        aria-label={label}
        className={`tech relative flex flex-1 items-center justify-center gap-1.5 ${slim ? "flex-row text-[11px]" : "flex-col text-[10px]"} font-medium ${
          active ? "text-accent" : "text-white/70"
        }`}
      >
        {active && <span className="absolute inset-x-5 top-0 h-[3px] rounded-b bg-accent shadow-[0_0_10px_#39ff7a]" />}
        <span className="relative">
          {icon}
          {extra}
        </span>
        <span>{label}</span>
      </button>
    );
  };

  const size = slim ? 18 : 22;
  return (
    <nav
      className={`relative z-40 flex shrink-0 items-stretch border-t border-accent/25 bg-[#050807]/95 backdrop-blur ${slim ? "h-[44px]" : "h-[58px]"}`}
      title={text}
    >
      <div className="pointer-events-none absolute inset-x-0 top-0 h-px bg-gradient-to-r from-accent/70 via-accent/10 to-accent/40" />
      {tab("Dash", <Gauge width={size} height={size} />, { name: "dashboard" }, ["dashboard"])}
      {tab("Activity", <List width={size} height={size} />, { name: "activity" }, ["activity", "session", "lap"])}
      {tab(
        status.state === "live" ? text.replace("LIVE ", "") : "Connect",
        <Wifi width={size} height={size} style={{ color: status.state === "idle" ? undefined : color }} />,
        { name: "connect" },
        ["connect"],
        <span
          className={`absolute -right-1 -top-0.5 h-[8px] w-[8px] rounded-full ${status.state === "live" && !paused ? "pulse-dot" : ""}`}
          style={{ background: color }}
        />,
      )}
      {tab(
        "Settings",
        <Cog width={size} height={size} />,
        { name: "settings" },
        ["settings"],
        // A new version is out: the Update button is in Settings.
        updateReady && <span className="absolute -right-1 -top-0.5 h-[8px] w-[8px] rounded-full bg-accent shadow-[0_0_6px_#39ff7a]" />,
      )}
    </nav>
  );
}
