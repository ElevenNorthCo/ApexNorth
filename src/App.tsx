import { useEffect, useState } from "react";
import { startBridge, useStore, type View } from "./lib/store";
import { Dashboard } from "./views/Dashboard";
import { Connect } from "./views/Connect";
import { Activity } from "./views/Activity";
import { SessionView } from "./views/Session";
import { LapAnalysis } from "./views/LapAnalysis";
import { SettingsView } from "./views/Settings";
import { Cog, Gauge, List, Wifi } from "./components/icons";

export default function App() {
  const view = useStore((s) => s.view);
  const [navOpen, setNavOpen] = useState(false);

  useEffect(() => {
    startBridge().catch((e) => console.error("bridge", e));
  }, []);

  useEffect(() => setNavOpen(false), [view.name]);

  const showNav = view.name === "dashboard" ? navOpen : view.name !== "lap";

  return (
    <div className="relative h-full w-full">
      {view.name === "dashboard" && <Dashboard onToggleNav={() => setNavOpen((o) => !o)} />}
      {view.name === "connect" && <Connect />}
      {view.name === "activity" && <Activity />}
      {view.name === "session" && <SessionView sessionId={view.sessionId} />}
      {view.name === "lap" && <LapAnalysis lapId={view.lapId} sessionId={view.sessionId} />}
      {view.name === "settings" && <SettingsView />}
      {showNav && <BottomNav />}
    </div>
  );
}

function BottomNav() {
  const view = useStore((s) => s.view);
  const status = useStore((s) => s.status.state);
  const go = useStore((s) => s.go);
  const tabs: { icon: typeof Gauge; target: View; match: View["name"][]; title: string }[] = [
    { icon: Gauge, target: { name: "dashboard" }, match: ["dashboard"], title: "Dashboard" },
    { icon: List, target: { name: "activity" }, match: ["activity", "session"], title: "Activity" },
    { icon: Wifi, target: { name: "connect" }, match: ["connect"], title: "Connection" },
    { icon: Cog, target: { name: "settings" }, match: ["settings"], title: "Settings" },
  ];
  return (
    <nav className="absolute bottom-5 left-1/2 z-50 flex -translate-x-1/2 gap-2 rounded-[22px] border border-white/10 bg-[#1f2230]/95 p-2 shadow-2xl backdrop-blur">
      {tabs.map(({ icon: Icon, target, match, title }) => {
        const active = match.includes(view.name);
        return (
          <button
            key={title}
            title={title}
            onClick={() => go(target)}
            className={`relative flex h-12 w-16 items-center justify-center rounded-[16px] transition ${
              active ? "border border-[#b18cf2]/50 bg-[#b18cf2]/20 text-[#c9b0f7]" : "text-white/70 hover:text-white"
            }`}
          >
            <Icon />
            {title === "Connection" && (
              <span
                className="absolute right-3 top-2.5 h-2 w-2 rounded-full"
                style={{ background: status === "live" ? "#22c55e" : status === "idle" ? "#6b7280" : "#eab308" }}
              />
            )}
          </button>
        );
      })}
    </nav>
  );
}
