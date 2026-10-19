import { useEffect, useMemo, useRef, type PointerEvent as ReactPointerEvent, type WheelEvent as ReactWheelEvent } from "react";
import type { Sample } from "../lib/api";
import { lerpAt, pointAt, type Playhead } from "../lib/playhead";

type XZ = { x: number; z: number };

interface Props {
  samples: Sample[];
  /** Reference lap's line, drawn dashed. */
  compare?: XZ[] | null;
  /** Reference car at the same lap time, one entry per sample (null where it has none). */
  ghost?: (XZ | null)[] | null;
  /** Where the replay is. */
  playhead: Playhead;
  /** Optional [startIdx, endIdx] to frame and highlight. */
  range?: [number, number] | null;
  sectorMarks: number[];
}

const TRACK_WIDTH_M = 13;
const MAX_ZOOM = 12;
const PAD = 40;
const MINI = 96;

/**
 * Laps are loops, so join the end back to the start when they're close: a
 * packet gap at the line can leave a short hole there.
 */
function closes(pts: XZ[]): boolean {
  if (pts.length < 10) return false;
  const a = pts[0];
  const b = pts[pts.length - 1];
  const gap = Math.hypot(a.x - b.x, a.z - b.z);
  return gap > 0.5 && gap < 250;
}

/** A line through `pts[from..=to]` in world space (x, -z). */
function line(pts: XZ[], from = 0, to = pts.length - 1): Path2D {
  const p = new Path2D();
  for (let i = from; i <= to; i++) {
    if (i === from) p.moveTo(pts[i].x, -pts[i].z);
    else p.lineTo(pts[i].x, -pts[i].z);
  }
  if (from === 0 && to === pts.length - 1 && closes(pts)) p.lineTo(pts[0].x, -pts[0].z);
  return p;
}

/** Everything that doesn't move during playback, built once per lap as world-space paths. */
interface Scene {
  track: Path2D;
  compare: Path2D | null;
  trace: { path: Path2D; color: string; dim: boolean }[];
  marks: Path2D;
  bounds: { minX: number; maxX: number; minY: number; maxY: number };
  mini: { path: Path2D; scale: number; ox: number; oy: number; minX: number; minY: number };
}

// Input trace colours: what the driver is doing, brighter with pedal pressure.
// Brake red, throttle green, trail braking (both pedals) amber, coasting blue-grey.
const BRAKE = ["#8f2430", "#c42b39", "#ee3243", "#ff4757", "#ff6b7a"];
const THROTTLE = ["#1f7a45", "#25a85a", "#2ed36f", "#39ff7a", "#8dffb4"];
const TRAIL = "#ffb020";
const COAST = "#6f8fb3";

function buildScene(samples: Sample[], compare: XZ[] | null | undefined, range: [number, number] | null | undefined, sectorMarks: number[]): Scene | null {
  if (samples.length < 2) return null;
  const framed = range ? samples.slice(range[0], range[1] + 1) : samples;
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  for (const s of framed) {
    minX = Math.min(minX, s.x);
    maxX = Math.max(maxX, s.x);
    minY = Math.min(minY, -s.z);
    maxY = Math.max(maxY, -s.z);
  }

  const marks = new Path2D();
  for (const i of sectorMarks) {
    const a = samples[Math.max(0, i - 1)];
    const b = samples[Math.min(samples.length - 1, i + 1)];
    const dx = b.x - a.x;
    const dz = b.z - a.z;
    const len = Math.hypot(dx, dz) || 1;
    const nx = (-dz / len) * (TRACK_WIDTH_M * 0.75);
    const nz = (dx / len) * (TRACK_WIDTH_M * 0.75);
    const s = samples[i];
    marks.moveTo(s.x - nx, -(s.z - nz));
    marks.lineTo(s.x + nx, -(s.z + nz));
  }

  // Matching segments are grouped so each frame strokes a few hundred paths, not every sample.
  const style = (i: number) => {
    const s = samples[i];
    const state = s.brk > 5 && s.thr > 5 ? "trail" : s.brk > 5 ? "brake" : s.thr > 5 ? "throttle" : "coast";
    const pressure = state === "brake" ? s.brk : state === "throttle" ? s.thr : 0;
    const band = Math.min(4, Math.floor(pressure / 20));
    const dim = !!range && (i < range[0] || i > range[1]);
    const color = state === "brake" ? BRAKE[band] : state === "throttle" ? THROTTLE[band] : state === "trail" ? TRAIL : COAST;
    return { color, dim, key: `${color}:${dim}` };
  };
  const trace: Scene["trace"] = [];
  let start = 0;
  let current = style(0);
  for (let i = 1; i < samples.length; i++) {
    const next = style(i);
    if (next.key === current.key && i < samples.length - 1) continue;
    trace.push({ path: line(samples, start, i), color: current.color, dim: current.dim });
    start = i;
    current = next;
  }

  // Overview mini-map: the whole lap, whatever is framed.
  let aX = Infinity, bX = -Infinity, aY = Infinity, bY = -Infinity;
  for (const s of samples) {
    aX = Math.min(aX, s.x);
    bX = Math.max(bX, s.x);
    aY = Math.min(aY, -s.z);
    bY = Math.max(bY, -s.z);
  }
  const ms = (MINI - 16) / Math.max(bX - aX || 1, bY - aY || 1);
  const ox = (MINI - (bX - aX) * ms) / 2;
  const oy = (MINI - (bY - aY) * ms) / 2;
  const mini = new Path2D();
  samples.forEach((s, i) => {
    const x = ox + (s.x - aX) * ms;
    const y = oy + (-s.z - aY) * ms;
    if (i) mini.lineTo(x, y);
    else mini.moveTo(x, y);
  });
  if (closes(samples)) mini.closePath();

  return {
    track: line(samples),
    compare: compare && compare.length > 1 ? line(compare) : null,
    trace,
    marks,
    bounds: { minX, maxX, minY, maxY },
    mini: { path: mini, scale: ms, ox, oy, minX: aX, minY: aY },
  };
}

/** The track band, sector lines, compare line and input trace, in world space under the current transform. */
function drawStatic(ctx: CanvasRenderingContext2D, scene: Scene, scale: number) {
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  ctx.strokeStyle = "#3a3d47";
  ctx.lineWidth = Math.max(6, TRACK_WIDTH_M * scale) / scale;
  ctx.stroke(scene.track);

  ctx.strokeStyle = "rgba(255,255,255,0.9)";
  ctx.lineWidth = 2 / scale;
  ctx.stroke(scene.marks);

  if (scene.compare) {
    ctx.strokeStyle = "rgba(255,255,255,0.55)";
    ctx.setLineDash([6 / scale, 5 / scale]);
    ctx.lineWidth = 1.6 / scale;
    ctx.stroke(scene.compare);
    ctx.setLineDash([]);
  }

  // The trace sits on a dark casing so it reads on any background.
  ctx.strokeStyle = "rgba(4,8,7,0.85)";
  ctx.lineWidth = 7 / scale;
  ctx.stroke(scene.track);
  ctx.lineWidth = 4 / scale;
  for (const seg of scene.trace) {
    ctx.strokeStyle = seg.color;
    ctx.globalAlpha = seg.dim ? 0.2 : 1;
    ctx.stroke(seg.path);
  }
  ctx.globalAlpha = 1;
}

/**
 * Canvas track map: track band, racing line, compare line, ghost and car.
 * Pinch (or scroll / trackpad pinch) to zoom, drag to look around when zoomed
 * in, double-tap to fit the lap again. The car follows the playhead between
 * samples, and only the moving parts are drawn each frame.
 */
export function TrackMap({ samples, compare, ghost, playhead, range, sectorMarks }: Props) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const mini = useRef<HTMLCanvasElement>(null);
  const distance = useRef<HTMLDivElement>(null);
  const size = useRef({ w: 0, h: 0, dpr: 1 });
  /** Zoom, and how far the view is moved from the car (world units) when zoomed in. */
  const view = useRef({ zoom: 1, panX: 0, panY: 0, scale: 1 });
  const cache = useRef<{ canvas: HTMLCanvasElement; key: string; scene: Scene } | null>(null);
  const pending = useRef(0);

  const scene = useMemo(() => buildScene(samples, compare, range, sectorMarks), [samples, compare, range, sectorMarks]);

  const draw = () => {
    const el = canvas.current;
    if (!el) return;
    const ctx = el.getContext("2d")!;
    const { w, h, dpr } = size.current;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, el.width, el.height);
    if (!scene || w <= 80 || h <= 80) return;

    const pos = Math.max(0, Math.min(samples.length - 1, playhead.pos));
    const car = pointAt(samples, pos);
    const b = scene.bounds;
    const fit = Math.min((w - PAD * 2) / (b.maxX - b.minX || 1), (h - PAD * 2) / (b.maxY - b.minY || 1));
    const v = view.current;
    const scale = fit * v.zoom;
    v.scale = scale;
    // Zoomed in, the view follows the car (moved by any drag).
    const cx = v.zoom > 1 ? car.x + v.panX : (b.minX + b.maxX) / 2;
    const cy = v.zoom > 1 ? -car.z + v.panY : (b.minY + b.maxY) / 2;
    const world = (c: CanvasRenderingContext2D) => c.setTransform(dpr * scale, 0, 0, dpr * scale, dpr * (w / 2 - cx * scale), dpr * (h / 2 - cy * scale));

    // Grid, in screen space.
    const grid = (c: CanvasRenderingContext2D) => {
      c.setTransform(dpr, 0, 0, dpr, 0, 0);
      c.strokeStyle = "rgba(255,255,255,0.05)";
      c.lineWidth = 1;
      const gridM = 100;
      c.beginPath();
      for (let gx = Math.floor((cx - w / 2 / scale) / gridM) * gridM; w / 2 + (gx - cx) * scale < w; gx += gridM) {
        const x = w / 2 + (gx - cx) * scale;
        c.moveTo(x, 0);
        c.lineTo(x, h);
      }
      for (let gy = Math.floor((cy - h / 2 / scale) / gridM) * gridM; h / 2 + (gy - cy) * scale < h; gy += gridM) {
        const y = h / 2 + (gy - cy) * scale;
        c.moveTo(0, y);
        c.lineTo(w, y);
      }
      c.stroke();
    };

    if (v.zoom === 1) {
      // The whole lap doesn't move: draw it once and copy it each frame.
      const key = `${w}x${h}@${dpr}`;
      if (!cache.current || cache.current.key !== key || cache.current.scene !== scene) {
        const off = cache.current?.canvas ?? document.createElement("canvas");
        off.width = el.width;
        off.height = el.height;
        const oc = off.getContext("2d")!;
        oc.setTransform(1, 0, 0, 1, 0, 0);
        oc.clearRect(0, 0, off.width, off.height);
        grid(oc);
        world(oc);
        drawStatic(oc, scene, scale);
        cache.current = { canvas: off, key, scene };
      }
      ctx.drawImage(cache.current.canvas, 0, 0);
    } else {
      grid(ctx);
      world(ctx);
      drawStatic(ctx, scene, scale);
    }

    // Moving parts, in screen space.
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const px = (x: number) => w / 2 + (x - cx) * scale;
    const py = (z: number) => h / 2 + (-z - cy) * scale;
    if (ghost) {
      const i = Math.floor(pos);
      const a = ghost[i];
      const g = ghost[Math.min(ghost.length - 1, i + 1)];
      const at = a && g ? pointAt([a, g], pos - i) : a;
      if (at) {
        ctx.beginPath();
        ctx.arc(px(at.x), py(at.z), 7, 0, Math.PI * 2);
        ctx.strokeStyle = "white";
        ctx.lineWidth = 2;
        ctx.stroke();
      }
    }
    ctx.beginPath();
    ctx.arc(px(car.x), py(car.z), 9, 0, Math.PI * 2);
    ctx.fillStyle = "white";
    ctx.shadowColor = "rgba(0,0,0,0.6)";
    ctx.shadowBlur = 8;
    ctx.fill();
    ctx.shadowBlur = 0;

    if (distance.current) distance.current.textContent = `${Math.round(lerpAt(samples, pos, (s) => s.d))} m`;

    const m = mini.current;
    if (m) {
      const mc = m.getContext("2d")!;
      const md = scene.mini;
      mc.setTransform(dpr, 0, 0, dpr, 0, 0);
      mc.clearRect(0, 0, MINI, MINI);
      mc.strokeStyle = "#4a9eff";
      mc.lineWidth = 3.5;
      mc.lineJoin = "round";
      mc.stroke(md.path);
      mc.beginPath();
      mc.arc(md.ox + (car.x - md.minX) * md.scale, md.oy + (-car.z - md.minY) * md.scale, 4, 0, Math.PI * 2);
      mc.fillStyle = "white";
      mc.fill();
    }
  };
  const drawRef = useRef(draw);
  drawRef.current = draw;
  const schedule = () => {
    if (!pending.current) {
      pending.current = requestAnimationFrame(() => {
        pending.current = 0;
        drawRef.current();
      });
    }
  };

  // Size the canvases only when the layout changes, never per frame.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      size.current = { w: el.clientWidth, h: el.clientHeight, dpr };
      el.width = Math.round(el.clientWidth * dpr);
      el.height = Math.round(el.clientHeight * dpr);
      if (mini.current) {
        mini.current.width = MINI * dpr;
        mini.current.height = MINI * dpr;
      }
      drawRef.current();
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(el);
    return () => {
      ro.disconnect();
      cancelAnimationFrame(pending.current);
      pending.current = 0;
    };
  }, []);

  useEffect(() => playhead.subscribe(() => drawRef.current()), [playhead]);
  useEffect(schedule, [scene, ghost]);
  // A new sector (or lap) fits the view again.
  useEffect(() => {
    view.current = { ...view.current, zoom: 1, panX: 0, panY: 0 };
    schedule();
  }, [range, samples]);

  const setZoom = (z: number) => {
    const v = view.current;
    v.zoom = Math.max(1, Math.min(MAX_ZOOM, z));
    if (v.zoom === 1) {
      v.panX = 0;
      v.panY = 0;
    }
    schedule();
  };

  // Touch: one finger looks around (when zoomed in), two fingers pinch.
  const pointers = useRef(new Map<number, { x: number; y: number }>());
  const gesture = useRef<{ dist: number; zoom: number } | null>(null);
  const drag = useRef<{ x: number; y: number; panX: number; panY: number } | null>(null);
  const lastTap = useRef(0);
  const startDrag = () => {
    const [p] = pointers.current.values();
    drag.current = p ? { x: p.x, y: p.y, panX: view.current.panX, panY: view.current.panY } : null;
  };
  const spread = () => {
    const [a, b] = [...pointers.current.values()];
    return Math.hypot(a.x - b.x, a.y - b.y) || 1;
  };
  const down = (e: ReactPointerEvent) => {
    if ((e.target as Element).closest("button")) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.current.size === 2) {
      gesture.current = { dist: spread(), zoom: view.current.zoom };
      drag.current = null;
    } else if (pointers.current.size === 1) {
      startDrag();
      if (e.pointerType !== "mouse") {
        const now = performance.now();
        if (now - lastTap.current < 300) setZoom(1);
        lastTap.current = now;
      }
    }
  };
  const move = (e: ReactPointerEvent) => {
    if (!pointers.current.has(e.pointerId)) return;
    pointers.current.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pointers.current.size >= 2 && gesture.current) {
      setZoom(gesture.current.zoom * (spread() / gesture.current.dist));
    } else if (drag.current && view.current.zoom > 1) {
      const p = pointers.current.get(e.pointerId)!;
      const v = view.current;
      v.panX = drag.current.panX - (p.x - drag.current.x) / v.scale;
      v.panY = drag.current.panY - (p.y - drag.current.y) / v.scale;
      schedule();
    }
  };
  const up = (e: ReactPointerEvent) => {
    pointers.current.delete(e.pointerId);
    if (pointers.current.size < 2) gesture.current = null;
    // Lifting one finger of a pinch carries on as a drag.
    if (pointers.current.size === 1) startDrag();
    else drag.current = null;
  };
  const wheel = (e: ReactWheelEvent) => {
    // A trackpad pinch arrives as a wheel event with ctrlKey.
    setZoom(view.current.zoom * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.002)));
  };

  return (
    <div
      className="relative h-full w-full touch-none select-none bg-ink"
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      onWheel={wheel}
      onDoubleClick={(e) => !(e.target as Element).closest("button") && setZoom(1)}
    >
      <canvas ref={canvas} className="h-full w-full" />
      <canvas ref={mini} className="pointer-events-none absolute right-4 top-4 h-[96px] w-[96px]" />
      <div className="tech pointer-events-none absolute left-[76px] top-[18px] flex flex-wrap gap-3 rounded-lg border border-white/10 bg-[#0a100f]/90 px-3 py-2 text-[11px]" aria-label="Driving input legend">
        <span className="flex items-center gap-2"><span className="h-[4px] w-5 rounded-full bg-[#ff4757]" />Brake</span>
        <span className="flex items-center gap-2"><span className="h-[4px] w-5 rounded-full bg-[#39ff7a]" />Throttle</span>
        <span className="flex items-center gap-2"><span className="h-[4px] w-5 rounded-full bg-[#ffb020]" />Trail brake</span>
        <span className="flex items-center gap-2"><span className="h-[4px] w-5 rounded-full bg-[#6f8fb3]" />Coast</span>
      </div>
      <div className="absolute bottom-5 right-4 flex flex-col gap-2">
        <button
          aria-label="Zoom in"
          onClick={() => setZoom(view.current.zoom * 1.5)}
          className="flex h-11 w-11 items-center justify-center rounded-full bg-[#0e1413] border border-white/10 text-2xl text-white/90 hover:border-accent/60"
        >
          +
        </button>
        <button
          aria-label="Zoom out"
          onClick={() => setZoom(view.current.zoom / 1.5)}
          className="flex h-11 w-11 items-center justify-center rounded-full bg-[#0e1413] border border-white/10 text-2xl text-white/90 hover:border-accent/60"
        >
          −
        </button>
      </div>
      <div ref={distance} className="num pointer-events-none absolute bottom-[76px] left-5 text-[15px] text-white/80" />
    </div>
  );
}
