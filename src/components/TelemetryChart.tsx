import { useEffect, useLayoutEffect, useRef } from "react";
import uPlot from "uplot";
import { lerpAt, type Playhead } from "../lib/playhead";

export interface ChartSeries {
  label: string;
  values: (number | null)[];
  color: string;
  dash?: number[];
  width?: number;
  stepped?: boolean;
}

/**
 * A single telemetry trace against lap distance. All charts share the same
 * x array (the lap's sample distances); the marker follows the shared
 * playhead so the map, charts and playback stay in sync.
 *
 * Drag (mouse or finger) to scrub; a vertical swipe still scrolls the page.
 * Pinch, or ctrl/trackpad-pinch the wheel, to zoom the distance axis.
 */
export function TelemetryChart({
  x,
  series,
  height = 120,
  yRange,
  xRange,
  playhead,
  onHover,
  onScrub,
  onRange,
  zeroLine = false,
}: {
  x: number[];
  series: ChartSeries[];
  height?: number;
  yRange?: [number, number];
  xRange: [number, number];
  playhead: Playhead;
  /** Mouse moved over the chart with no button down. */
  onHover: (d: number) => void;
  /** Pressed (`start`) or dragged on the chart: the parent pauses playback and scrubs here. */
  onScrub: (d: number, start: boolean) => void;
  /** Pinched to a new distance range. */
  onRange?: (range: [number, number]) => void;
  zeroLine?: boolean;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const marker = useRef<HTMLDivElement | null>(null);
  const handlers = useRef({ onHover, onScrub, onRange });
  handlers.current = { onHover, onScrub, onRange };
  const rangeRef = useRef(xRange);
  rangeRef.current = xRange;

  const placeMarker = () => {
    const u = plot.current;
    const m = marker.current;
    if (!u || !m || !x.length) return;
    const d = lerpAt(x, playhead.pos, (v) => v);
    const left = u.valToPos(d, "x");
    m.style.display = left >= 0 && left <= u.over.clientWidth ? "block" : "none";
    m.style.left = `${left}px`;
  };

  useLayoutEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const opts: uPlot.Options = {
      width: el.clientWidth,
      height,
      legend: { show: false },
      // Pointer handling is our own (below), so touch works the same as a mouse.
      cursor: {
        show: false,
        x: false,
        y: false,
        points: { show: false },
        drag: { x: false, y: false },
      },
      scales: {
        x: { time: false, min: xRange[0], max: xRange[1] },
        y: yRange ? { range: yRange } : { auto: true },
      },
      axes: [
        { show: false },
        {
          stroke: "#7c8296",
          size: 44,
          gap: 4,
          ticks: { show: false },
          grid: { stroke: "rgba(255,255,255,0.07)", width: 1 },
          font: "11px JetBrains Mono",
          splits: yRange ? () => [yRange[0], (yRange[0] + yRange[1]) / 2, yRange[1]] : undefined,
        },
      ],
      series: [
        {},
        ...series.map((s) => ({
          label: s.label,
          stroke: s.color,
          width: s.width ?? 1.6,
          dash: s.dash,
          points: { show: false },
          paths: s.stepped ? uPlot.paths.stepped!({ align: 1 }) : undefined,
        })),
      ],
      hooks: {
        draw: [
          (u) => {
            if (!zeroLine) return;
            const y = u.valToPos(0, "y", true);
            const ctx = u.ctx;
            ctx.save();
            ctx.strokeStyle = "rgba(255,255,255,0.35)";
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(u.bbox.left, y);
            ctx.lineTo(u.bbox.left + u.bbox.width, y);
            ctx.stroke();
            ctx.restore();
          },
        ],
      },
    };
    const u = new uPlot(opts, [x, ...series.map((s) => s.values)] as uPlot.AlignedData, el);
    plot.current = u;
    const m = document.createElement("div");
    m.style.cssText = "position:absolute;top:0;bottom:0;width:1.5px;background:#fff;pointer-events:none;transform:translateX(-0.75px)";
    u.over.appendChild(m);
    marker.current = m;
    const over = u.over;
    over.style.cursor = "ew-resize";
    // Horizontal drags and pinches are ours; vertical swipes scroll the page.
    over.style.touchAction = "pan-y";
    const at = (clientX: number) => u.posToVal(clientX - over.getBoundingClientRect().left, "x");
    const pointers = new Map<number, number>();
    let pinch: { dist: number; range: [number, number]; centerPx: number } | null = null;
    const startPinch = () => {
      const [a, b] = [...pointers.values()];
      const left = over.getBoundingClientRect().left;
      pinch = { dist: Math.abs(a - b) || 1, range: rangeRef.current, centerPx: (a + b) / 2 - left };
    };
    const onDown = (e: PointerEvent) => {
      if (e.pointerType === "mouse" && e.button !== 0) return;
      over.setPointerCapture(e.pointerId);
      pointers.set(e.pointerId, e.clientX);
      if (pointers.size === 2) startPinch();
      else if (pointers.size === 1) handlers.current.onScrub(at(e.clientX), true);
    };
    const onMove = (e: PointerEvent) => {
      if (!pointers.has(e.pointerId)) {
        if (e.pointerType === "mouse" && e.buttons === 0) handlers.current.onHover(at(e.clientX));
        return;
      }
      pointers.set(e.pointerId, e.clientX);
      if (pointers.size >= 2 && pinch) {
        const [a, b] = [...pointers.values()];
        const width = over.clientWidth || 1;
        const span = ((pinch.range[1] - pinch.range[0]) * pinch.dist) / (Math.abs(a - b) || 1);
        const centerVal = pinch.range[0] + (pinch.centerPx / width) * (pinch.range[1] - pinch.range[0]);
        const min = centerVal - (pinch.centerPx / width) * span;
        handlers.current.onRange?.([min, min + span]);
      } else if (pointers.size === 1) {
        handlers.current.onScrub(at(e.clientX), false);
      }
    };
    const onUp = (e: PointerEvent) => {
      pointers.delete(e.pointerId);
      if (pointers.size < 2) pinch = null;
    };
    const onWheel = (e: WheelEvent) => {
      // Trackpad pinch (and ctrl + wheel) zooms; a plain wheel scrolls the page.
      if (!e.ctrlKey || !handlers.current.onRange) return;
      e.preventDefault();
      const [lo, hi] = rangeRef.current;
      const width = over.clientWidth || 1;
      const px = e.clientX - over.getBoundingClientRect().left;
      const center = lo + (px / width) * (hi - lo);
      const span = (hi - lo) * Math.exp(e.deltaY * 0.01);
      const min = center - (px / width) * span;
      handlers.current.onRange([min, min + span]);
    };
    over.addEventListener("pointerdown", onDown);
    over.addEventListener("pointermove", onMove);
    over.addEventListener("pointerup", onUp);
    over.addEventListener("pointercancel", onUp);
    over.addEventListener("wheel", onWheel, { passive: false });
    placeMarker();

    const ro = new ResizeObserver(() => {
      u.setSize({ width: el.clientWidth, height });
      placeMarker();
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
    // Rebuild when the data set changes; range and cursor are updated below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [x, series, height]);

  useEffect(() => {
    plot.current?.setScale("x", { min: xRange[0], max: xRange[1] });
    placeMarker();
  }, [xRange[0], xRange[1]]);

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => playhead.subscribe(placeMarker), [playhead, x]);

  return <div ref={wrap} className="w-full" />;
}
