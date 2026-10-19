import { useEffect, useMemo, useState } from "react";
import { api, type SessionKind, type SessionSummary } from "../lib/api";
import { carName, delta, lapTime, shortDate, timeOfDay } from "../lib/format";
import { KINDS, carClass, carImage, conditionTags, conditionsMatch, kindInfo, sessionKind, splitCar, trackTitle } from "../lib/media";
import { useStore } from "../lib/store";
import { TrackOutline } from "../components/TrackOutline";
import { TrackArt } from "../components/Art";
import { Flag } from "../components/Flag";
import { ChevronRight } from "../components/icons";

export function Activity() {
  const go = useStore((s) => s.go);
  const version = useStore((s) => s.lapsVersion);
  const [sessions, setSessions] = useState<SessionSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<SessionKind | "all">("all");

  useEffect(() => {
    api.listSessions().then(setSessions).catch((e) => setError(String(e)));
  }, [version]);

  /**
   * Best lap against your previous best at the same track under matching
   * conditions (sessions arrive newest first), preferring the same car. A first visit compares the best
   * lap with the session's average instead.
   */
  const improvement = useMemo(() => {
    const out = new Map<number, Gain>();
    const all = sessions ?? [];
    all.forEach((s, i) => {
      if (s.bestMs == null) return;
      const earlier = all.slice(i + 1).filter((e) => s.trackId != null && e.trackId === s.trackId && e.bestMs != null && conditionsMatch(e.conditions, s.conditions));
      const sameCar = earlier.filter((e) => e.carCode === s.carCode);
      const pool = sameCar.length ? sameCar : earlier;
      if (pool.length) out.set(s.id, { ms: s.bestMs - Math.min(...pool.map((e) => e.bestMs!)), label: "Vs prev best" });
      else if (s.avgMs != null && s.lapCount > 1) out.set(s.id, { ms: s.bestMs - Math.round(s.avgMs), label: "Best vs avg" });
    });
    return out;
  }, [sessions]);

  const shown = (sessions ?? []).filter((s) => filter === "all" || sessionKind(s) === filter);

  return (
    <div className="scroll-thin h-full overflow-y-auto pb-16">
      <div className="mx-auto max-w-[1400px] px-8 max-sm:px-4">
        <header className="flex flex-wrap items-end justify-between gap-6 pb-6 pt-8 max-sm:gap-4 max-sm:pb-4 max-sm:pt-5">
          <div>
            <h1 className="brand text-[52px] leading-none max-sm:text-[38px]">
              YOUR <span className="text-accent drop-shadow-[0_0_12px_rgba(57,255,122,0.5)]">ACTIVITY</span>
            </h1>
            <div className="tech mt-2 text-[14px] italic tracking-[0.3em] text-white/60">Recent sessions, laps and races</div>
          </div>
          <div className="flex overflow-hidden rounded-[12px] border border-white/10 bg-black/40 max-sm:w-full max-sm:overflow-x-auto">
            {[{ value: "all" as const, label: "All", color: "#39ff7a" }, ...KINDS].map((k) => {
              const active = filter === k.value;
              return (
                <button
                  key={k.value}
                  onClick={() => setFilter(k.value)}
                  className="tech shrink-0 whitespace-nowrap border-r border-white/10 px-6 py-3 text-[14px] font-medium transition last:border-r-0 max-sm:flex-1 max-sm:px-2.5 max-sm:text-[11px]"
                  style={
                    active
                      ? { color: k.color, background: `color-mix(in srgb, ${k.color} 12%, transparent)`, boxShadow: `inset 0 0 0 1.5px ${k.color}, 0 0 12px color-mix(in srgb, ${k.color} 35%, transparent)` }
                      : { color: "rgba(255,255,255,0.7)" }
                  }
                >
                  {k.label}
                </button>
              );
            })}
          </div>
        </header>

        {error && <p className="text-red-300">{error}</p>}
        {sessions && shown.length === 0 && (
          <div className="rounded-2xl border border-white/10 bg-black/30 p-10 text-center text-muted">
            {sessions.length === 0
              ? "No sessions yet. Every lap you drive is recorded automatically. Connect and get on track."
              : "No sessions of this type yet. Open a session to set whether it was practice, qualifying, a race or a time trial."}
          </div>
        )}

        <div className="space-y-4">
          {shown.map((s) => (
            <SessionCard key={s.id} s={s} gain={improvement.get(s.id)} onOpen={() => go({ name: "session", sessionId: s.id })} />
          ))}
        </div>
      </div>
    </div>
  );
}

interface Gain {
  ms: number;
  label: string;
}

function SessionCard({ s, gain, onOpen }: { s: SessionSummary; gain: Gain | undefined; onOpen: () => void }) {
  const kind = kindInfo(sessionKind(s));
  const [title, subtitle] = trackTitle(s);
  const fullCar = carName(s.carCode, s.carName);
  const [maker, model] = splitCar(fullCar);
  const cls = carClass(s.carName);
  const img = carImage(s.carCode);
  const c = kind.color;
  return (
    <button
      onClick={onOpen}
      className="group relative flex h-[208px] w-full items-stretch overflow-hidden rounded-[16px] text-left transition hover:brightness-110 max-sm:h-auto max-sm:flex-col"
      style={{
        border: `1.5px solid ${c}`,
        background: "linear-gradient(90deg, rgba(10,16,15,0.95), rgba(6,9,8,0.95))",
        boxShadow: `0 0 16px color-mix(in srgb, ${c} 30%, transparent), inset 4px 0 0 ${c}`,
      }}
    >
      <span className="absolute right-0 top-0 h-0 w-0 border-l-[22px] border-t-[22px] border-l-transparent" style={{ borderTopColor: c }} />

      <TrackArt venue={s.venue} outline={s.outline} className="w-[170px] shrink-0 max-md:hidden" />

      <div className="flex min-w-0 flex-1 flex-col justify-between py-5 pl-5 pr-2 max-sm:gap-3 max-sm:pb-3 max-sm:pr-8 max-sm:pt-4">
        <div>
          <div className="flex items-start gap-3">
            <span className="mt-[2px]"><Flag country={s.country} height={20} /></span>
            <h2 className="tech line-clamp-2 min-w-0 break-words text-[21px] font-bold leading-[1.1] tracking-[0.02em]">{title}</h2>
          </div>
          {subtitle && <div className="tech mt-1.5 truncate pl-[1px] text-[15px] text-white/60">{subtitle}</div>}
        </div>
        {/* Phones and narrow windows: the car column is hidden, so the car sits here. */}
        <div className="flex min-h-0 flex-1 items-center gap-3 py-1 lg:hidden" title={fullCar}>
          {img && <img src={img} alt={fullCar} className="h-[56px] w-auto max-w-[46%] shrink-0 object-contain max-sm:h-[72px]" draggable={false} />}
          <div className="min-w-0 leading-tight">
            <div className="tech truncate text-[11px] text-white/60">{maker}</div>
            <div className="tech line-clamp-2 text-[13px] font-semibold leading-snug">{model || maker}</div>
          </div>
        </div>
        <div className="flex items-end justify-between gap-3">
          <div className="min-w-0">
            <div className="text-[16px] text-white/90">{shortDate(s.startedAt)}</div>
            <div className="truncate text-[15px] text-white/55">
              <span style={{ color: c }}>{kind.label}</span> · GT7 · {timeOfDay(s.startedAt)}
            </div>
          </div>
          <div className="mr-3 shrink-0 max-xl:hidden">
            <TrackOutline points={s.outline} size={78} strokeWidth={4} stroke="white" />
          </div>
        </div>
      </div>

      <div className="my-4 flex w-[290px] shrink-0 flex-col items-center justify-center gap-1 text-center border-l border-white/10 px-6 max-lg:hidden">
        {img ? (
          <img src={img} alt={fullCar} className="min-h-0 w-full max-h-[112px] flex-1 object-contain object-center" draggable={false} />
        ) : (
          <div className="tech text-[12px] text-white/55">Car</div>
        )}
        <div className="w-full shrink-0 leading-tight" title={fullCar}>
          <div className="tech truncate text-[12px] text-white/70">{maker}</div>
          <div className="tech line-clamp-2 text-[14px] font-semibold leading-snug">{model || maker}</div>
        </div>
        <div className="flex shrink-0 flex-wrap justify-center gap-1.5">
          {cls && <Chip color="#b44cff">{cls}</Chip>}
          {conditionTags(s.conditions).map((t) => (
            <Chip key={t} color="#ffc81a">
              {t}
            </Chip>
          ))}
        </div>
      </div>

      <div className="my-5 grid w-[310px] shrink-0 grid-cols-[1fr_1.3fr] content-center gap-x-5 gap-y-4 border-l border-white/10 px-6 max-sm:mx-5 max-sm:mb-4 max-sm:mt-0 max-sm:w-auto max-sm:border-l-0 max-sm:border-t max-sm:px-0 max-sm:pt-3">
        <Stat label="Laps">{s.lapCount}</Stat>
        <Stat
          label="Best Lap"
          color="#39ff7a"
          big
          sub={
            s.perfectMs != null && s.bestMs != null && s.perfectMs < s.bestMs ? (
              <span title="Your best sectors from different laps, added up">
                Perfect <span className="num text-[#c084fc]">{lapTime(s.perfectMs)}</span>
              </span>
            ) : undefined
          }
        >
          {lapTime(s.bestMs)}
        </Stat>
        <Stat label="Avg Lap">{lapTime(s.avgMs != null ? Math.round(s.avgMs) : null)}</Stat>
        <Stat label={gain?.label ?? "Delta"} color={gain == null ? undefined : gain.ms <= 0 ? "#39ff7a" : "#ff9f1a"}>
          {gain == null ? "—" : delta(gain.ms)}
        </Stat>
      </div>

      <div className="my-5 flex w-[56px] shrink-0 items-center justify-center border-l border-white/10 max-sm:hidden" style={{ color: c }}>
        <ChevronRight width={32} height={32} className="transition group-hover:translate-x-1" />
      </div>
    </button>
  );
}

function Stat({ label, children, color, big, sub }: { label: string; children: React.ReactNode; color?: string; big?: boolean; sub?: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="tech text-[12px] text-white/55">{label}</div>
      <div className={`num mt-1 truncate font-bold leading-none ${big ? "text-[24px]" : "text-[21px]"}`} style={{ color }}>
        {children}
      </div>
      {sub && <div className="tech mt-1 truncate text-[11px] text-white/55">{sub}</div>}
    </div>
  );
}

export function Chip({ color, children }: { color: string; children: React.ReactNode }) {
  return (
    <span className="tech rounded-[5px] border px-2 py-[2px] text-[11px] font-semibold" style={{ color, borderColor: `color-mix(in srgb, ${color} 70%, transparent)` }}>
      {children}
    </span>
  );
}
