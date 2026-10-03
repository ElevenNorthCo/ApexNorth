import { hueFor, trackImage } from "../lib/media";
import { TrackOutline } from "./TrackOutline";

/** Venue photo when one has been added, else generated art from the track's shape. */
export function TrackArt({ venue, outline, className = "" }: { venue: string | null; outline: [number, number][]; className?: string }) {
  const img = trackImage(venue);
  const hue = hueFor(venue ?? "track");
  return (
    <div className={`relative overflow-hidden ${className}`}>
      {img ? (
        <img src={img} alt="" className="absolute inset-0 h-full w-full object-cover" draggable={false} />
      ) : (
        <div
          className="absolute inset-0"
          style={{
            background: `radial-gradient(120% 90% at 20% 110%, hsl(${hue} 70% 32% / 0.85), transparent 60%),
              radial-gradient(80% 60% at 90% 0%, hsl(${(hue + 40) % 360} 80% 55% / 0.35), transparent 70%),
              linear-gradient(160deg, hsl(${hue} 40% 14%), #050807)`,
          }}
        >
          <div className="absolute inset-0 opacity-[0.12]" style={{ background: "repeating-linear-gradient(-55deg, #fff 0 1px, transparent 1px 12px)" }} />
          <div className="absolute -bottom-6 -right-4 opacity-40">
            <TrackOutline points={outline} size={170} stroke={`hsl(${hue} 90% 70%)`} strokeWidth={3} />
          </div>
        </div>
      )}
      <div className="absolute inset-0 bg-gradient-to-r from-transparent via-transparent to-[#070b0a]" />
    </div>
  );
}
