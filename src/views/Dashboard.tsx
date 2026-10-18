import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import type { ConnStatus, LiveState, PitWindow } from "../lib/api";
import { DASH, delta, gearLabel, lapClock, speed, tyreColor } from "../lib/format";
import { useStore } from "../lib/store";
import { ScaleToFit } from "../components/ScaleToFit";
import { Tile } from "../components/Tile";
import { Brand } from "../components/Brand";
import { ChevronDown } from "../components/icons";
import { useViewport } from "../lib/viewport";

type Box = { left: number; top: number; width: number; height: number };
type At = Pick<CSSProperties, "left" | "right" | "top" | "width">;

/**
 * Where everything sits, in design-canvas pixels. The landscape canvas is
 * the desktop dashboard (and a phone on its side, mounted by the wheel);
 * the portrait canvas stacks the same tiles for a phone held upright.
 */
interface Layout {
  w: number;
  h: number;
  brand: At;
  shift: At;
  badge: At;
  position: Box;
  lap: Box;
  lapsLeft: Box;
  tyres: Box;
  gear: At;
  speed: At;
  laps: Box;
  brake: Box;
  throttle: Box;
  fuel: Box;
  clock: Box | null;
  chips: At;
  engineer: At;
  slash: "wide" | "narrow";
}

const LANDSCAPE: Layout = {
  w: 1000,
  h: 540,
  brand: { left: 18, top: 10 },
  shift: { left: 356, top: 38, width: 340 },
  badge: { right: 26, top: 24 },
  position: { left: 28, top: 76, width: 128, height: 82 },
  lap: { left: 166, top: 76, width: 112, height: 82 },
  lapsLeft: { left: 288, top: 76, width: 94, height: 82 },
  tyres: { left: 28, top: 174, width: 354, height: 202 },
  gear: { left: 394, top: 80 },
  speed: { left: 406, top: 294 },
  laps: { left: 656, top: 86, width: 318, height: 290 },
  brake: { left: 28, top: 392, width: 116, height: 102 },
  throttle: { left: 154, top: 392, width: 124, height: 102 },
  fuel: { left: 290, top: 392, width: 440, height: 102 },
  clock: { left: 742, top: 392, width: 232, height: 102 },
  chips: { left: 28, top: 508 },
  engineer: { left: 500, right: 26, top: 505 },
  slash: "wide",
};

const PORTRAIT: Layout = {
  w: 540,
  h: 1140,
  brand: { left: 14, top: 10 },
  shift: { left: 24, top: 74, width: 492 },
  badge: { right: 20, top: 20 },
  position: { left: 20, top: 118, width: 160, height: 82 },
  lap: { left: 190, top: 118, width: 160, height: 82 },
  lapsLeft: { left: 360, top: 118, width: 160, height: 82 },
  gear: { left: 14, top: 214 },
  speed: { left: 290, top: 222 },
  brake: { left: 290, top: 322, width: 110, height: 102 },
  throttle: { left: 410, top: 322, width: 110, height: 102 },
  laps: { left: 20, top: 446, width: 500, height: 290 },
  tyres: { left: 20, top: 756, width: 500, height: 202 },
  fuel: { left: 20, top: 976, width: 500, height: 102 },
  clock: null,
  chips: { left: 20, top: 1094 },
  engineer: { left: 20, right: 20, top: 1118 },
  slash: "narrow",
};

/**
 * The landscape canvas made `w` wide for a phone's longer screen (about
 * 2.1 : 1 against the desktop's 1.85 : 1), so the tiles fill it instead of
 * leaving bands at the sides. The extra width goes in thirds: to the left
 * column, the gear and speed in the middle, and the right column.
 */
function widen(L: Layout, w: number): Layout {
  const a = (w - L.w) / 3;
  if (a <= 0) return L;
  const box = (b: Box, dx: number, dw: number): Box => ({ ...b, left: b.left + dx, width: b.width + dw });
  const at = (p: At, dx: number): At => ({ ...p, left: (p.left as number) + dx });
  return {
    ...L,
    w,
    position: box(L.position, 0, a / 3),
    lap: box(L.lap, a / 3, a / 3),
    lapsLeft: box(L.lapsLeft, (2 * a) / 3, a / 3),
    tyres: box(L.tyres, 0, a),
    shift: at(L.shift, 1.5 * a),
    gear: at(L.gear, 1.5 * a),
    speed: at(L.speed, 1.5 * a),
    laps: box(L.laps, 2 * a, a),
    brake: box(L.brake, 0, a / 2),
    throttle: box(L.throttle, a / 2, a / 2),
    fuel: box(L.fuel, a, a),
    clock: L.clock && box(L.clock, 2 * a, a),
  };
}

/** Width ÷ height of an element, kept up to date. */
function useAspect(ref: React.RefObject<HTMLElement | null>) {
  const [aspect, setAspect] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setAspect(el.clientHeight ? el.clientWidth / el.clientHeight : 0));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return aspect;
}

export function Dashboard({ onToggleNav, navOpen }: { onToggleNav: () => void; navOpen: boolean }) {
  const live = useStore((s) => s.live);
  const status = useStore((s) => s.status);
  const units = useStore((s) => s.settings?.units ?? "metric");
  const go = useStore((s) => s.go);
  const isLive = status.state === "live";
  const l = isLive ? live : null;

  const fuelCritical = l != null && ((l.fuelPct != null && l.fuelPct < 15) || l.strategy?.window === "box_now");
  const { compact, portrait } = useViewport();
  const root = useRef<HTMLDivElement>(null);
  const aspect = useAspect(root);
  // Phones on their side: as wide as the screen's shape, up to 2.3 : 1.
  const L = compact && portrait ? PORTRAIT : compact ? widen(LANDSCAPE, Math.round(Math.min(2.3, aspect) * LANDSCAPE.h)) : LANDSCAPE;

  return (
    <div ref={root} className="an-bg relative h-full w-full">
      {fuelCritical && <div className="hit-glow pointer-events-none absolute inset-0 z-40" aria-hidden />}
      <ScaleToFit width={L.w} height={L.h}>
        <HeaderSlash kind={L.slash} width={L.w} />
        <button
          onClick={onToggleNav}
          className="absolute flex items-center gap-[6px] text-white/70 hover:text-white"
          style={L.brand}
          title="Menu"
          aria-label="Menu"
        >
          <Brand scale={0.95} />
          <ChevronDown width={20} height={20} className={`transition ${navOpen ? "rotate-180" : ""}`} />
        </button>

        <ShiftLights l={l} at={L.shift} />
        <StatusBadge status={status} paused={!!l?.paused} onClick={() => go({ name: "connect" })} at={L.badge} />

        <Tile label="Position" color="var(--color-pos)" style={L.position}>
          <Big value={l?.position != null ? `P${l.position}` : DASH} of={l?.numCars} />
        </Tile>
        <Tile label="Lap" color="var(--color-lap)" style={L.lap}>
          <Big value={l?.lap ?? DASH} of={l?.totalLaps || null} />
        </Tile>
        <Tile label="Laps Left" color="var(--color-laps)" style={L.lapsLeft}>
          <Big value={l?.lapsRemaining ?? DASH} />
        </Tile>

        <Tyres l={l} at={L.tyres} />
        <GearPod l={l} at={L.gear} />
        <Speed l={l} units={units} at={L.speed} />
        <LapTimes l={l} at={L.laps} />

        <Pedal label="Brake" color="var(--color-brk)" value={l?.brake} at={L.brake} />
        <Pedal label="Throttle" color="var(--color-thr)" value={l?.throttle} at={L.throttle} />
        <Fuel l={l} at={L.fuel} />
        {L.clock && <Clock at={L.clock} />}

        <Indicators l={l} at={L.chips} />
        <EngineerLine at={L.engineer} />
      </ScaleToFit>
    </div>
  );
}

/** The angled accent line under the logo, like a pit-wall banner. */
function HeaderSlash({ kind, width }: { kind: "wide" | "narrow"; width: number }) {
  return (
    <svg className="pointer-events-none absolute left-0 top-0" width={width} height={70}>
      {kind === "wide" ? (
        <>
          <path d="M0 62 H300 L330 36 H640" fill="none" stroke="rgba(57,255,122,0.35)" strokeWidth="1.2" />
          <path d="M0 66 H296" fill="none" stroke="rgba(57,255,122,0.12)" strokeWidth="1" />
        </>
      ) : (
        <>
          <path d={`M0 62 H260 L284 52 H${width}`} fill="none" stroke="rgba(57,255,122,0.35)" strokeWidth="1.2" />
          <path d="M0 66 H256" fill="none" stroke="rgba(57,255,122,0.12)" strokeWidth="1" />
        </>
      )}
    </svg>
  );
}

/** Big tile number with an optional "/total", shrunk so "P10/20" keeps even side padding. */
function Big({ value, of }: { value: string | number; of?: number | null }) {
  const chars = String(value).length + (of != null ? String(of).length + 1 : 0);
  const size = chars <= 4 ? 44 : chars <= 5 ? 38 : 33;
  return (
    <div className="absolute inset-x-[8px] bottom-[10px] flex items-baseline justify-center gap-[3px]">
      <span className="font-[800] leading-none tracking-tight" style={{ fontSize: size }}>
        {value}
      </span>
      {of != null && <span className="num font-medium text-white/55" style={{ fontSize: Math.round(size * 0.4) }}>/{of}</span>}
    </div>
  );
}

const LIGHT_COLORS = ["#2bea5c", "#2bea5c", "#39ff7a", "#a6f23a", "#ffe03a", "#ffc21a", "#ff8a1a", "#ff3b4a", "#ff3b4a", "#ff2d55", "#ff2d55", "#ff2d55"];

function ShiftLights({ l, at }: { l: LiveState | null; at: At }) {
  const n = LIGHT_COLORS.length;
  let lit = 0;
  let flash = false;
  if (l && l.rpmAlertMax > 0) {
    const start = Math.max(1000, l.rpmAlertMin * 0.78);
    const end = l.rpmAlertMax;
    lit = Math.round(((l.rpm - start) / (end - start)) * n);
    lit = Math.max(0, Math.min(n, lit));
    flash = l.revLimit || l.rpm >= end;
  }
  return (
    <div className="absolute flex justify-between" style={at}>
      {LIGHT_COLORS.map((color, i) => {
        const on = flash || i < lit;
        const c = flash ? "#4aa8ff" : color;
        return (
          <div
            key={i}
            className={`h-[22px] w-[22px] rounded-full ${flash ? "blink" : ""}`}
            style={{
              background: on ? `radial-gradient(circle at 35% 35%, #fff8, ${c} 45%)` : "#0d1211",
              border: `1.5px solid ${on ? c : "#2c3532"}`,
              boxShadow: on ? `0 0 12px ${c}, 0 0 3px ${c}` : "inset 0 0 4px #000",
            }}
          />
        );
      })}
    </div>
  );
}

function StatusBadge({ status, paused, onClick, at }: { status: ConnStatus; paused: boolean; onClick: () => void; at: At }) {
  const map: Record<ConnStatus["state"], [string, string, boolean]> = {
    idle: ["OFFLINE", "#8b9b96", false],
    searching: ["SEARCHING", "#ffc81a", true],
    connecting: ["CONNECTING", "#ffc81a", true],
    live: [paused ? "PAUSED" : `LIVE  ${Math.round(status.hz)}Hz`, paused ? "#ff9f1a" : "#39ff7a", false],
    lost: ["SIGNAL LOST", "#ff9f1a", true],
    error: ["ERROR", "#ff3b4a", false],
  };
  const [text, color, spin] = map[status.state];
  return (
    <button
      onClick={onClick}
      className="num absolute flex h-[30px] items-center gap-[10px] whitespace-pre rounded-full px-[14px] text-[14px] font-bold tracking-[0.06em]"
      style={{ ...at, color, border: `1.5px solid ${color}`, boxShadow: `0 0 10px color-mix(in srgb, ${color} 40%, transparent)` }}
      title={status.error ?? (status.ip ? `PlayStation ${status.ip}` : "Connect")}
    >
      {spin ? (
        <span className="inline-block h-[12px] w-[12px] animate-spin rounded-full border-2 border-white/25" style={{ borderTopColor: color }} />
      ) : (
        <span className="inline-block h-[10px] w-[10px] rounded-full" style={{ background: color, boxShadow: `0 0 8px ${color}` }} />
      )}
      {text}
    </button>
  );
}

function tyreState(t: number): string {
  if (t <= 0) return "";
  if (t < 50) return "COLD";
  if (t < 65) return "WARMING";
  if (t < 92) return "OPTIMAL";
  if (t < 100) return "WARM";
  return "HOT";
}

function Tyres({ l, at }: { l: LiveState | null; at: Box }) {
  const t = l?.tyreTemp;
  const corner = (i: number, side: "left" | "right", top: number) => {
    const temp = t?.[i] ?? 0;
    const color = t ? tyreColor(temp) : "#3a4441";
    const fill = t ? Math.max(8, Math.min(100, ((temp - 20) / 90) * 100)) : 0;
    const textX = side === "left" ? { left: 14, textAlign: "right" as const } : { right: 14, textAlign: "left" as const };
    const barX = side === "left" ? { left: 116 } : { right: 116 };
    return (
      <>
        <div className="tech absolute text-[13px] text-white/55" style={{ top: top - 12, [side]: 14 }}>
          {["FL", "FR", "RL", "RR"][i]}
        </div>
        <div className="absolute w-[86px]" style={{ top: top + 12, ...textX }}>
          <div className="num text-[28px] font-bold leading-none">
            {t ? Math.round(temp) : DASH}
            {t && <span className="align-top text-[15px] text-white/60">°</span>}
          </div>
          <div className="tech mt-[6px] text-[10px] leading-none" style={{ color: t ? color : "#5b6662" }}>
            {t ? tyreState(temp) : "—"}
          </div>
        </div>
        <div
          className="absolute h-[74px] w-[20px] overflow-hidden rounded-[6px] border border-white/10 bg-[#0b100f]"
          style={{ top, ...barX, boxShadow: t ? `0 0 10px color-mix(in srgb, ${color} 50%, transparent)` : undefined }}
        >
          <div
            className="absolute inset-x-[3px] bottom-[3px] rounded-[4px] transition-[height] duration-300"
            style={{ height: `calc(${fill}% - 6px)`, background: `linear-gradient(180deg, #fff6, ${color} 30%, ${color})` }}
          />
        </div>
      </>
    );
  };
  return (
    <Tile notch label="Tyre Temperatures" color="#c9d2cf" style={at}>
      <div className="absolute inset-x-[14px] top-[100px] h-px bg-white/[0.07]" />
      <CarTopDown className="absolute top-[14px]" style={{ left: (at.width - 82) / 2 }} />
      {corner(0, "left", 24)}
      {corner(1, "right", 24)}
      {corner(2, "left", 112)}
      {corner(3, "right", 112)}
    </Tile>
  );
}

/**
 * Top-down open-wheel (F1-style) car for the tyre panel. It spans the gap
 * between the tyre bars, and its suspension arms reach out to them, so the
 * temperature bars read as the wheels.
 */
function CarTopDown({ className, style }: { className?: string; style?: CSSProperties }) {
  const arm = { stroke: "#7d8d89", strokeWidth: 1.6, strokeLinecap: "round" as const };
  return (
    <svg className={className} style={style} width="82" height="172" viewBox="0 0 82 172" fill="none">
      <defs>
        <linearGradient id="car-body" x1="0" x2="1">
          <stop offset="0" stopColor="#1b2422" />
          <stop offset="0.5" stopColor="#33413e" />
          <stop offset="1" stopColor="#1b2422" />
        </linearGradient>
      </defs>
      {/* Suspension: front wheels centre at y 47, rear at y 135. */}
      <path d="M36 38 L1 44 M35 54 L1 50 M46 38 L81 44 M47 54 L81 50" {...arm} />
      <path d="M31 126 L1 132 M31 142 L1 138 M51 126 L81 132 M51 142 L81 138" {...arm} />
      <g style={{ filter: "drop-shadow(0 0 5px rgba(57,255,122,0.55))" }} stroke="#39ff7a" strokeOpacity="0.7" strokeWidth="1.3">
        {/* Front wing */}
        <path d="M5 2 H77 V7 Q41 12 5 7 Z" fill="url(#car-body)" />
        {/* Nose, monocoque, sidepods and engine cover */}
        <path
          d="M38 9 L44 9 C45 24 47 40 48 54 L50 62 C58 70 64 80 64 92 C64 106 60 112 54 118 C50 124 48 136 47 152 L46 158 L36 158 L35 152 C34 136 32 124 28 118 C22 112 18 106 18 92 C18 80 24 70 32 62 L34 54 C35 40 37 24 38 9 Z"
          fill="url(#car-body)"
        />
        {/* Rear wing */}
        <path d="M14 159 H68 V167 H14 Z" fill="url(#car-body)" />
      </g>
      {/* Wing endplates */}
      <path d="M4 0 V10 M78 0 V10 M13 156 V170 M69 156 V170" stroke="#9fb1ac" strokeWidth="1.6" strokeLinecap="round" />
      {/* Cockpit and halo */}
      <ellipse cx="41" cy="68" rx="5.5" ry="9" fill="#0a0f0e" stroke="#5d6b67" strokeWidth="1" />
      <path d="M34.5 62 Q41 54 47.5 62 M41 56.5 V61" stroke="#9fb1ac" strokeWidth="1.4" strokeLinecap="round" fill="none" />
      {/* Airbox and sidepod inlets */}
      <path d="M38 80 H44 V88 H38 Z" fill="#0a0f0e" stroke="#3a4643" strokeWidth="0.8" />
      <path d="M21 84 H28 M54 84 H61" stroke="#0a0f0e" strokeWidth="2.4" strokeLinecap="round" />
      {/* Rain light */}
      <path d="M38.5 170 H43.5" stroke="#ff3b4a" strokeWidth="2.2" strokeLinecap="round" />
    </svg>
  );
}

/** RPM arc segments either side of the gear: green build-up left, red shift zone right. */
function RpmArc({ side, lit, colors }: { side: "left" | "right"; lit: number; colors: (i: number) => string }) {
  const N = 12;
  const cx = 126;
  const cy = 105;
  const r = 112;
  const segs = [];
  for (let i = 0; i < N; i++) {
    // Bottom to top, -38° to +38° around the horizontal.
    const a0 = (-38 + (76 * i) / N + 1.2) * (Math.PI / 180);
    const a1 = (-38 + (76 * (i + 1)) / N - 1.2) * (Math.PI / 180);
    const dir = side === "left" ? -1 : 1;
    const p = (a: number, rr: number) => [cx + dir * rr * Math.cos(a), cy - rr * Math.sin(a)];
    const [x0, y0] = p(a0, r);
    const [x1, y1] = p(a1, r);
    const [x2, y2] = p(a1, r - 11);
    const [x3, y3] = p(a0, r - 11);
    const on = i < lit;
    const c = colors(i);
    segs.push(
      <path
        key={i}
        d={`M${x0},${y0} L${x1},${y1} L${x2},${y2} L${x3},${y3} Z`}
        fill={on ? c : "#151c1b"}
        stroke={on ? c : "#232c2a"}
        strokeWidth="0.8"
        style={on ? { filter: `drop-shadow(0 0 4px ${c})` } : undefined}
      />,
    );
  }
  return <>{segs}</>;
}

function GearPod({ l, at }: { l: LiveState | null; at: At }) {
  const gear = l ? gearLabel(l.gear, l.inGear) : "N";
  const shiftUp = !!l && l.suggestedGear != null && l.inGear && l.suggestedGear > l.gear;
  const N = 12;
  let left = 0;
  let right = 0;
  if (l && l.rpmAlertMax > 0) {
    left = Math.round(Math.min(1, l.rpm / Math.max(1, l.rpmAlertMin)) * N);
    right = Math.round(Math.max(0, Math.min(1, (l.rpm - l.rpmAlertMin) / Math.max(1, l.rpmAlertMax - l.rpmAlertMin))) * N);
  }
  return (
    <div className="absolute h-[212px] w-[252px]" style={at}>
      <svg className="absolute inset-0" width="252" height="212" viewBox="0 0 252 212">
        <defs>
          <linearGradient id="pod" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#121a18" />
            <stop offset="1" stopColor="#070a09" />
          </linearGradient>
        </defs>
        <path d="M54 2 H198 L232 30 V186 L206 210 H46 L20 186 V30 Z" fill="url(#pod)" stroke="#2b3734" strokeWidth="1.5" />
        <path d="M76 10 H176 L196 30 V166 H56 V30 Z" fill="#0a0f0e" stroke="#1f2a27" strokeWidth="1" />
        <RpmArc side="left" lit={left} colors={(i) => (i < 8 ? "#2bea5c" : i < 10 ? "#a6f23a" : "#ffe03a")} />
        <RpmArc side="right" lit={right} colors={() => "#ff3b4a"} />
        <path d="M80 172 H172" stroke="#3a4643" strokeWidth="1.2" />
      </svg>
      <div className="absolute inset-x-0 top-[8px] text-center text-[150px] font-[900] leading-none tracking-tight" style={{ textShadow: "0 0 18px rgba(255,255,255,0.18)" }}>
        {gear}
      </div>
      {shiftUp && (
        <div className="num absolute right-[64px] top-[16px] rounded-[4px] bg-[#b44cff]/25 px-[5px] text-[15px] font-bold text-[#d9a6ff]">
          ↑{l!.suggestedGear}
        </div>
      )}
      <div className="absolute inset-x-0 top-[178px] text-center leading-none">
        <span className="num text-[22px] font-bold text-white/85">{l ? Math.round(l.rpm) : 0}</span>
        <span className="tech ml-[6px] text-[12px] text-white/50">RPM</span>
      </div>
    </div>
  );
}

function Speed({ l, units, at }: { l: LiveState | null; units: "metric" | "imperial"; at: At }) {
  return (
    <div className="absolute h-[84px] w-[228px]" style={at}>
      <svg className="absolute inset-0" width="228" height="84" viewBox="0 0 228 84" style={{ filter: "drop-shadow(0 0 8px rgba(57,255,122,0.6))" }}>
        <path d="M24 2 H204 L226 26 V58 L204 82 H24 L2 58 V26 Z" fill="#070b0a" stroke="#39ff7a" strokeWidth="2.2" />
      </svg>
      <div className="relative flex h-full flex-col items-center justify-center leading-none">
        <span className="text-[50px] font-[800] tracking-tight">{l ? speed(l.speedKph, units) : 0}</span>
        <span className="tech mt-[2px] text-[12px] text-white/60">{units === "imperial" ? "MPH" : "KPH"}</span>
      </div>
    </div>
  );
}

function LapTimes({ l, at }: { l: LiveState | null; at: Box }) {
  const d = l?.deltaMs;
  // Remember how much the last personal best improved on the one before.
  const prevBest = useRef<number | null>(null);
  const [gain, setGain] = useState<number | null>(null);
  const best = l?.bestLapMs ?? null;
  useEffect(() => {
    if (best == null) return;
    if (prevBest.current != null && best < prevBest.current) setGain(best - prevBest.current);
    prevBest.current = best;
  }, [best]);

  const last = l?.lastLapMs ?? null;
  const isPb = last != null && best != null && last === best;
  const lastDelta = last != null && best != null && last > 0 ? (isPb ? gain : last - best) : null;

  const row = (label: string, ms: number | null | undefined, top: number, extra: React.ReactNode) => (
    <div className="absolute inset-x-[14px] h-[80px] rounded-[10px] border border-white/[0.07] bg-[#0b1110]/90" style={{ top }}>
      <div className="tech absolute left-[16px] top-[11px] text-[12px] text-white/70">{label}</div>
      <div className="num absolute left-[14px] top-[30px] text-[36px] font-bold leading-none tracking-tight">{lapClock(ms)}</div>
      <div className="num absolute right-[14px] top-[38px] text-[17px] font-bold leading-none">{extra}</div>
    </div>
  );
  return (
    <Tile notch label="Lap Times" color="var(--color-laptime)" style={at}>
      {row(
        "Current",
        l?.currentLapMs,
        18,
        l?.lapInvalid ? (
          <span className="tech text-[12px] text-[#ff3b4a]">Invalid</span>
        ) : d != null ? (
          <span style={{ color: d <= 0 ? "#39ff7a" : "#ff3b4a" }}>{delta(d)}</span>
        ) : (
          <span className="text-white/40">---</span>
        ),
      )}
      {row(
        "Last",
        last,
        106,
        lastDelta != null ? <span style={{ color: lastDelta <= 0 ? "#39ff7a" : "#ff9f1a" }}>{delta(lastDelta)}</span> : <span className="text-white/40">---</span>,
      )}
      {row(
        "Best",
        best,
        194,
        best != null ? (
          <span className="tech rounded-[5px] border border-[#b44cff] px-[7px] py-[3px] text-[12px] text-[#d9a6ff] shadow-[0_0_8px_rgba(180,76,255,0.5)]">PB</span>
        ) : null,
      )}
    </Tile>
  );
}

function Pedal({ label, color, value, at }: { label: string; color: string; value: number | undefined; at: Box }) {
  const N = 14;
  const lit = value != null ? Math.round((Math.min(100, value) / 100) * N) : 0;
  return (
    <Tile label={label} color={color} style={at}>
      <div
        className="absolute inset-x-[10px] bottom-[10px] top-[26px] rounded-[8px] border"
        style={{ borderColor: `color-mix(in srgb, ${color} 35%, transparent)`, background: `color-mix(in srgb, ${color} 7%, #050807)` }}
      >
        <div className="absolute inset-x-0 top-[6px] text-center text-[34px] font-[800] leading-none">{value ?? DASH}</div>
        <div className="absolute inset-x-[7px] bottom-[6px] flex h-[16px] gap-[2px]">
          {Array.from({ length: N }, (_, i) => (
            <div
              key={i}
              className="flex-1 rounded-[1px]"
              style={{ background: i < lit ? color : `color-mix(in srgb, ${color} 18%, #0b0f0e)`, boxShadow: i < lit ? `0 0 5px ${color}` : undefined }}
            />
          ))}
        </div>
      </div>
    </Tile>
  );
}

const PIT_LABEL: Partial<Record<PitWindow, [string, string]>> = {
  closed: ["PIT WINDOW CLOSED", "#4b5552"],
  open: ["PIT WINDOW OPEN", "#16a34a"],
  box_now: ["BOX THIS LAP", "#dc2626"],
};

/** Pit badge text, colour, and whether it should blink. */
function pitBadge(l: LiveState | null): [string, string, boolean] | undefined {
  const s = l?.strategy;
  if (!l || !s) return undefined;
  const plan = l.pitPlan;
  if (s.window === "box_now") return ["BOX THIS LAP", "#dc2626", true];
  if (plan && l.lap != null && plan.lap <= l.lap) {
    return [plan.driver === "tyres" ? "TYRE STOP ADVISED" : `FUEL STOP BY L${plan.fuelLatest}`, "#b9770e", false];
  }
  if (s.stopsRequired > 0 && s.earliestPitLap != null && s.latestPitLap != null) {
    const target = plan?.lap ?? s.targetPitLap;
    return [`PIT L${s.earliestPitLap}–${s.latestPitLap} · TARGET L${target}`, s.window === "open" ? "#16a34a" : "#4b5552", false];
  }
  const label = l.pitWindow ? PIT_LABEL[l.pitWindow] : undefined;
  return label && [label[0], label[1], false];
}

function fuelColor(pct: number) {
  if (pct < 15) return "#ff3b4a";
  if (pct < 30) return "#ffc81a";
  return "#39ff7a";
}

function Fuel({ l, at }: { l: LiveState | null; at: Box }) {
  const fmt = (v: number | null | undefined, digits = 0, signed = false) =>
    v == null ? DASH : `${signed && v > 0 ? "+" : ""}${v.toFixed(digits)}`;
  const pit = pitBadge(l);
  const pct = l?.fuelPct ?? null;
  const c = pct != null ? fuelColor(pct) : "#39ff7a";
  const margin = l?.strategy?.fuelMarginLaps;
  const col = (value: string, label: string, color?: string) => (
    <div className="flex-1 text-center">
      <div className="num text-[24px] font-bold leading-none" style={{ color }}>
        {value}
      </div>
      <div className="tech mt-[6px] text-[11px] leading-none text-white/60">{label}</div>
    </div>
  );
  return (
    <Tile notch label="Fuel" color="var(--color-fuel)" style={{ ...at, borderRadius: 18 }}>
      <svg className="absolute left-[18px] top-[16px]" width="26" height="30" viewBox="0 0 24 28" fill="none" style={{ filter: "drop-shadow(0 0 4px #39ff7a)" }}>
        <path d="M3 26V4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v22z" fill="#39ff7a" />
        <path d="M5 5h8v6H5z" fill="#06100a" />
        <path d="M15 9h3l2 3v10a1.5 1.5 0 0 1-3 0v-5h-2" stroke="#39ff7a" strokeWidth="1.8" strokeLinejoin="round" />
        <path d="M1 26h16" stroke="#39ff7a" strokeWidth="2" />
      </svg>
      <div className="absolute left-[58px] right-[92px] top-[20px] h-[22px] rounded-[6px] border border-white/10 bg-[#0b100f] p-[3px]">
        <div
          className="h-full rounded-[4px] transition-[width] duration-100"
          style={{ width: `${Math.max(0, Math.min(100, pct ?? 0))}%`, background: `linear-gradient(90deg, ${c}, color-mix(in srgb, ${c} 70%, #fff))`, boxShadow: `0 0 10px ${c}` }}
        />
      </div>
      <div title="Reported tank level; no strategy reserve is subtracted" className="num absolute right-[16px] top-[18px] text-[22px] font-bold leading-none">{pct != null ? `${pct.toFixed(1)}%` : DASH}</div>
      <div className="absolute inset-x-[30px] top-[56px] flex items-center">
        {col(l?.fuelPerLapPct != null ? `${fmt(l.fuelPerLapPct, 1)}%` : DASH, "Per Lap")}
        <div className="h-[30px] w-px bg-white/15" />
        {col(fmt(l?.fuelLapsLeft, 1), "Fuel Laps")}
        <div className="h-[30px] w-px bg-white/15" />
        {col(fmt(margin, 1, true), "Margin", margin == null ? undefined : margin < 0 ? "#ff3b4a" : "#39ff7a")}
      </div>
      {pit && (
        <div
          className={`num absolute -top-[9px] right-[18px] z-20 whitespace-nowrap rounded-[4px] px-[7px] py-[1px] text-[11px] font-bold ${pit[2] ? "blink" : ""}`}
          style={{ background: pit[1] }}
        >
          {pit[0]}
        </div>
      )}
    </Tile>
  );
}

function Clock({ at }: { at: Box }) {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 5000);
    return () => clearInterval(id);
  }, []);
  const text = now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const m = text.match(/^(.*?)\s*([AaPp]\.?\s?[Mm]\.?)$/);
  const [time, suffix] = m ? [m[1], m[2].toUpperCase()] : [text, ""];
  const h = now.getHours();
  const day = h >= 6 && h < 19;
  return (
    <Tile notch label="Time" color="#e6ecea" style={{ ...at, borderRadius: 18 }}>
      <div className="absolute right-[14px] top-[12px]" title={day ? "Day" : "Night"}>
        {day ? (
          <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="#ffc81a" strokeWidth="2" strokeLinecap="round" style={{ filter: "drop-shadow(0 0 4px #ffc81a)" }}>
            <circle cx="12" cy="12" r="4" fill="#ffc81a" />
            <path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
          </svg>
        ) : (
          <svg width="20" height="20" viewBox="0 0 24 24" fill="#9fc3ff" style={{ filter: "drop-shadow(0 0 4px #9fc3ff)" }}>
            <path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z" />
          </svg>
        )}
      </div>
      <div className="absolute inset-0 flex items-center justify-center gap-[8px] pt-[6px]">
        <span className="text-[50px] font-[800] leading-none tracking-tight">{time}</span>
        {suffix && <span className="mt-[16px] text-[24px] font-bold leading-none text-white/60">{suffix}</span>}
      </div>
    </Tile>
  );
}

/** Small status chips for the driver aids and warnings. */
function Indicators({ l, at }: { l: LiveState | null; at: At }) {
  const pit = l?.pitWindow;
  const chips: [string, boolean, string, string][] = [
    ["TCS", !!l?.tcs, "#ff9f1a", "Traction control active"],
    ["ASM", !!l?.asm, "#ff9f1a", "Stability management active"],
    ["PIT", pit === "open", "#39ff7a", "Pit window open"],
    ["BOX", pit === "box_now", "#ff3b4a", "Box this lap"],
    ["AHEAD", l?.deltaMs != null && l.deltaMs < 0, "#b44cff", "Ahead of best lap"],
    ["HOT", !!l && l.tyreTemp.some((t) => t > 100), "#ff3b4a", "Tyres overheating"],
    ["SPIN", !!l && l.throttle > 50 && Math.max(l.wheelSlip[2], l.wheelSlip[3]) > 0.15, "#ff8a1a", "Wheelspin"],
    ["LOCK", !!l && l.brake > 50 && Math.min(l.wheelSlip[0], l.wheelSlip[1]) < -0.2, "#ff8a1a", "Front lock-up"],
  ];
  return (
    <div className="absolute flex gap-[6px]" style={at}>
      {chips.map(([label, on, color, title]) => (
        <div
          key={label}
          title={title}
          className="tech rounded-[5px] px-[8px] py-[3px] text-[11px] font-semibold leading-none"
          style={{
            color: on ? "#050807" : "rgba(255,255,255,0.28)",
            background: on ? color : "transparent",
            border: `1px solid ${on ? color : "rgba(255,255,255,0.12)"}`,
            boxShadow: on ? `0 0 10px ${color}` : undefined,
          }}
        >
          {label}
        </div>
      ))}
    </div>
  );
}

function EngineerLine({ at }: { at: At }) {
  const msg = useStore((s) => s.engineer);
  const voiceError = useStore((s) => s.voiceError);
  const go = useStore((s) => s.go);
  const [, force] = useState(0);
  useEffect(() => {
    if (!msg) return;
    const id = setTimeout(() => force((n) => n + 1), 6000);
    return () => clearTimeout(id);
  }, [msg]);
  const box = "absolute flex justify-end";
  if (voiceError && (!msg || Date.now() - msg.at > 6000)) {
    return (
      <button onClick={() => go({ name: "settings" })} className={box} style={at}>
        <span className="truncate rounded-full border border-red-500/40 bg-red-500/15 px-[12px] py-[3px] text-[13px] font-medium text-red-300">
          Voice off · {voiceError} (click to fix)
        </span>
      </button>
    );
  }
  if (!msg || Date.now() - msg.at > 6000) return null;
  return (
    <div className={box} style={at}>
      <span className="truncate rounded-full border border-accent/40 bg-accent/10 px-[12px] py-[3px] text-[13px] font-medium text-[#b6ffcf]">
        <span className="tech mr-[6px] text-[11px] text-accent">Engineer</span>
        {msg.text}
      </span>
    </div>
  );
}