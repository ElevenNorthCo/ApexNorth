import { useEffect, useState } from "react";
import { startBridge, useStore } from "./lib/store";
import { Dashboard } from "./views/Dashboard";
import { Connect } from "./views/Connect";
import { Activity } from "./views/Activity";
import { SessionView } from "./views/Session";
import { LapAnalysis } from "./views/LapAnalysis";
import { SettingsView } from "./views/Settings";
import { TopBar } from "./components/TopBar";
import { MobileNav } from "./components/MobileNav";
import { useViewport } from "./lib/viewport";

export default function App() {
  const view = useStore((s) => s.view);
  const [navOpen, setNavOpen] = useState(false);
  // Phones get a bottom tab bar instead of the desktop toolbar.
  const { compact, portrait } = useViewport();

  useEffect(() => {
    startBridge().catch((e) => console.error("bridge", e));
  }, []);

  useEffect(() => setNavOpen(false), [view.name]);

  // The dashboard is full-bleed; its logo drops the toolbar down over it.
  if (view.name === "dashboard") {
    return (
      <div className="relative h-full w-full">
        <Dashboard navOpen={navOpen} onToggleNav={() => setNavOpen((o) => !o)} />
        {navOpen &&
          (compact ? (
            <div className="absolute inset-x-0 bottom-0">
              <MobileNav slim={!portrait} />
            </div>
          ) : (
            <div className="absolute inset-x-0 top-0">
              <TopBar />
            </div>
          ))}
      </div>
    );
  }

  return (
    <div className="an-bg flex h-full w-full flex-col">
      {view.name !== "lap" && !compact && <TopBar />}
      <main className="relative min-h-0 flex-1">
        {view.name === "connect" && <Connect />}
        {view.name === "activity" && <Activity />}
        {view.name === "session" && <SessionView sessionId={view.sessionId} />}
        {view.name === "lap" && <LapAnalysis lapId={view.lapId} sessionId={view.sessionId} />}
        {view.name === "settings" && <SettingsView />}
      </main>
      {compact && view.name !== "lap" && <MobileNav slim={!portrait} />}
    </div>
  );
}
