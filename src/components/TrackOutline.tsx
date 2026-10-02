/** Small SVG thumbnail of a track from [x, z] points (z is flipped so north is up). */
export function TrackOutline({
  points,
  size = 64,
  stroke = "white",
  strokeWidth = 5,
}: {
  points: [number, number][];
  size?: number;
  stroke?: string;
  strokeWidth?: number;
}) {
  if (points.length < 3) return null;
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => -p[1]);
  const minX = Math.min(...xs);
  const minY = Math.min(...ys);
  const w = Math.max(...xs) - minX || 1;
  const h = Math.max(...ys) - minY || 1;
  const s = 100 / Math.max(w, h);
  const ox = (100 - w * s) / 2;
  const oy = (100 - h * s) / 2;
  const d =
    points.map((p, i) => `${i ? "L" : "M"}${(ox + (p[0] - minX) * s).toFixed(1)},${(oy + (-p[1] - minY) * s).toFixed(1)}`).join("") + "Z";
  return (
    <svg width={size} height={size} viewBox="-6 -6 112 112" className="shrink-0">
      <path d={d} fill="none" stroke={stroke} strokeWidth={strokeWidth} strokeLinejoin="round" />
    </svg>
  );
}
