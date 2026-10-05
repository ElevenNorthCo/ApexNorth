import { useEffect, useRef } from "react";
import type { Sample } from "../lib/api";

interface Props {
  samples: Sample[];
  /** Reference lap's line, drawn dashed. */
  compare?: { x: number; z: number }[] | null;
  /** Reference car at the same lap time. */
  ghost?: { x: number; z: number } | null;
  /** Index into `samples` of the car position. */
  cursor: number;
  /** Optional [startIdx, endIdx] to frame and highlight. */
  range?: [number, number] | null;
  zoom: number;
  onZoom: (z: number) => void;
  sectorMarks: number[];
}

const TRACK_WIDTH_M = 13;

/** Canvas track map: track band, racing line, compare line and car dot. */
/**
 * Laps are loops, so join the end back to the start when they're close: a
 * packet gap at the line can leave a short hole there.
 */
function closes(pts: { x: number; z: number }[]): boolean {
  if (pts.length < 10) return false;
  const a = pts[0];
  const b = pts[pts.length - 1];
  const gap = Math.hypot(a.x - b.x, a.z - b.z);
  return gap > 0.5 && gap < 250;
}

export function TrackMap({ samples, compare, ghost, cursor, range, zoom, onZoom, sectorMarks }: Props) {
  const ref = useRef<HTMLCanvasElement>(null);
  const mini = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas || samples.length < 2) return;
    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      canvas.width = w * dpr;
      canvas.height = h * dpr;
      const ctx = canvas.getContext("2d")!;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      // Frame: the whole lap, the selected sector, or follow the car when zoomed.
      const framed = range ? samples.slice(range[0], range[1] + 1) : samples;
      const xs = framed.map((s) => s.x);
      const ys = framed.map((s) => -s.z);
      const minX = Math.min(...xs);
      const maxX = Math.max(...xs);
      const minY = Math.min(...ys);
      const maxY = Math.max(...ys);
      const pad = 40;
      const fit = Math.min((w - pad * 2) / (maxX - minX || 1), (h - pad * 2) / (maxY - minY || 1));
      const scale = fit * zoom;
      const car = samples[Math.min(cursor, samples.length - 1)];
      const cx = zoom > 1 ? car.x : (minX + maxX) / 2;
      const cy = zoom > 1 ? -car.z : (minY + maxY) / 2;
      const px = (x: number) => w / 2 + (x - cx) * scale;
      const py = (z: number) => h / 2 + (-z - cy) * scale;

      // Grid
      ctx.strokeStyle = "rgba(255,255,255,0.05)";
      ctx.lineWidth = 1;
      const gridM = 100;
      const gx0 = Math.floor((cx - w / 2 / scale) / gridM) * gridM;
      for (let gx = gx0; px(gx) < w; gx += gridM) {
        ctx.beginPath();
        ctx.moveTo(px(gx), 0);
        ctx.lineTo(px(gx), h);
        ctx.stroke();
      }
      const gy0 = Math.floor((cy - h / 2 / scale) / gridM) * gridM;
      for (let gy = gy0; h / 2 + (gy - cy) * scale < h; gy += gridM) {
        const y = h / 2 + (gy - cy) * scale;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }

      const path = (pts: { x: number; z: number }[], from = 0, to = pts.length - 1) => {
        ctx.beginPath();
        for (let i = from; i <= to; i++) {
          const p = pts[i];
          if (i === from) ctx.moveTo(px(p.x), py(p.z));
          else ctx.lineTo(px(p.x), py(p.z));
        }
        if (from === 0 && to === pts.length - 1 && closes(pts)) ctx.lineTo(px(pts[0].x), py(pts[0].z));
      };

      ctx.lineJoin = "round";
      ctx.lineCap = "round";
      // Track surface
      path(samples);
      ctx.strokeStyle = "#3a3d47";
      ctx.lineWidth = Math.max(6, TRACK_WIDTH_M * scale);
      ctx.stroke();

      // Sector boundaries
      ctx.strokeStyle = "rgba(255,255,255,0.9)";
      ctx.lineWidth = 2;
      for (const i of sectorMarks) {
        const a = samples[Math.max(0, i - 1)];
        const b = samples[Math.min(samples.length - 1, i + 1)];
        const dx = b.x - a.x;
        const dz = b.z - a.z;
        const len = Math.hypot(dx, dz) || 1;
        const nx = (-dz / len) * (TRACK_WIDTH_M * 0.75);
        const nz = (dx / len) * (TRACK_WIDTH_M * 0.75);
        const s = samples[i];
        ctx.beginPath();
        ctx.moveTo(px(s.x - nx), py(s.z - nz));
        ctx.lineTo(px(s.x + nx), py(s.z + nz));
        ctx.stroke();
      }

      if (compare && compare.length > 1) {
        path(compare);
        ctx.strokeStyle = "rgba(255,255,255,0.55)";
        ctx.setLineDash([6, 5]);
        ctx.lineWidth = 1.6;
        ctx.stroke();
        ctx.setLineDash([]);
      }

      // Racing line (dimmed outside the selected sector)
      path(samples);
      ctx.strokeStyle = range ? "rgba(177,140,242,0.3)" : "#b18cf2";
      ctx.lineWidth = 2;
      ctx.stroke();
      if (range) {
        path(samples, range[0], range[1]);
        ctx.strokeStyle = "#b18cf2";
        ctx.lineWidth = 2.5;
        ctx.stroke();
      }

      if (ghost) {
        const c = ghost;
        ctx.beginPath();
        ctx.arc(px(c.x), py(c.z), 7, 0, Math.PI * 2);
        ctx.strokeStyle = "white";
        ctx.lineWidth = 2;
        ctx.stroke();
      }
      ctx.beginPath();
      ctx.arc(px(car.x), py(car.z), 9, 0, Math.PI * 2);
      ctx.fillStyle = "white";
      ctx.shadowColor = "rgba(0,0,0,0.6)";
      ctx.shadowBlur = 8;
      ctx.fill();
      ctx.shadowBlur = 0;
    };
    draw();
    const ro = new ResizeObserver(draw);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [samples, compare, ghost, cursor, range, zoom, sectorMarks]);

  // Overview mini-map
  useEffect(() => {
    const canvas = mini.current;
    if (!canvas || samples.length < 2) return;
    const dpr = window.devicePixelRatio || 1;
    const size = 96;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, size, size);
    const xs = samples.map((s) => s.x);
    const ys = samples.map((s) => -s.z);
    const minX = Math.min(...xs);
    const minY = Math.min(...ys);
    const sc = (size - 16) / Math.max(Math.max(...xs) - minX || 1, Math.max(...ys) - minY || 1);
    const ox = (size - (Math.max(...xs) - minX) * sc) / 2;
    const oy = (size - (Math.max(...ys) - minY) * sc) / 2;
    ctx.beginPath();
    samples.forEach((s, i) => {
      const x = ox + (s.x - minX) * sc;
      const y = oy + (-s.z - minY) * sc;
      if (i) ctx.lineTo(x, y);
      else ctx.moveTo(x, y);
    });
    if (closes(samples)) ctx.closePath();
    ctx.strokeStyle = "#4a9eff";
    ctx.lineWidth = 3.5;
    ctx.lineJoin = "round";
    ctx.stroke();
    const c = samples[Math.min(cursor, samples.length - 1)];
    ctx.beginPath();
    ctx.arc(ox + (c.x - minX) * sc, oy + (-c.z - minY) * sc, 4, 0, Math.PI * 2);
    ctx.fillStyle = "white";
    ctx.fill();
  }, [samples, cursor]);

  return (
    <div
      className="relative h-full w-full bg-ink"
      onWheel={(e) => onZoom(Math.max(1, Math.min(12, zoom * (e.deltaY < 0 ? 1.2 : 1 / 1.2))))}
    >
      <canvas ref={ref} className="h-full w-full" />
      <canvas ref={mini} className="absolute right-4 top-4 h-[96px] w-[96px]" />
      <div className="absolute bottom-5 right-4 flex flex-col gap-2">
        <button
          onClick={() => onZoom(Math.min(12, zoom * 1.5))}
          className="flex h-11 w-11 items-center justify-center rounded-full bg-[#0e1413] border border-white/10 text-2xl text-white/90 hover:border-accent/60"
        >
          +
        </button>
        <button
          onClick={() => onZoom(Math.max(1, zoom / 1.5))}
          className="flex h-11 w-11 items-center justify-center rounded-full bg-[#0e1413] border border-white/10 text-2xl text-white/90 hover:border-accent/60"
        >
          −
        </button>
      </div>
      <div className="num absolute bottom-5 left-1/2 -translate-x-1/2 text-[15px] text-white/80">
        {Math.round(samples[Math.min(cursor, samples.length - 1)]?.d ?? 0)} m
      </div>
    </div>
  );
}
