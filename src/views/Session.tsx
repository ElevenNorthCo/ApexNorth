import { useEffect, useMemo, useState } from "react";
import { api, type InvalidReason, type LapSummary, type SessionSummary, type TrackLayout } from "../lib/api";

const INVALID_WHY: Record<InvalidReason, string> = {
  joined_mid_lap: "Timing joined part-way through the lap",
  rewind_or_reset: "Rewind or reset detected",
  telemetry_jump: "The car jumped position (confirmed over several packets)",
  insufficient_distance: "Lap was much shorter than the track",
  pit_lap: "Pit lap",
};
import { carName, lapTime, shortDate, split, timeOfDay } from "../lib/format";
import { useStore } from "../lib/store";
import { Btn } from "./Connect";
import { Back, Check, Pencil, Trash } from "../components/icons";
import { TrackOutline } from "../components/TrackOutline";

export function SessionView({ sessionId }: { sessionId: number }) {
  const go = useStore((s) => s.go);
  const version = useStore((s) => s.lapsVersion);
  const [session, setSession] = useState<SessionSummary | null>(null);
  const [laps, setLaps] = useState<LapSummary[]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [layouts, setLayouts] = useState<TrackLayout[]>([]);
  useEffect(() => {
    if (editing != null && layouts.length === 0) api.trackCatalog().then(setLayouts);
  }, [editing, layouts.length]);

  const load = () => {
    api.listSessions().then((all) => setSession(all.find((s) => s.id === sessionId) ?? null));
    api.sessionLaps(sessionId).then(setLaps);
  };
  useEffect(load, [sessionId, version]);

  const best = useMemo(() => {
    const valid = laps.filter((l) => l.isValid);
    return valid.length ? valid.reduce((a, b) => (b.timeMs < a.timeMs ? b : a)) : null;
  }, [laps]);
  const bestSectors = useMemo(
    () => [0, 1, 2, 3, 4].map((i) => Math.min(...laps.filter((l) => l.isValid && l.sectorsMs[i] != null).map((l) => l.sectorsMs[i]))),
    [laps],
  );

  const saveName = async () => {
    if (session?.trackId != null && editing?.trim()) {
      await api.renameTrack(session.trackId, editing.trim());
      load();
    }
    setEditing(null);
  };

  return (
    <div className="scroll-thin h-full overflow-y-auto bg-panel pb-32">
      <div className="mx-auto max-w-4xl px-6 pt-8">
        <button onClick={() => go({ name: "activity" })} className="flex items-center gap-1 text-white/70 hover:text-white">
          <Back /> Activity
        </button>

        {session && (
          <div className="mt-5 flex items-start justify-between gap-6">
            <div className="min-w-0">
              {editing != null ? (
                <form
                  className="flex items-center gap-2"
                  onSubmit={(e) => {
                    e.preventDefault();
                    saveName();
                  }}
                >
                  <input
                    autoFocus
                    list="gt7-tracks"
                    placeholder="Pick a GT7 track or type a name"
                    value={editing}
                    onChange={(e) => setEditing(e.target.value)}
                    className="rounded-lg border border-white/15 bg-black/40 px-3 py-1.5 text-[22px] font-bold outline-none focus:border-accent"
                  />
                  <datalist id="gt7-tracks">
                    {layouts.map((l) => (
                      <option key={l.id} value={l.name}>
                        {`${l.country} · ${(l.lengthM / 1000).toFixed(2)} km`}
                      </option>
                    ))}
                  </datalist>
                  <button type="submit" className="text-green-400">
                    <Check />
                  </button>
                </form>
              ) : (
                <h1 className="flex items-center gap-3 text-[28px] font-bold">
                  {session.trackName ?? "Unknown track"}
                  {session.trackId != null && (
                    <button title="Rename track" onClick={() => setEditing(session.trackName ?? "")} className="text-white/40 hover:text-white">
                      <Pencil width={18} height={18} />
                    </button>
                  )}
                </h1>
              )}
              <div className="mt-1 text-white/60">
                {shortDate(session.startedAt)} · {timeOfDay(session.startedAt)} · {carName(session.carCode, session.carName)} ·{" "}
                {session.totalLaps > 0 ? `Race (${session.totalLaps} laps)` : "Practice"}
              </div>
            </div>
            <TrackOutline points={session.outline} size={84} stroke="#4a9eff" />
          </div>
        )}

        <div className="mt-6 overflow-hidden rounded-2xl border border-white/10 bg-[#232634]">
          <table className="w-full text-left">
            <thead className="text-[12px] tracking-[0.08em] text-white/50">
              <tr className="border-b border-white/10">
                <th className="px-5 py-3 font-medium">LAP</th>
                <th className="px-3 py-3 font-medium">TIME</th>
                {[1, 2, 3, 4, 5].map((s) => (
                  <th key={s} className="px-3 py-3 text-right font-medium">S{s}</th>
                ))}
                <th className="px-3 py-3 text-right font-medium">FUEL</th>
                <th className="px-5 py-3" />
              </tr>
            </thead>
            <tbody>
              {laps.map((l) => (
                <tr
                  key={l.id}
                  onClick={() => go({ name: "lap", lapId: l.id, sessionId })}
                  className="num cursor-pointer border-b border-white/5 text-[15px] hover:bg-white/5"
                >
                  <td className="px-5 py-3 text-white/70">{l.lapNo}</td>
                  <td className={`px-3 py-3 font-bold ${best?.id === l.id ? "text-[#c084fc]" : !l.isValid ? "text-white/40" : ""}`}>
                    {lapTime(l.timeMs)}
                  </td>
                  {[0, 1, 2, 3, 4].map((i) => (
                    <td
                      key={i}
                      className={`px-3 py-3 text-right ${l.isValid && l.sectorsMs[i] === bestSectors[i] ? "text-[#c084fc]" : "text-white/75"}`}
                    >
                      {split(l.sectorsMs[i])}
                    </td>
                  ))}
                  <td className="px-3 py-3 text-right text-white/60">{l.fuelUsedPct != null ? `${l.fuelUsedPct.toFixed(1)}%` : "—"}</td>
                  <td className="px-5 py-3 text-right text-[11px] font-sans font-semibold">
                    {l.isPit ? (
                      <span className="rounded bg-amber-500/20 px-2 py-0.5 text-amber-300">PIT</span>
                    ) : !l.isValid ? (
                      <span className="rounded bg-white/10 px-2 py-0.5 text-white/50" title={l.invalidReason ? INVALID_WHY[l.invalidReason] : undefined}>
                        INVALID
                      </span>
                    ) : best?.id === l.id ? (
                      <span className="rounded bg-purple-500/25 px-2 py-0.5 text-purple-300">BEST</span>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="mt-6 flex justify-end">
          {confirmDelete ? (
            <div className="flex items-center gap-3">
              <span className="text-[14px] text-white/60">Delete this session and its laps?</span>
              <Btn onClick={() => setConfirmDelete(false)}>Cancel</Btn>
              <Btn
                danger
                onClick={async () => {
                  await api.deleteSession(sessionId);
                  go({ name: "activity" });
                }}
              >
                Delete
              </Btn>
            </div>
          ) : (
            <Btn danger onClick={() => setConfirmDelete(true)} className="flex items-center gap-2">
              <Trash width={16} height={16} /> Delete session
            </Btn>
          )}
        </div>
      </div>
    </div>
  );
}
