import { useEffect, useMemo, useState } from "react";
import { api, type InvalidReason, type LapSummary, type SessionConditions, type SessionKind, type SessionSummary, type TrackLayout } from "../lib/api";

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
import { TrackArt } from "../components/Art";
import { Flag } from "../components/Flag";
import { KINDS, conditionsMatch, kindInfo, sessionKind, trackTitle } from "../lib/media";

export function SessionView({ sessionId }: { sessionId: number }) {
  const go = useStore((s) => s.go);
  const version = useStore((s) => s.lapsVersion);
  const [all, setAll] = useState<SessionSummary[]>([]);
  const session = all.find((s) => s.id === sessionId) ?? null;
  const [laps, setLaps] = useState<LapSummary[]>([]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [layouts, setLayouts] = useState<TrackLayout[]>([]);
  useEffect(() => {
    if (editing != null && layouts.length === 0) api.trackCatalog().then(setLayouts);
  }, [editing, layouts.length]);

  const load = () => {
    api.listSessions().then(setAll);
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
    <div className="scroll-thin h-full overflow-y-auto pb-16">
      <div className="mx-auto max-w-5xl px-6 pt-6">
        <button onClick={() => go({ name: "activity" })} className="tech flex items-center gap-1 text-[14px] text-white/70 hover:text-accent">
          <Back /> Activity
        </button>

        {session && (
          <div
            className="relative mt-4 flex min-h-[150px] overflow-hidden rounded-[16px]"
            style={{ border: `1.5px solid ${kindInfo(sessionKind(session)).color}`, boxShadow: `0 0 18px color-mix(in srgb, ${kindInfo(sessionKind(session)).color} 30%, transparent)` }}
          >
            <div className="absolute inset-0 bg-[#070b0a]" />
            <div className="absolute inset-y-0 right-0 w-[62%]">
              <TrackArt venue={session.venue} outline={session.outline} fade="left" className="h-full w-full" />
            </div>
            <div className="absolute inset-0 bg-gradient-to-r from-[#070b0a] from-35% via-[#070b0a]/60 via-55% to-transparent" />
            <div className="relative flex flex-1 items-center justify-between gap-6 p-6">
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
                      className="tech rounded-lg border border-white/15 bg-black/60 px-3 py-1.5 text-[22px] font-bold outline-none focus:border-accent"
                    />
                    <datalist id="gt7-tracks">
                      {layouts.map((l) => (
                        <option key={l.id} value={l.name}>
                          {`${l.country} · ${(l.lengthM / 1000).toFixed(2)} km`}
                        </option>
                      ))}
                    </datalist>
                    <button type="submit" className="text-accent">
                      <Check />
                    </button>
                  </form>
                ) : (
                  <>
                    <h1 className="flex items-center gap-3">
                      <Flag country={session.country} height={22} />
                      <span className="tech text-[30px] font-bold leading-none tracking-[0.04em]">{trackTitle(session)[0]}</span>
                      {session.trackId != null && (
                        <button title="Rename track" onClick={() => setEditing(session.trackName ?? "")} className="text-white/40 hover:text-accent">
                          <Pencil width={18} height={18} />
                        </button>
                      )}
                    </h1>
                    {trackTitle(session)[1] && <div className="tech mt-1.5 text-[15px] text-white/60">{trackTitle(session)[1]}</div>}
                  </>
                )}
                <div className="mt-3 text-[15px] text-white/70">
                  {shortDate(session.startedAt)} · {timeOfDay(session.startedAt)} · {carName(session.carCode, session.carName)}
                  {session.totalLaps > 0 && ` · ${session.totalLaps} lap race distance`}
                </div>
              </div>
              <TrackOutline points={session.outline} size={96} stroke="white" strokeWidth={4} />
            </div>
          </div>
        )}

        {session && <KindPicker session={session} onChange={load} />}
        {session && <ConditionsPicker session={session} laps={laps} onChange={load} />}
        {session && <TrackComparison session={session} all={all} />}

        <h2 className="tech mb-2 mt-8 text-[13px] text-white/50">Laps</h2>
        <div className="overflow-hidden rounded-[14px] border border-laptime/50 bg-black/40 shadow-[0_0_14px_rgba(31,168,255,0.15)]">
          <table className="w-full text-left">
            <thead className="tech text-[12px] text-white/50">
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
                  className="num cursor-pointer border-b border-white/5 text-[15px] transition hover:bg-accent/[0.06]"
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

function KindPicker({ session, onChange }: { session: SessionSummary; onChange: () => void }) {
  const current = session.kind;
  const guessed = sessionKind({ ...session, kind: null });
  const set = async (kind: SessionKind | null) => {
    await api.setSessionKind(session.id, kind);
    onChange();
  };
  return (
    <div className="mt-5 flex flex-wrap items-center gap-4">
      <span className="tech text-[13px] text-white/50">Session type</span>
      <div className="flex overflow-hidden rounded-[10px] border border-white/10 bg-black/40">
        <button
          onClick={() => set(null)}
          title={`Let ApexNorth decide (currently ${kindInfo(guessed).label})`}
          className={`tech border-r border-white/10 px-4 py-2 text-[13px] ${current == null ? "bg-white/10 text-white" : "text-white/50 hover:text-white"}`}
        >
          Auto
        </button>
        {KINDS.map((k) => {
          const active = sessionKind(session) === k.value;
          return (
            <button
              key={k.value}
              onClick={() => set(k.value)}
              className="tech border-r border-white/10 px-4 py-2 text-[13px] last:border-r-0"
              style={
                active
                  ? { color: k.color, background: `color-mix(in srgb, ${k.color} 14%, transparent)`, boxShadow: `inset 0 0 0 1.5px ${k.color}` }
                  : { color: "rgba(255,255,255,0.6)" }
              }
            >
              {k.label}
            </button>
          );
        })}
      </div>
      {current == null && <span className="text-[12px] text-white/40">Guessed. Pick one to tag it.</span>}
    </div>
  );
}

const MULTIPLIERS = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 15, 20, 25, 30, 40, 50];

/**
 * Race settings GT7 doesn't send (online or offline, BoP, tyre wear and fuel
 * multipliers), noted by the driver. Fuel use is measured, so it's offered
 * as a hint.
 */
function ConditionsPicker({ session, laps, onChange }: { session: SessionSummary; laps: LapSummary[]; onChange: () => void }) {
  const c = session.conditions ?? {};
  const set = async (patch: SessionConditions) => {
    await api.setSessionConditions(session.id, { ...c, ...patch });
    onChange();
  };
  const fuel = laps.filter((l) => l.isValid && !l.isPit && l.fuelUsedPct != null).map((l) => l.fuelUsedPct!);
  const fuelPerLap = fuel.length ? fuel.reduce((a, b) => a + b, 0) / fuel.length : null;
  const seg = (active: boolean) =>
    `tech border-r border-white/10 px-3 py-2 text-[13px] last:border-r-0 ${active ? "bg-accent/15 text-accent shadow-[inset_0_0_0_1.5px_var(--color-accent)]" : "text-white/60 hover:text-white"}`;
  const multiplier = (label: string, value: number | null | undefined, on: (v: number | null) => void, hint?: string) => (
    <label className="flex items-center gap-2">
      <span className="tech text-[13px] text-white/50">{label}</span>
      <select
        value={value ?? ""}
        onChange={(e) => on(e.target.value === "" ? null : Number(e.target.value))}
        className="num rounded-[10px] border border-white/10 bg-black/40 px-3 py-2 text-[13px] outline-none focus:border-accent"
      >
        <option value="">—</option>
        {MULTIPLIERS.map((m) => (
          <option key={m} value={m}>
            {m === 0 ? "Off" : `×${m}`}
          </option>
        ))}
      </select>
      {hint && <span className="text-[12px] text-white/40">{hint}</span>}
    </label>
  );
  return (
    <div className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-3">
      <div className="flex items-center gap-3">
        <span className="tech text-[13px] text-white/50">Race</span>
        <div className="flex overflow-hidden rounded-[10px] border border-white/10 bg-black/40">
          {([["offline", "Single player"], ["online", "Online"]] as const).map(([m, label]) => (
            <button key={m} onClick={() => set({ mode: c.mode === m ? null : m })} className={seg(c.mode === m)}>
              {label}
            </button>
          ))}
        </div>
      </div>
      <div className="flex items-center gap-3">
        <span className="tech text-[13px] text-white/50">BoP</span>
        <div className="flex overflow-hidden rounded-[10px] border border-white/10 bg-black/40">
          {([[true, "On"], [false, "Off"]] as const).map(([b, label]) => (
            <button key={label} onClick={() => set({ bop: c.bop === b ? null : b })} className={seg(c.bop === b)}>
              {label}
            </button>
          ))}
        </div>
      </div>
      {multiplier("Tyre wear", c.tyreWear, (v) => set({ tyreWear: v }))}
      {multiplier(
        "Fuel",
        c.fuelRate,
        (v) => set({ fuelRate: v }),
        fuelPerLap == null ? undefined : fuelPerLap < 0.05 ? "No fuel used" : `Measured ${fuelPerLap.toFixed(1)}% a lap`,
      )}
    </div>
  );
}

/** Your pace at this track split by session type: clean time-trial pace vs race pace with traffic and wear. */
function TrackComparison({ session, all }: { session: SessionSummary; all: SessionSummary[] }) {
  if (session.trackId == null) return null;
  const atTrack = all.filter((s) => s.trackId === session.trackId);
  const here = atTrack.filter((s) => conditionsMatch(s.conditions, session.conditions));
  const skipped = atTrack.length - here.length;
  const rows = KINDS.map((k) => {
    const ss = here.filter((s) => sessionKind(s) === k.value);
    const bests = ss.map((s) => s.bestMs).filter((v): v is number => v != null);
    const avgs = ss.map((s) => s.avgMs).filter((v): v is number => v != null);
    return {
      ...k,
      sessions: ss.length,
      best: bests.length ? Math.min(...bests) : null,
      avg: avgs.length ? avgs.reduce((a, b) => a + b, 0) / avgs.length : null,
    };
  });
  const overall = rows.map((r) => r.best).filter((v): v is number => v != null);
  const ref = overall.length ? Math.min(...overall) : null;
  if (rows.filter((r) => r.sessions > 0).length < 1) return null;
  return (
    <>
      <h2 className="tech mb-2 mt-8 flex items-baseline gap-3 text-[13px] text-white/50">
        Your pace here by session type
        {skipped > 0 && (
          <span className="normal-case text-[12px] text-white/40">
            {skipped} {skipped === 1 ? "session" : "sessions"} with different conditions left out
          </span>
        )}
      </h2>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        {rows.map((r) => (
          <div
            key={r.value}
            className={`rounded-[12px] border bg-black/40 p-4 ${r.sessions ? "" : "opacity-40"}`}
            style={{ borderColor: `color-mix(in srgb, ${r.color} ${r.sessions ? 70 : 25}%, transparent)` }}
          >
            <div className="tech flex items-center justify-between text-[13px]" style={{ color: r.color }}>
              {r.label}
              <span className="text-[11px] text-white/40">{r.sessions ? `${r.sessions}×` : "none yet"}</span>
            </div>
            <div className="num mt-2 text-[24px] font-bold leading-none">{lapTime(r.best)}</div>
            <div className="mt-2 flex justify-between text-[12px] text-white/50">
              <span>avg {lapTime(r.avg != null ? Math.round(r.avg) : null)}</span>
              {r.best != null && ref != null && r.best !== ref && <span className="num text-[#ff9f1a]">+{((r.best - ref) / 1000).toFixed(3)}</span>}
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
