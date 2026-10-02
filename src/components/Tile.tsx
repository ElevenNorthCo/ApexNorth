import type { CSSProperties, ReactNode } from "react";

/** Outlined dashboard tile with its label notched into the top border. */
export function Tile({
  label,
  color,
  style,
  className = "",
  children,
}: {
  label: string;
  color: string;
  style?: CSSProperties;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      className={`absolute rounded-[12px] ${className}`}
      style={{ border: `2.5px solid ${color}`, ...style }}
    >
      <div
        className="num absolute left-1/2 top-0 -translate-x-1/2 -translate-y-1/2 whitespace-nowrap bg-black px-[5px] text-[17px] font-bold leading-none tracking-[0.06em] text-white"
      >
        {label}
      </div>
      {children}
    </div>
  );
}
