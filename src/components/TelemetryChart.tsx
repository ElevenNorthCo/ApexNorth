import { useEffect, useLayoutEffect, useRef } from "react";
import uPlot from "uplot";

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
 * x array; the cursor (lap distance) is controlled by the parent so the map,
 * charts and playback stay in sync.
 */
export function TelemetryChart({
  x,
  series,
  height = 120,
  yRange,
  xRange,
  cursorD,
  onCursor,
  onScrubStart,
  zeroLine = false,
}: {
  x: number[];
  series: ChartSeries[];
  height?: number;
  yRange?: [number, number];
  xRange: [number, number];
  cursorD: number | null;
  onCursor: (d: number) => void;
  /** Pointer pressed on the chart: the parent pauses playback and scrubs from here. */
  onScrubStart?: (d: number) => void;
  zeroLine?: boolean;
}) {
  const wrap = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const marker = useRef<HTMLDivElement | null>(null);
  const onCursorRef = useRef(onCursor);
  onCursorRef.current = onCursor;
  const onScrubRef = useRef(onScrubStart);
  onScrubRef.current = onScrubStart;
  const cursorRef = useRef(cursorD);
  cursorRef.current = cursorD;

  const placeMarker = () => {
    const u = plot.current;
    const m = marker.current;
    const d = cursorRef.current;
    if (!u || !m) return;
    if (d == null) {
      m.style.display = "none";
      return;
    }
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
      cursor: {
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
        setCursor: [
          (u) => {
            const left = u.cursor.left;
            if (left == null || left < 0) return;
            onCursorRef.current(u.posToVal(left, "x"));
          },
        ],
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
    u.over.style.cursor = "ew-resize";
    u.over.addEventListener("pointerdown", (e) => {
      const rect = u.over.getBoundingClientRect();
      onScrubRef.current?.(u.posToVal(e.clientX - rect.left, "x"));
    });
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

  useEffect(placeMarker, [cursorD]);

  return <div ref={wrap} className="w-full" />;
}
