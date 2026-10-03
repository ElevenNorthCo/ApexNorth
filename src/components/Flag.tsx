import type { ReactNode } from "react";

/** Simplified flags for every country in the GT7 track catalog (3:2, SVG so they render on every OS). */
const FLAGS: Record<string, ReactNode> = {
  France: <Vertical colors={["#0055A4", "#fff", "#EF4135"]} />,
  Italy: <Vertical colors={["#009246", "#fff", "#CE2B37"]} />,
  Belgium: <Vertical colors={["#111", "#FDDA24", "#EF3340"]} />,
  Germany: <Horizontal colors={["#111", "#DD0000", "#FFCE00"]} />,
  Austria: <Horizontal colors={["#C8102E", "#fff", "#C8102E"]} />,
  Croatia: <Horizontal colors={["#FF0000", "#fff", "#171796"]} />,
  Spain: (
    <>
      <rect width="30" height="20" fill="#AA151B" />
      <rect y="5" width="30" height="10" fill="#F1BF00" />
    </>
  ),
  Japan: (
    <>
      <rect width="30" height="20" fill="#fff" />
      <circle cx="15" cy="10" r="6" fill="#BC002D" />
    </>
  ),
  Switzerland: (
    <>
      <rect width="30" height="20" fill="#DA291C" />
      <rect x="13" y="4" width="4" height="12" fill="#fff" />
      <rect x="9" y="8" width="12" height="4" fill="#fff" />
    </>
  ),
  Canada: (
    <>
      <rect width="30" height="20" fill="#fff" />
      <rect width="7.5" height="20" fill="#D52B1E" />
      <rect x="22.5" width="7.5" height="20" fill="#D52B1E" />
      <path d="M15 4 l1.2 2.4 1.6-.6-.6 3.4 2-1.6.5 1.2 1.8-.3-.8 2 1 .6-3.3 2.4.3 1.2-3.2-.4V16.5h-1v-2.2l-3.2.4.3-1.2-3.3-2.4 1-.6-.8-2 1.8.3.5-1.2 2 1.6-.6-3.4 1.6.6z" fill="#D52B1E" />
    </>
  ),
  Brazil: (
    <>
      <rect width="30" height="20" fill="#009C3B" />
      <path d="M15 2.5 L27.5 10 L15 17.5 L2.5 10 Z" fill="#FFDF00" />
      <circle cx="15" cy="10" r="4.2" fill="#002776" />
      <path d="M11 9.2 Q15 8 19 10.6" stroke="#fff" strokeWidth="0.8" fill="none" />
    </>
  ),
  "United States": (
    <>
      <rect width="30" height="20" fill="#fff" />
      {[0, 2, 4, 6, 8, 10, 12].map((i) => (
        <rect key={i} y={(i * 20) / 13} width="30" height={20 / 13} fill="#B22234" />
      ))}
      <rect width="13" height={(7 * 20) / 13} fill="#3C3B6E" />
      {[0, 1, 2].flatMap((r) =>
        [0, 1, 2, 3].map((c) => <circle key={`${r}${c}`} cx={2 + c * 3 + (r % 2) * 1.5} cy={2 + r * 3.4} r="0.6" fill="#fff" />),
      )}
    </>
  ),
  "United Kingdom": <UnionJack />,
  Australia: (
    <>
      <rect width="30" height="20" fill="#012169" />
      <svg width="15" height="10" viewBox="0 0 30 20">
        <UnionJack />
      </svg>
      <circle cx="7.5" cy="15" r="1.6" fill="#fff" />
      {[
        [22, 4.5],
        [19, 9],
        [25, 8.5],
        [22, 16],
        [24, 11.5],
      ].map(([x, y]) => (
        <circle key={`${x}${y}`} cx={x} cy={y} r="0.9" fill="#fff" />
      ))}
    </>
  ),
  UAE: (
    <>
      <rect width="30" height="20" fill="#fff" />
      <rect width="30" height="6.67" fill="#00732F" />
      <rect y="13.33" width="30" height="6.67" fill="#111" />
      <rect width="8" height="20" fill="#FF0000" />
    </>
  ),
};

function Vertical({ colors }: { colors: string[] }) {
  return (
    <>
      {colors.map((c, i) => (
        <rect key={i} x={i * 10} width="10" height="20" fill={c} />
      ))}
    </>
  );
}

function Horizontal({ colors }: { colors: string[] }) {
  return (
    <>
      {colors.map((c, i) => (
        <rect key={i} y={(i * 20) / 3} width="30" height={20 / 3} fill={c} />
      ))}
    </>
  );
}

function UnionJack() {
  return (
    <>
      <rect width="30" height="20" fill="#012169" />
      <path d="M0 0 L30 20 M30 0 L0 20" stroke="#fff" strokeWidth="4" />
      <path d="M0 0 L30 20 M30 0 L0 20" stroke="#C8102E" strokeWidth="1.4" />
      <path d="M15 0 V20 M0 10 H30" stroke="#fff" strokeWidth="6" />
      <path d="M15 0 V20 M0 10 H30" stroke="#C8102E" strokeWidth="3.4" />
    </>
  );
}

export function Flag({ country, height = 18 }: { country: string | null | undefined; height?: number }) {
  const flag = country ? FLAGS[country] : undefined;
  if (!flag) return null;
  return (
    <svg
      width={height * 1.5}
      height={height}
      viewBox="0 0 30 20"
      className="shrink-0 rounded-[2px] shadow-[0_0_0_1px_rgba(255,255,255,0.15)]"
      aria-label={country ?? undefined}
    >
      <title>{country}</title>
      {flag}
    </svg>
  );
}
