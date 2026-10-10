import carImageMap from "./carImages.json";
import type { SessionConditions, SessionKind, SessionSummary } from "./api";

/**
 * Track photos, one per venue: src/assets/tracks/<venue-slug>.jpg (e.g.
 * brands-hatch.jpg, nurburgring.jpg; see the README there). Picked up at build
 * time; a venue without one gets generated art.
 */
const trackFiles = import.meta.glob("../assets/tracks/*.{jpg,jpeg,png,webp}", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;

const byBase = (files: Record<string, string>) =>
  Object.fromEntries(Object.entries(files).map(([path, url]) => [path.split("/").pop()!.replace(/\.\w+$/, ""), url]));
const TRACK_IMAGES = byBase(trackFiles);

/**
 * Car renders, one per GT7 car: src/assets/cars/<slug>.webp (600×350, transparent,
 * left-facing three-quarter). carImages.json maps GT7 car_code → file name; it comes from
 * the apexnorthCarAssets repo (gt7-cars.csv). A code with no entry shows no image.
 */
const carFiles = import.meta.glob("../assets/cars/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
}) as Record<string, string>;
const CAR_IMAGES = byBase(carFiles);

export function carImage(carCode: number | null | undefined): string | undefined {
  if (carCode == null) return undefined;
  const file = (carImageMap as Record<string, string>)[String(carCode)];
  return file ? CAR_IMAGES[file.replace(/\.\w+$/, "")] : undefined;
}

/** "Autódromo de Interlagos" → "autodromo-de-interlagos". */
export function slug(s: string): string {
  return s
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

export function trackImage(venue: string | null | undefined): string | undefined {
  return venue ? TRACK_IMAGES[slug(venue)] : undefined;
}


/** A stable hue per venue so generated track art is distinct but repeatable. */
export function hueFor(s: string): number {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return h % 360;
}

/** Split a layout name into the venue headline and the layout subtitle. */
export function trackTitle(s: Pick<SessionSummary, "trackName" | "venue">): [string, string] {
  const name = s.trackName ?? "Unknown track";
  const venue = s.venue;
  if (!venue) return [name, ""];
  if (name.startsWith(venue)) {
    const rest = name.slice(venue.length).replace(/^[\s:–-]+/, "");
    return [venue, rest || "Full Circuit"];
  }
  return [venue, name];
}

const MULTI_WORD_MAKERS = ["Aston Martin", "Alfa Romeo", "Gran Turismo", "Super Formula", "De Tomaso", "RE Amemiya", "Land Rover"];

/** "Toyota GR Corolla MORIZO '22" → ["Toyota", "GR Corolla MORIZO '22"]. */
export function splitCar(name: string): [string, string] {
  const multi = MULTI_WORD_MAKERS.find((m) => name.startsWith(m + " "));
  if (multi) return [multi, name.slice(multi.length + 1)];
  const i = name.indexOf(" ");
  return i < 0 ? [name, ""] : [name.slice(0, i), name.slice(i + 1)];
}

/** Racing class read off the car name, when GT7 puts it there. */
export function carClass(name: string | null | undefined): string | null {
  if (!name) return null;
  const gr = name.match(/Gr\.(\d|B|X)/);
  if (gr) return `Gr.${gr[1]}`;
  if (/\bGT3\b/.test(name)) return "Gr.3";
  if (/Vision Gran Turismo|VGT/.test(name)) return "VGT";
  return null;
}

export const KINDS: { value: SessionKind; label: string; color: string }[] = [
  { value: "practice", label: "Practice", color: "#39ff7a" },
  { value: "qualifying", label: "Qualifying", color: "#ff9f1a" },
  { value: "race", label: "Race", color: "#1ea7ff" },
  { value: "time_trial", label: "Time Trial", color: "#b44cff" },
];

/** The user's tag, else a guess: a lap count set by the game means a race. */
export function sessionKind(s: Pick<SessionSummary, "kind" | "totalLaps">): SessionKind {
  return s.kind ?? (s.totalLaps > 0 ? "race" : "practice");
}

export function kindInfo(kind: SessionKind) {
  return KINDS.find((k) => k.value === kind)!;
}

/** Short tags for a session's noted race settings, e.g. ["Online", "BoP", "Tyre ×3", "Fuel ×2"]. */
export function conditionTags(c: SessionConditions | null | undefined): string[] {
  if (!c) return [];
  const out: string[] = [];
  if (c.mode === "online") out.push("Online");
  if (c.mode === "offline") out.push("Offline");
  if (c.bop) out.push("BoP");
  if (c.tyreWear != null) out.push(c.tyreWear === 0 ? "No tyre wear" : `Tyre ×${c.tyreWear}`);
  if (c.fuelRate != null) out.push(c.fuelRate === 0 ? "No fuel use" : `Fuel ×${c.fuelRate}`);
  return out;
}

/**
 * Whether two sessions' pace can be compared: BoP, tyre wear and fuel
 * multipliers change the lap time, so a setting noted on both sides has to
 * agree. A setting not noted on one side doesn't rule it out.
 */
export function conditionsMatch(a: SessionConditions | null | undefined, b: SessionConditions | null | undefined): boolean {
  const same = <T,>(x: T | null | undefined, y: T | null | undefined) => x == null || y == null || x === y;
  return same(a?.bop, b?.bop) && same(a?.tyreWear, b?.tyreWear) && same(a?.fuelRate, b?.fuelRate);
}
