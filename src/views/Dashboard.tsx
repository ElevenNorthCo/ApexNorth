import { useEffect, useState } from "react";
import type { ConnStatus, LiveState, PitWindow } from "../lib/api";
import { DASH, clock, delta, gearLabel, lapClock, speed, tyreColor } from "../lib/format";
import { useStore } from "../lib/store";
import { ScaleToFit } from "../components/ScaleToFit";
import { Tile } from "../components/Tile";
import { ChevronDown } from "../components/icons";

const W = 1000;
const H = 462;

export function Dashboard({ onToggleNav }: { onToggleNav: () => void }) {
  const live = useStore((s) => s.live);
  const status = useStore((s) => s.status);
  const units = useStore((s) => s.settings?.units ?? "metric");
  const go = useStore((s) => s.go);
  const isLive = status.state === "live";
  const l = isLive ? live : null;

  return (
    <div className="h-full w-full bg-black">
      <ScaleToFit width={W} height={H}>
        <button
          onClick={onToggleNav}
          className="absolute left-[16px] top-[18px] text-white/90 hover:text-white"
          title="Menu"
        >
          <ChevronDown width={30} height={30} />
        </button>

        <ShiftLights l={l} />
        <StatusBadge status={status} paused={!!l?.paused} onClick={() => go({ name: "connect" })} />
        <SideLights l={l} />

        <Tile label="POS" color="var(--color-pos)" style={{ left: 104, top: 50, width: 91, height: 83 }}>
          <Big>{l?.position ?? DASH}</Big>
        </Tile>
        <Tile label="LAP" color="var(--color-lap)" style={{ left: 204, top: 50, width: 91, height: 83 }}>
          <Big>{l?.lap ?? DASH}</Big>
        </Tile>
        <Tile label="LAPS" color="var(--color-laps)" style={{ left: 304, top: 50, width: 91, height: 83 }}>
          <Big>{l?.totalLaps ?? DASH}</Big>
        </Tile>

        <Tyres l={l} />
        <GearBlock l={l} units={units} />
        <LapTimes l={l} />

        <Tile label="BRK" color="var(--color-brk)" style={{ left: 104, top: 349, width: 96, height: 82 }}>
          <Fill pct={l?.brake ?? 0} color="var(--color-brk)" />
          <Big>{l ? l.brake : DASH}</Big>
        </Tile>
        <Tile label="THROT" color="var(--color-thr)" style={{ left: 209, top: 349, width: 96, height: 82 }}>
          <Fill pct={l?.throttle ?? 0} color="var(--color-thr)" />
          <Big>{l ? l.throttle : DASH}</Big>
        </Tile>
        <Fuel l={l} />
        <Clock />
        <EngineerLine />
      </ScaleToFit>
    </div>
  );
}

function Big({ children }: { children: React.ReactNode }) {
  return (
    <div className="num relative flex h-full items-center justify-center text-[40px] font-bold leading-none">
      {children}
    </div>
  );
}

function Fill({ pct, color }: { pct: number; color: string }) {
  return (
    <div
      className="absolute inset-x-[4px] bottom-[4px] rounded-[7px] transition-[height] duration-75"
      style={{ height: `calc(${Math.min(100, pct)}% - 8px)`, background: color, opacity: 0.22 }}
    />
  );
}

const LIGHT_COUNT = 16;
function lightColor(i: number) {
  if (i < 6) return "#22c55e";
  if (i < 11) return "#eab308";
  return "#ef4444";
}

function ShiftLights({ l }: { l: LiveState | null }) {
  let lit = 0;
  let flash = false;
  if (l && l.rpmAlertMax > 0) {
    const start = Math.max(1000, l.rpmAlertMin * 0.78);
    const end = l.rpmAlertMax;
    lit = Math.round(((l.rpm - start) / (end - start)) * LIGHT_COUNT);
    lit = Math.max(0, Math.min(LIGHT_COUNT, lit));
    flash = l.revLimit || l.rpm >= end;
  }
  return (
    <div className="absolute left-[223px] top-[7px] flex w-[554px] justify-between">
      {Array.from({ length: LIGHT_COUNT }, (_, i) => {
        const on = flash || i < lit;
        const c = flash ? "#7c5cff" : lightColor(i);
        return (
          <div
            key={i}
            className={`h-[20px] w-[20px] rounded-full ${flash ? "blink" : ""}`}
            style={{
              background: on ? c : "#1e1e1e",
              border: `1.5px solid ${on ? c : "#3a3a3a"}`,
              boxShadow: on ? `0 0 10px ${c}` : undefined,
            }}
          />
        );
      })}
    </div>
  );
}

function StatusBadge({ status, paused, onClick }: { status: ConnStatus; paused: boolean; onClick: () => void }) {
  const map: Record<ConnStatus["state"], [string, string, boolean]> = {
    idle: ["OFFLINE", "#8b8f9c", false],
    searching: ["SEARCHING", "#f2e14c", true],
    connecting: ["CONNECTING", "#f2e14c", true],
    live: [paused ? "PAUSED" : `LIVE ${Math.round(status.hz)}HZ`, paused ? "#f59e0b" : "#22c55e", false],
    lost: ["SIGNAL LOST", "#f59e0b", true],
    error: ["ERROR", "#ef4444", false],
  };
  const [text, color, spin] = map[status.state];
  return (
    <button
      onClick={onClick}
      className="num absolute right-[50px] top-[9px] flex items-center gap-[8px] text-[15px] font-bold tracking-[0.05em]"
      style={{ color }}
      title={status.error ?? (status.ip ? `PlayStation ${status.ip}` : "Connect")}
    >
      {spin ? (
        <span
          className="inline-block h-[14px] w-[14px] animate-spin rounded-full border-2 border-white/25"
          style={{ borderTopColor: color }}
        />
      ) : (
        <span className="inline-block h-[10px] w-[10px] rounded-full" style={{ background: color }} />
      )}
      {text}
    </button>
  );
}

function Led({ on, color, title, y, x }: { on: boolean; color: string; title: string; y: number; x: number }) {
  return (
    <div
      title={title}
      className="absolute h-[20px] w-[20px] rounded-full"
      style={{
        left: x - 10,
        top: y - 10,
        background: on ? color : "#1e1e1e",
        border: `1.5px solid ${on ? color : "#3a3a3a"}`,
        boxShadow: on ? `0 0 10px ${color}` : undefined,
      }}
    />
  );
}

function SideLights({ l }: { l: LiveState | null }) {
  const pit = l?.pitWindow;
  const hot = !!l && l.tyreTemp.some((t) => t > 100);
  const spin = !!l && l.throttle > 50 && Math.max(l.wheelSlip[2], l.wheelSlip[3]) > 0.15;
  const lock = !!l && l.brake > 50 && Math.min(l.wheelSlip[0], l.wheelSlip[1]) < -0.2;
  const ys = [161, 200, 239, 278];
  return (
    <>
      <Led x={73} y={ys[0]} on={!!l?.tcs} color="#f59e0b" title="Traction control active" />
      <Led x={73} y={ys[1]} on={!!l?.asm} color="#f59e0b" title="Stability management active" />
      <Led x={73} y={ys[2]} on={pit === "open"} color="#22c55e" title="Pit window open" />
      <Led x={73} y={ys[3]} on={pit === "box_now"} color="#ef4444" title="Box this lap" />
      <Led x={926} y={ys[0]} on={l?.deltaMs != null && l.deltaMs < 0} color="#a855f7" title="Ahead of best lap" />
      <Led x={926} y={ys[1]} on={hot} color="#ef4444" title="Tyres overheating" />
      <Led x={926} y={ys[2]} on={spin} color="#f97316" title="Wheelspin" />
      <Led x={926} y={ys[3]} on={lock} color="#f97316" title="Front lock-up" />
    </>
  );
}

function Tyres({ l }: { l: LiveState | null }) {
  const t = l?.tyreTemp;
  const label = (i: number) => (t ? `${Math.round(t[i])}` : DASH);
  const tyre = (i: number, x: number, y: number) => (
    <div
      className="absolute h-[72px] w-[30px] rounded-[4px] p-[4px]"
      style={{ left: x, top: y, background: "#8d8d8d" }}
    >
      <div className="h-full w-full rounded-[2px]" style={{ background: t ? tyreColor(t[i]) : "#555" }} />
    </div>
  );
  const temp = (i: number, x: number, y: number, align: "right" | "left") => (
    <div
      className="num absolute w-[60px] text-[24px] font-bold leading-none"
      style={{ left: x, top: y, textAlign: align }}
    >
      {label(i)}
      {t && <span className="text-[13px] text-white/60">°</span>}
    </div>
  );
  return (
    <Tile label="TYRES" color="#ffffff" style={{ left: 104, top: 150, width: 291, height: 182 }}>
      {tyre(0, 101, 14)}
      {tyre(1, 161, 14)}
      {tyre(2, 101, 96)}
      {tyre(3, 161, 96)}
      <div className="absolute left-[137px] top-[32px] h-[42px] w-[6px] rounded bg-[#8d8d8d]" />
      <div className="absolute left-[149px] top-[32px] h-[42px] w-[6px] rounded bg-[#8d8d8d]" />
      <div className="absolute left-[137px] top-[114px] h-[42px] w-[6px] rounded bg-[#8d8d8d]" />
      <div className="absolute left-[149px] top-[114px] h-[42px] w-[6px] rounded bg-[#8d8d8d]" />
      {temp(0, 30, 38, "right")}
      {temp(1, 200, 38, "left")}
      {temp(2, 30, 120, "right")}
      {temp(3, 200, 120, "left")}
    </Tile>
  );
}

function GearBlock({ l, units }: { l: LiveState | null; units: "metric" | "imperial" }) {
  const gear = l ? gearLabel(l.gear, l.inGear) : "N";
  const shiftUp = !!l && l.suggestedGear != null && l.inGear && l.suggestedGear > l.gear;
  return (
    <>
      <div className="absolute left-[403px] top-[49px] h-[225px] w-[193px] rounded-[18px] bg-[#1b1b1b]">
        <div className="absolute inset-x-0 top-[8px] text-center text-[176px] font-[800] leading-none tracking-tight">
          {gear}
        </div>
        {shiftUp && (
          <div className="num absolute right-[12px] top-[10px] text-[18px] font-bold text-[#a855f7]">
            ↑{l!.suggestedGear}
          </div>
        )}
        <div className="num absolute inset-x-0 bottom-[10px] text-center text-[22px] font-bold">
          {l ? Math.round(l.rpm) : 0}
        </div>
      </div>
      <div className="num absolute left-[403px] top-[281px] flex h-[50px] w-[193px] items-center justify-center rounded-[14px] bg-white text-[38px] font-bold text-black">
        {l ? speed(l.speedKph, units) : 0}
        <span className="ml-[6px] mt-[10px] text-[12px] font-bold text-black/50">{units === "imperial" ? "MPH" : "KPH"}</span>
      </div>
    </>
  );
}

function LapTimes({ l }: { l: LiveState | null }) {
  const d = l?.deltaMs;
  const row = (ms: number | null | undefined, label: string, top: number, extra?: React.ReactNode) => (
    <div className="absolute inset-x-[18px] text-center" style={{ top }}>
      <div className="num text-[44px] font-bold leading-none tracking-tight">{lapClock(ms)}</div>
      <div className="num mt-[8px] flex items-center justify-end gap-[10px] pr-[4px] text-[12px] tracking-[0.12em] text-white/60">
        {extra}
        {label}
      </div>
    </div>
  );
  return (
    <Tile label="LAP TIME" color="var(--color-laptime)" style={{ left: 605, top: 50, width: 290, height: 282 }}>
      {row(
        l?.currentLapMs,
        "CURRENT",
        28,
        d != null && (
          <span
            className="rounded-[4px] px-[6px] py-[1px] text-[13px] font-bold tracking-normal"
            style={{ background: d <= 0 ? "#16a34a" : "#dc2626", color: "white" }}
          >
            {delta(d)}
          </span>
        ),
      )}
      {row(l?.lastLapMs, "LAST", 106)}
      {row(l?.bestLapMs, "BEST", 184)}
    </Tile>
  );
}

const PIT_LABEL: Partial<Record<PitWindow, [string, string]>> = {
  closed: ["PIT WINDOW CLOSED", "#6b7280"],
  open: ["PIT WINDOW OPEN", "#16a34a"],
  box_now: ["BOX THIS LAP", "#dc2626"],
};

/** Pit badge text, colour, and whether it should blink. */
function pitBadge(l: LiveState | null): [string, string, boolean] | undefined {
  const s = l?.strategy;
  if (!l || !s) return undefined;
  const plan = l.pitPlan;
  if (s.window === "box_now" || (plan && l.lap != null && plan.lap <= l.lap)) return ["BOX THIS LAP", "#dc2626", true];
  if (s.stopsRequired > 0 && s.earliestPitLap != null && s.latestPitLap != null) {
    const target = plan?.lap ?? s.targetPitLap;
    return [`PIT L${s.earliestPitLap}–${s.latestPitLap} · TARGET L${target}`, s.window === "open" ? "#16a34a" : "#6b7280", false];
  }
  const label = l.pitWindow ? PIT_LABEL[l.pitWindow] : undefined;
  return label && [label[0], label[1], false];
}

function Fuel({ l }: { l: LiveState | null }) {
  const pct = (v: number | null | undefined, digits = 0) => (v == null ? DASH : v.toFixed(digits));
  const pit = pitBadge(l);
  const col = (value: string, unit: string, label: string, x: number) => (
    <div className="absolute w-[110px] text-center" style={{ left: x, top: 13 }}>
      <div className="num text-[32px] font-bold leading-none">
        {value}
        {unit && <span className="ml-[6px] text-[16px] text-white/70">{unit}</span>}
      </div>
      <div className="num mt-[8px] text-[12px] leading-none tracking-[0.12em] text-white/60">{label}</div>
    </div>
  );
  return (
    <Tile label="FUEL" color="var(--color-fuel)" style={{ left: 313, top: 349, width: 382, height: 82 }}>
      {col(pct(l?.fuelPct), "%", "FUEL LEFT", 14)}
      {col(pct(l?.fuelPerLapPct, 1), "%", "PER LAP", 136)}
      {col(pct(l?.fuelLapsLeft, 1), "", "LAPS LEFT", 258)}
      {pit && (
        <div
          className={`num absolute -bottom-[10px] right-[14px] rounded-[4px] px-[6px] text-[11px] font-bold ${pit[2] ? "blink" : ""}`}
          style={{ background: pit[1] }}
        >
          {pit[0]}
        </div>
      )}
    </Tile>
  );
}

function Clock() {
  const [now, setNow] = useState(() => clock());
  useEffect(() => {
    const id = setInterval(() => setNow(clock()), 5000);
    return () => clearInterval(id);
  }, []);
  return (
    <Tile label="TIME" color="#ffffff" style={{ left: 703, top: 349, width: 192, height: 82 }}>
      <Big>
        <span className="text-[36px]">{now}</span>
      </Big>
    </Tile>
  );
}

function EngineerLine() {
  const msg = useStore((s) => s.engineer);
  const voiceError = useStore((s) => s.voiceError);
  const go = useStore((s) => s.go);
  const [, force] = useState(0);
  useEffect(() => {
    if (!msg) return;
    const id = setTimeout(() => force((n) => n + 1), 6000);
    return () => clearTimeout(id);
  }, [msg]);
  if (voiceError && (!msg || Date.now() - msg.at > 6000)) {
    return (
      <button onClick={() => go({ name: "settings" })} className="absolute inset-x-0 top-[438px] text-center">
        <span className="rounded-full bg-red-500/20 px-[12px] py-[3px] text-[13px] font-medium text-red-300">
          Voice off · {voiceError} (click to fix)
        </span>
      </button>
    );
  }
  if (!msg || Date.now() - msg.at > 6000) return null;
  return (
    <div className="absolute inset-x-0 top-[438px] text-center">
      <span className="rounded-full bg-[#a855f7]/20 px-[12px] py-[3px] text-[13px] font-medium text-[#d8b4fe]">
        Engineer · {msg.text}
      </span>
    </div>
  );
}
