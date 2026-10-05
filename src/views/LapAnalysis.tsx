import { useEffect, useMemo, useRef, useState } from "react";
import { api, type LapDetail, type LapSummary, type Sample } from "../lib/api";
import { carName, delta, lapTime, split } from "../lib/format";
import { useStore } from "../lib/store";
import { TrackMap } from "../components/TrackMap";
import { TelemetryChart, type ChartSeries } from "../components/TelemetryChart";
import { Back, Pause, Play } from "../components/icons";

const SECTORS = 5;

/** Index of the first sample at or beyond `v` for a monotonically increasing key. */
function seek(samples: Sample[], key: "d" | "t", v: number): number {
  let lo = 0;
  let hi = samples.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (samples[mid][key] < v) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

interface Aligned {
  spd: (number | null)[];
  thr: (number | null)[];
  brk: (number | null)[];
  gear: (number | null)[];
  delta: (number | null)[];
  /** Where the reference car was at the same elapsed time (the ghost). */
  ghost: ({ x: number; z: number } | null)[];
}

/** How far along segment a→b the point p projects (0..1 inside), and the squared distance to it. */
function project(p: Sample, a: Sample, b: Sample): [number, number] {
  const dx = b.x - a.x;
  const dz = b.z - a.z;
  const len2 = dx * dx + dz * dz || 1e-6;
  const u = ((p.x - a.x) * dx + (p.z - a.z) * dz) / len2;
  const c = Math.max(0, Math.min(1, u));
  const ex = a.x + dx * c - p.x;
  const ez = a.z + dz * c - p.z;
  return [u, ex * ex + ez * ez];
}

/**
 * Match each point of the primary lap to the same place on the reference lap
 * (nearest point on its line, searched forward so the matching can't jump
 * across the track), so traces and the delta compare like with like even
 * when the laps were recorded from slightly different start points.
 */
function align(primary: Sample[], ref: Sample[]): Aligned {
  const out: Aligned = { spd: [], thr: [], brk: [], gear: [], delta: [], ghost: [] };
  const last = ref.length - 2;
  // Start: the nearest segment in the first fifth of the reference lap.
  let j = 0;
  let bestD = Infinity;
  for (let k = 0; k <= Math.min(last, Math.ceil(ref.length / 5)); k++) {
    const [, d2] = project(primary[0], ref[k], ref[k + 1]);
    if (d2 < bestD) {
      bestD = d2;
      j = k;
    }
  }
  const WINDOW = 150; // segments (~5 s at 30 Hz)
  for (const p of primary) {
    let bj = j;
    let bu = 0;
    let bd = Infinity;
    for (let k = Math.max(0, j - 3); k <= Math.min(last, j + WINDOW); k++) {
      const [u, d2] = project(p, ref[k], ref[k + 1]);
      if (d2 < bd) {
        bd = d2;
        bj = k;
        bu = u;
      }
    }
    j = bj;
    // Off either end of the reference line, or nowhere near it: no match.
    const off = (bj === 0 && bu < 0) || (bj === last && bu > 1) || bd > 60 * 60;
    if (off) {
      out.spd.push(null);
      out.thr.push(null);
      out.brk.push(null);
      out.gear.push(null);
      out.delta.push(null);
    } else {
      const a = ref[bj];
      const b = ref[bj + 1];
      const f = Math.max(0, Math.min(1, bu));
      const lerp = (x: number, y: number) => x + (y - x) * f;
      out.spd.push(lerp(a.spd, b.spd));
      out.thr.push(lerp(a.thr, b.thr));
      out.brk.push(lerp(a.brk, b.brk));
      out.gear.push(f < 0.5 ? a.gear : b.gear);
      out.delta.push((p.t - lerp(a.t, b.t)) / 1000);
    }
    // Ghost car: the reference at the same lap time.
    if (p.t < ref[0].t || p.t > ref[ref.length - 1].t) out.ghost.push(null);
    else {
      const i = Math.max(1, seek(ref, "t", p.t));
      const a = ref[i - 1];
      const b = ref[i];
      const f = Math.max(0, Math.min(1, (p.t - a.t) / (b.t - a.t || 1)));
      out.ghost.push({ x: a.x + (b.x - a.x) * f, z: a.z + (b.z - a.z) * f });
    }
  }
  return out;
}

export function LapAnalysis({ lapId, sessionId }: { lapId: number; sessionId?: number }) {
  const go = useStore((s) => s.go);
  const imperial = useStore((s) => s.settings?.units === "imperial");
  const [lap, setLap] = useState<LapDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [options, setOptions] = useState<LapSummary[]>([]);
  const [cmpId, setCmpId] = useState<number | null>(null);
  const [cmp, setCmp] = useState<LapDetail | null>(null);
  const [cursor, setCursor] = useState(0);
  const [sector, setSector] = useState<number | null>(null);
  const [zoom, setZoom] = useState(1);
  const [playing, setPlaying] = useState(false);
  const [rate, setRate] = useState(1);

  useEffect(() => {
    setLap(null);
    setCursor(0);
    api
      .getLap(lapId)
      .then(async (l) => {
        setLap(l);
        const [own, track] = await Promise.all([
          api.sessionLaps(l.sessionId),
          l.trackId != null ? api.trackLaps(l.trackId) : Promise.resolve([]),
        ]);
        const seen = new Set<number>([l.id]);
        const merged = [...own.filter((o) => o.isValid), ...track].filter((o) => !seen.has(o.id) && seen.add(o.id));
        setOptions(merged.sort((a, b) => a.timeMs - b.timeMs));
      })
      .catch((e) => setError(String(e)));
  }, [lapId]);

  useEffect(() => {
    if (cmpId == null) setCmp(null);
    else api.getLap(cmpId).then(setCmp).catch((e) => setError(String(e)));
  }, [cmpId]);

  const samples = lap?.samples ?? [];
  const len = samples.length ? samples[samples.length - 1].d : 1;
  const x = useMemo(() => samples.map((s) => s.d), [samples]);
  const aligned = useMemo(() => (cmp && samples.length && cmp.samples.length > 1 ? align(samples, cmp.samples) : null), [samples, cmp]);

  const sectorIdx = useMemo(
    () => Array.from({ length: SECTORS + 1 }, (_, k) => (samples.length ? seek(samples, "d", (len * k) / SECTORS) : 0)),
    [samples, len],
  );
  const xRange: [number, number] = sector == null ? [0, len] : [(len * sector) / SECTORS, (len * (sector + 1)) / SECTORS];
  const mapRange = useMemo<[number, number] | null>(
    () => (sector == null ? null : [sectorIdx[sector], sectorIdx[sector + 1]]),
    [sector, sectorIdx],
  );
  const sectorMarks = useMemo(() => sectorIdx.slice(0, SECTORS), [sectorIdx]);

  // Playback in real time along the lap.
  const raf = useRef(0);
  const cursorRef = useRef(cursor);
  cursorRef.current = cursor;
  useEffect(() => {
    if (!playing || !samples.length) return;
    const startIdx = cursorRef.current >= samples.length - 1 ? 0 : cursorRef.current;
    const t0 = samples[startIdx].t;
    const wall0 = performance.now();
    const frame = (now: number) => {
      const idx = seek(samples, "t", t0 + (now - wall0) * rate);
      setCursor(idx);
      if (idx >= samples.length - 1) setPlaying(false);
      else raf.current = requestAnimationFrame(frame);
    };
    raf.current = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf.current);
  }, [playing, rate, samples]);

  const onChartCursor = (d: number) => {
    if (!playing && samples.length) setCursor(Math.min(samples.length - 1, seek(samples, "d", d)));
  };
  // Grabbing a chart takes over from playback: pause, then drag to scrub.
  const onScrubStart = (d: number) => {
    setPlaying(false);
    if (samples.length) setCursor(Math.min(samples.length - 1, seek(samples, "d", d)));
  };

  const k = imperial ? 0.621371 : 1;
  const charts = useMemo(() => {
    if (!samples.length) return [];
    const dim = "rgba(255,255,255,0.45)";
    const refSpd = (aligned?.spd ?? []).filter((v): v is number => v != null);
    const maxSpd = Math.ceil(Math.max(...samples.map((s) => s.spd * k), ...refSpd.map((v) => v * k)));
    const list: { title: string; series: ChartSeries[]; yRange?: [number, number]; height: number; zero?: boolean }[] = [
      {
        title: imperial ? "SPEED mph" : "SPEED km/h",
        height: 130,
        yRange: [0, maxSpd],
        series: [
          { label: "Speed", values: samples.map((s) => s.spd * k), color: "#7dd3fc", width: 1.8 },
          ...(aligned ? [{ label: "Ref", values: aligned.spd.map((v) => (v == null ? null : v * k)), color: dim, dash: [4, 4] }] : []),
        ],
      },
      {
        title: "THROTTLE %",
        height: 100,
        yRange: [0, 100],
        series: [
          { label: "Throttle", values: samples.map((s) => s.thr), color: "#86efac" },
          ...(aligned ? [{ label: "Ref", values: aligned.thr, color: dim, dash: [4, 4] }] : []),
        ],
      },
      {
        title: "BRAKE %",
        height: 100,
        yRange: [0, 100],
        series: [
          { label: "Brake", values: samples.map((s) => s.brk), color: "#f87171" },
          ...(aligned ? [{ label: "Ref", values: aligned.brk, color: dim, dash: [4, 4] }] : []),
        ],
      },
      {
        title: "GEAR",
        height: 80,
        series: [
          { label: "Gear", values: samples.map((s) => s.gear), color: "#c4b5fd", stepped: true },
          ...(aligned ? [{ label: "Ref", values: aligned.gear, color: dim, dash: [4, 4], stepped: true }] : []),
        ],
      },
    ];
    if (aligned) {
      list.unshift({
        title: "DELTA s (+ slower)",
        height: 90,
        zero: true,
        series: [{ label: "Delta", values: aligned.delta, color: "#c084fc", width: 1.8 }],
      });
    }
    return list;
  }, [samples, aligned, k, imperial]);

  if (error) return <div className="p-8 text-red-300">{error}</div>;
  if (!lap) return <div className="flex h-full items-center justify-center text-muted">Loading lap…</div>;

  const cur = samples[Math.min(cursor, samples.length - 1)];
  const sectorDelta = (i: number) => (cmp && lap.sectorsMs[i] != null && cmp.sectorsMs[i] != null ? lap.sectorsMs[i] - cmp.sectorsMs[i] : null);

  return (
    <div className="flex h-full bg-ink">
      <div className="relative h-full w-[52%] min-w-[360px]">
        <TrackMap
          samples={samples}
          compare={cmp?.samples}
          ghost={aligned?.ghost[cursor] ?? null}
          cursor={cursor}
          range={mapRange}
          zoom={zoom}
          onZoom={setZoom}
          sectorMarks={sectorMarks}
        />
        <button
          onClick={() => (sessionId != null ? go({ name: "session", sessionId }) : go({ name: "activity" }))}
          className="absolute left-4 top-4 flex h-12 w-12 items-center justify-center rounded-full bg-[#0e1413] border border-white/10 text-white hover:border-accent/60"
        >
          <Back />
        </button>
        <div className="absolute bottom-5 left-4 flex items-center gap-2">
          <button
            onClick={() => setPlaying((p) => !p)}
            className="flex h-12 w-12 items-center justify-center rounded-full bg-accent text-black hover:brightness-110"
            title={playing ? "Pause" : "Play lap"}
          >
            {playing ? <Pause /> : <Play />}
          </button>
          {[1, 2, 4].map((r) => (
            <button
              key={r}
              onClick={() => setRate(r)}
              className={`num h-9 rounded-full px-3 text-[13px] ${rate === r ? "bg-white text-black" : "bg-[#0e1413] border border-white/10 text-white/80"}`}
            >
              {r}×
            </button>
          ))}
        </div>
      </div>

      <div className="scroll-thin flex-1 overflow-y-auto border-l border-accent/20 bg-panel px-5 pb-10 pt-5">
        <div className="flex items-baseline justify-between">
          <div>
            <div className="text-[13px] text-white/50">
              {lap.trackName ?? "Unknown track"} · {carName(lap.carCode, lap.carName)}
            </div>
            <h1 className="num mt-1 text-[28px] font-bold">
              Lap {lap.lapNo} · {lapTime(lap.timeMs)}
            </h1>
          </div>
          <div className="num text-right text-[13px] text-white/60">
            <div>{Math.round(cur.spd * k)} {imperial ? "mph" : "km/h"} · G{cur.gear} · {cur.thr}% / {cur.brk}%</div>
            <div>t {split(cur.t)}s</div>
          </div>
        </div>

        <div className="mt-4 overflow-x-auto rounded-2xl border border-white/10 bg-black/40">
          <table className="num w-full text-[15px]">
            <thead>
              <tr className="border-b border-white/10 text-left font-sans text-[14px] font-semibold">
                <th className="px-4 py-3">Driver</th>
                {Array.from({ length: SECTORS }, (_, i) => (
                  <th key={i} className="px-3 py-3 text-right">S{i + 1}</th>
                ))}
                <th className="px-4 py-3 text-right">Lap</th>
              </tr>
            </thead>
            <tbody>
              <tr className="border-b border-white/10">
                <td className="px-4 py-3 font-sans font-semibold text-accent">Lap {lap.lapNo}</td>
                {Array.from({ length: SECTORS }, (_, i) => (
                  <td key={i} className="px-3 py-3 text-right">{split(lap.sectorsMs[i])}</td>
                ))}
                <td className="px-4 py-3 text-right font-bold">{lapTime(lap.timeMs)}</td>
              </tr>
              <tr className="bg-white/[0.03]">
                <td className="px-4 py-2">
                  <select
                    value={cmpId ?? ""}
                    onChange={(e) => setCmpId(e.target.value ? Number(e.target.value) : null)}
                    className="w-full rounded-lg bg-transparent font-sans font-semibold text-accent outline-none"
                  >
                    <option value="">⊕ Compare</option>
                    {options.map((o) => (
                      <option key={o.id} value={o.id}>
                        {lapTime(o.timeMs)} · lap {o.lapNo}
                        {o.sessionId === lap.sessionId ? "" : " (other session)"}
                      </option>
                    ))}
                  </select>
                </td>
                {Array.from({ length: SECTORS }, (_, i) => (
                  <td key={i} className="px-3 py-2 text-right text-white/70">{cmp ? split(cmp.sectorsMs[i]) : "—"}</td>
                ))}
                <td className="px-4 py-2 text-right text-white/70">{cmp ? lapTime(cmp.timeMs) : "—"}</td>
              </tr>
              {cmp && (
                <tr className="border-t border-white/10 text-[13px]">
                  <td className="px-4 py-2 font-sans text-white/50">Delta</td>
                  {Array.from({ length: SECTORS }, (_, i) => {
                    const d = sectorDelta(i);
                    return (
                      <td key={i} className={`px-3 py-2 text-right ${d == null ? "" : d <= 0 ? "text-green-400" : "text-red-400"}`}>
                        {d == null ? "—" : delta(d)}
                      </td>
                    );
                  })}
                  <td className={`px-4 py-2 text-right font-bold ${lap.timeMs <= cmp.timeMs ? "text-green-400" : "text-red-400"}`}>
                    {delta(lap.timeMs - cmp.timeMs)}
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>

        <div className="mt-4 inline-flex rounded-2xl border border-white/10 bg-black/40 p-1">
          {[null, 0, 1, 2, 3, 4].map((s) => (
            <button
              key={String(s)}
              onClick={() => {
                setSector(s);
                setZoom(1);
              }}
              className={`rounded-xl px-4 py-2 text-[15px] font-medium ${sector === s ? "bg-accent text-black" : "text-white/75 hover:text-white"}`}
            >
              {s == null ? "All" : `S${s + 1}`}
            </button>
          ))}
        </div>

        <div className="mt-4 space-y-2">
          {charts.map((c) => (
            <div key={c.title}>
              <div className="num pl-1 text-[11px] tracking-[0.08em] text-white/45">{c.title}</div>
              <TelemetryChart
                x={x}
                series={c.series}
                height={c.height}
                yRange={c.yRange}
                xRange={xRange}
                cursorD={cur.d}
                onCursor={onChartCursor}
                onScrubStart={onScrubStart}
                zeroLine={c.zero}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
