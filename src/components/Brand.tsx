/** The ApexNorth peak mark: two neon chevrons, the taller one cut by a road. */
export function LogoMark({ size = 44, color = "var(--color-accent)" }: { size?: number; color?: string }) {
  return (
    <svg width={size} height={size * 0.68} viewBox="0 0 66 45" fill={color} style={{ filter: `drop-shadow(0 0 6px ${color})` }}>
      <path d="M0 45 L27 3 L54 45 H43 L27 20 L11 45 Z" />
      <path d="M30 45 L44 23 L66 45 H56 L44 33 L37 45 Z" opacity="0.85" />
      <path d="M19 45 L27 33 L35 45 Z" fill="#040706" />
    </svg>
  );
}

/** Logo mark + "APEX NORTH" wordmark + tagline. */
export function Brand({ scale = 1, tagline = true }: { scale?: number; tagline?: boolean }) {
  return (
    <div className="flex items-center" style={{ gap: 12 * scale }}>
      <LogoMark size={52 * scale} />
      <div className="leading-none">
        <div className="brand whitespace-nowrap" style={{ fontSize: 30 * scale }}>
          <span className="text-white">APEX</span> <span className="text-accent">NORTH</span>
        </div>
        {tagline && (
          <div
            className="tech mt-[3px] whitespace-nowrap italic text-white/70"
            style={{ fontSize: 9 * scale, letterSpacing: "0.32em" }}
          >
            Gran Turismo 7 Companion
          </div>
        )}
      </div>
    </div>
  );
}
