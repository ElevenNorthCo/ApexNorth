import type { SessionKind, SessionSummary } from "./api";

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

// One expected photo per venue, regardless of how many layouts it has.
const TRACK_PHOTO_SLUGS = [
  "mount-panorama-circuit", "red-bull-ring", "circuit-de-spa-francorchamps",
  "autodromo-de-interlagos", "circuit-gilles-villeneuve", "lake-louise", "dragon-trail",
  "24-heures-du-mans-racing-circuit", "alsace", "circuit-de-sainte-croix", "nurburgring",
  "autodrome-lago-maggiore", "autodromo-nazionale-monza", "sardegna-road-track", "sardegna-windmills",
  "autopolis", "broad-bean-raceway", "fuji-international-speedway", "high-speed-ring",
  "kyoto-driving-park", "suzuka-circuit", "tokyo-expressway", "tsukuba-circuit",
  "circuit-de-barcelona-catalunya", "deep-forest-raceway", "eiger-nordwand", "yas-marina-circuit",
  "brands-hatch", "goodwood", "blue-moon-bay-speedway", "colorado-springs",
  "daytona-international-speedway", "fishermans-ranch", "grand-valley-highway-1",
  "michelin-raceway-road-atlanta", "northern-isle-speedway", "special-stage-route-x",
  "trial-mountain-circuit", "watkins-glen-international", "weathertech-raceway-laguna-seca",
  "willow-springs-international-raceway",
] as const;

export const trackPhotoStats = {
  added: TRACK_PHOTO_SLUGS.filter((key) => Boolean(TRACK_IMAGES[key])).length,
  expected: TRACK_PHOTO_SLUGS.length,
};

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
