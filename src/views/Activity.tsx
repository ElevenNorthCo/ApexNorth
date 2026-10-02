import { useEffect, useMemo, useState } from "react";
import { api, type SessionSummary } from "../lib/api";
import { carName, lapTime, shortDate, timeOfDay } from "../lib/format";
import { useStore } from "../lib/store";
import { TrackOutline } from "../components/TrackOutline";
import { ChevronRight } from "../components/icons";

interface Group {
  key: string;
  trackName: string;
  date: number;
  outline: [number, number][];
  sessions: SessionSummary[];
}

export function Activity() {
  const go = useStore((s) => s.go);
  const version = useStore((s) => s.lapsVersion);
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listSessions().then(setSessions).catch((e) => setError(String(e)));
  }, [version]);

  const groups = useMemo(() => {
    const out: Group[] = [];
    for (const s of sessions ?? []) {
      const day = new Date(s.startedAt).toDateString();
      const key = `${s.trackId ?? "?"}|${day}`;
      const last = out[out.length - 1];
      if (last?.key === key) last.sessions.push(s);
      else
        out.push({
          key,
          trackName: s.trackName ?? "Unknown track",
          date: s.startedAt,
          outline: s.outline,
          sessions: [s],
        });
    }
    return out;
  }, [sessions]);

  return (
    <div className="scroll-thin h-full overflow-y-auto bg-panel pb-32">
      <header className="px-8 pb-6 pt-10">
        <h1 className="text-[32px] font-semibold tracking-tight">Your Activity</h1>
      </header>

      {error && <p className="px-8 text-red-300">{error}</p>}
      {sessions && sessions.length === 0 && (
        <div className="mx-8 rounded-2xl border border-white/10 p-8 text-center text-muted">
          No sessions yet. Every lap you drive is recorded automatically. Connect and get on track.
        </div>
      )}

      <div className="mx-auto max-w-4xl">
        {groups.map((g) => (
          <section key={g.key} className="mb-3 overflow-hidden bg-[#232634] sm:rounded-2xl">
            <div className="flex items-center justify-between bg-gradient-to-r from-[#2c3044] to-[#232634] px-7 py-5">
              <div>
                <h2 className="text-[24px] font-bold leading-tight">{g.trackName}</h2>
                <div className="mt-1 text-[16px] text-white/70">{shortDate(g.date)}</div>
              </div>
              <TrackOutline points={g.outline} size={76} />
            </div>
            {g.sessions.map((s, i) => (
              <button
                key={s.id}
                onClick={() => go({ name: "session", sessionId: s.id })}
                className="flex w-full items-center border-t border-white/5 px-7 py-4 text-left transition hover:bg-white/5"
              >
                <div className="grid flex-1 grid-cols-[90px_150px_1fr] gap-y-1">
                  {i === 0 && (
                    <>
                      <Label>LAPS</Label>
                      <Label>BEST</Label>
                      <Label>CAR</Label>
                    </>
                  )}
                  <Value>{s.lapCount}</Value>
                  <Value>{lapTime(s.bestMs)}</Value>
                  <Value className="truncate">{carName(s.carCode).toUpperCase()}</Value>
                  <div className="col-span-3 text-[15px] text-white/55">
                    {s.totalLaps > 0 ? "Race" : "Practice"} · GT7 · {timeOfDay(s.startedAt)}
                  </div>
                </div>
                <ChevronRight className="text-white/50" />
              </button>
            ))}
          </section>
        ))}
      </div>
    </div>
  );
}

const Label = ({ children }: { children: React.ReactNode }) => (
  <div className="text-[13px] tracking-[0.08em] text-white/55">{children}</div>
);
const Value = ({ children, className = "" }: { children: React.ReactNode; className?: string }) => (
  <div className={`num text-[20px] font-medium ${className}`}>{children}</div>
);
