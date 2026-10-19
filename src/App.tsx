import { useEffect, useState, type MouseEvent } from "react";
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
import { useUpdateChecks } from "./lib/updates";

/** How long the dashboard's menu stays up after it's opened or touched. */
const NAV_HIDE_MS = 4000;

export default function App() {
  const view = useStore((s) => s.view);
  const [navOpen, setNavOpen] = useState(false);
  // Phones get a bottom tab bar instead of the desktop toolbar.
  const { compact, portrait } = useViewport();
  useUpdateChecks();

  useEffect(() => {
    startBridge().catch((e) => console.error("bridge", e));
  }, []);

  // Each touch of the open menu restarts its hide timer.
  const [navTouched, setNavTouched] = useState(0);

  useEffect(() => setNavOpen(false), [view.name]);

  // The dashboard's menu hides itself again after a few seconds.
  useEffect(() => {
    if (!navOpen || view.name !== "dashboard") return;
    const id = setTimeout(() => setNavOpen(false), NAV_HIDE_MS);
    return () => clearTimeout(id);
  }, [navOpen, navTouched, view.name]);

  // The dashboard is full-bleed; its logo, or a tap anywhere on it, drops the
  // menu over it.
  if (view.name === "dashboard") {
    const tap = (e: MouseEvent) => {
      // Buttons (logo, link badge, tiles that open something) do their own thing.
      if ((e.target as Element).closest("button, a, input, select, [data-nav]")) return;
      setNavOpen((o) => !o);
    };
    return (
      <div className="relative h-full w-full" onClick={tap}>
        <Dashboard navOpen={navOpen} onToggleNav={() => setNavOpen((o) => !o)} />
        {navOpen &&
          (compact ? (
            <div data-nav className="absolute inset-x-0 bottom-0" onPointerDown={() => setNavTouched((n) => n + 1)}>
              <MobileNav slim={!portrait} />
            </div>
          ) : (
            <div data-nav className="absolute inset-x-0 top-0" onPointerDown={() => setNavTouched((n) => n + 1)}>
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
