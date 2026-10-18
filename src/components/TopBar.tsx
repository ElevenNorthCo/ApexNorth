import { useEffect, useState } from "react";
import { clock } from "../lib/format";
import { useStore, type View } from "../lib/store";
import { Brand } from "./Brand";
import { linkBadge } from "./MobileNav";
import { Cog } from "./icons";
import { UpdateButton } from "./UpdateButton";

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

  const [text, color] = linkBadge(status, paused);

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
        <UpdateButton />
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
