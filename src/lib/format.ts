export const DASH = "--";

/** 94700 → "01:34.700" (dashboard style, always mm:ss.mmm). */
export function lapClock(ms: number | null | undefined): string {
  if (ms == null || ms < 0) return "00:00.000";
  const m = Math.floor(ms / 60000);
  const s = Math.floor((ms % 60000) / 1000);
  const r = Math.floor(ms % 1000);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(r).padStart(3, "0")}`;
}

/** 94700 → "1:34.700" (list style). */
export function lapTime(ms: number | null | undefined): string {
  if (ms == null || ms <= 0) return "—";
  const m = Math.floor(ms / 60000);
  const s = ((ms % 60000) / 1000).toFixed(3).padStart(6, "0");
  return m > 0 ? `${m}:${s}` : s;
}

/** Sector / split time: 25864 → "25.864". */
export function split(ms: number | null | undefined): string {
  if (ms == null) return "—";
  return (ms / 1000).toFixed(3);
}

export function delta(ms: number | null | undefined, digits = 3): string {
  if (ms == null) return "";
  const sign = ms < 0 ? "-" : "+";
  return `${sign}${(Math.abs(ms) / 1000).toFixed(digits)}`;
}

export function gearLabel(gear: number, inGear: boolean): string {
  if (!inGear) return "N";
  if (gear === 0) return "R";
  return String(gear);
}

export function speed(kph: number, units: "metric" | "imperial"): number {
  return Math.round(units === "imperial" ? kph * 0.621371 : kph);
}

export function carName(code: number): string {
  return `Car #${code}`;
}

export function clock(d = new Date()): string {
  return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export function shortDate(ms: number): string {
  return new Date(ms).toLocaleDateString([], { month: "short", day: "numeric", year: "numeric" });
}

export function timeOfDay(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

export function tyreColor(t: number | null | undefined): string {
  if (t == null || t <= 0) return "#5b5f6b";
  if (t < 50) return "#3b82f6";
  if (t < 65) return "#22d3ee";
  if (t < 92) return "#22c55e";
  if (t < 100) return "#eab308";
  return "#ef4444";
}
