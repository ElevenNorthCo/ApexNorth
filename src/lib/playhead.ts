import { useSyncExternalStore } from "react";
import type { Sample } from "./api";

/**
 * Where the lap replay is: a fractional index into the lap's samples, so the
 * car glides between recorded points instead of hopping. The map, the chart
 * markers and the readout subscribe to it and redraw themselves directly, at
 * most once per animation frame, without re-rendering the whole view. That
 * keeps playback smooth on phones.
 */
export class Playhead {
  pos = 0;
  private subs = new Set<() => void>();
  private frame = 0;

  set(pos: number) {
    if (pos === this.pos) return;
    this.pos = pos;
    if (!this.frame) {
      this.frame = requestAnimationFrame(() => {
        this.frame = 0;
        this.subs.forEach((f) => f());
      });
    }
  }

  subscribe = (f: () => void) => {
    this.subs.add(f);
    return () => {
      this.subs.delete(f);
    };
  };
}

/** Re-render on every playhead move (for small readouts only). */
export function usePlayhead(p: Playhead): number {
  return useSyncExternalStore(p.subscribe, () => p.pos);
}

/** Index of the first sample at or beyond `v` for a monotonically increasing key. */
export function seek(samples: Sample[], key: "d" | "t", v: number): number {
  let lo = 0;
  let hi = samples.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (samples[mid][key] < v) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

/** The fractional sample position at lap time `t` or distance `d`. */
export function posAt(samples: Sample[], key: "d" | "t", v: number): number {
  if (samples.length < 2) return 0;
  const i = seek(samples, key, v);
  if (i === 0) return 0;
  const a = samples[i - 1][key];
  const b = samples[i][key];
  const f = b > a ? (v - a) / (b - a) : 1;
  return i - 1 + Math.max(0, Math.min(1, f));
}

/** A value between samples: `get` read at the two neighbours of `pos`, blended. */
export function lerpAt<T>(list: T[], pos: number, get: (v: T) => number): number {
  const i = Math.max(0, Math.min(list.length - 1, Math.floor(pos)));
  const j = Math.min(list.length - 1, i + 1);
  const f = pos - i;
  const a = get(list[i]);
  return f > 0 && j !== i ? a + (get(list[j]) - a) * f : a;
}

/** Where the car is at `pos`, in world (x, z). */
export function pointAt(list: { x: number; z: number }[], pos: number): { x: number; z: number } {
  return { x: lerpAt(list, pos, (p) => p.x), z: lerpAt(list, pos, (p) => p.z) };
}
