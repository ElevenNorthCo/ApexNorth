import type { CSSProperties, ReactNode } from "react";

/**
 * Neon-outlined dashboard tile. The label is either notched into the top
 * border (`notch`, for the big panels) or sits inside the top edge.
 */
export function Tile({
  label,
  color,
  style,
  className = "",
  notch = false,
  children,
}: {
  label: string;
  color: string;
  style?: CSSProperties;
  className?: string;
  notch?: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={`absolute rounded-[14px] ${className}`}
      style={{
        border: `2px solid ${color}`,
        background: "linear-gradient(180deg, rgba(16,24,22,0.92), rgba(5,8,7,0.96))",
        boxShadow: `0 0 14px color-mix(in srgb, ${color} 45%, transparent), inset 0 0 18px color-mix(in srgb, ${color} 14%, transparent)`,
        ...style,
      }}
    >
      {notch ? (
        <div className="tech absolute left-1/2 top-0 z-10 -translate-x-1/2 -translate-y-1/2 whitespace-nowrap bg-ink px-[12px] text-[15px] font-semibold leading-none text-white">
          {label}
        </div>
      ) : (
        <div className="tech absolute inset-x-0 top-[8px] text-center text-[12px] font-semibold leading-none text-white/90">
          {label}
        </div>
      )}
      {children}
    </div>
  );
}
