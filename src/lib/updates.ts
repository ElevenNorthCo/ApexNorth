import { getVersion } from "@tauri-apps/api/app";

/** Where releases are published (GitHub Actions builds them from version tags). */
export const REPO = "ElevenNorthCo/ApexNorth";
export const RELEASES_URL = `https://github.com/${REPO}/releases/latest`;

export interface Update {
  version: string;
  url: string;
}

/** "v0.2.1" / "0.2.1" → [0, 2, 1]; anything after a "-" (betas) is ignored. */
function parts(v: string): number[] {
  return v.replace(/^v/, "").split("-")[0].split(".").map((n) => Number(n) || 0);
}

export function newer(latest: string, current: string): boolean {
  const a = parts(latest);
  const b = parts(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) > (b[i] ?? 0);
  }
  return false;
}

/**
 * The latest published release, if it's newer than this app. Quietly null
 * when offline, rate-limited, or the repo isn't public yet.
 */
export async function checkForUpdate(): Promise<Update | null> {
  try {
    const [current, res] = await Promise.all([
      getVersion(),
      fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } }),
    ]);
    if (!res.ok) return null;
    const r: { tag_name?: string; html_url?: string; draft?: boolean; prerelease?: boolean } = await res.json();
    if (!r.tag_name || r.draft || r.prerelease || !newer(r.tag_name, current)) return null;
    return { version: r.tag_name.replace(/^v/, ""), url: r.html_url ?? RELEASES_URL };
  } catch {
    return null;
  }
}
