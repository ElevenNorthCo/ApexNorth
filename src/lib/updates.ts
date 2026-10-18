import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect } from "react";
import { create } from "zustand";
import { api, type UpdateInfo } from "./api";

/** Where releases are published, for when an update can't install itself. */
export const REPO = "ElevenNorthCo/ApexNorth";
export const RELEASES_URL = `https://github.com/${REPO}/releases/latest`;

/** An update to offer: `manual` ones open the download page instead of installing. */
export type Offer = UpdateInfo & { manual?: boolean };

/** "v0.2.1" / "0.2.1" → [0, 2, 1]; anything after a "-" or "+" is ignored. */
function parts(v: string): number[] {
  return v.replace(/^v/, "").split(/[-+ ]/)[0].split(".").map((n) => Number(n) || 0);
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
 * A newer release, if there is one. The signed in-app update comes first. If
 * that check fails (no update feed on the release, a network hiccup), the
 * GitHub releases list is the fallback, so nobody silently misses a release:
 * the button then opens the download page. Quietly null when offline.
 */
export async function checkForUpdate(): Promise<Offer | null> {
  try {
    return await api.checkUpdate();
  } catch (e) {
    console.warn("in-app update check failed, falling back to the releases list:", e);
  }
  try {
    const [current, res] = await Promise.all([
      getVersion(),
      fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } }),
    ]);
    if (!res.ok) return null;
    const r: { tag_name?: string; draft?: boolean; prerelease?: boolean } = await res.json();
    if (!r.tag_name || r.draft || r.prerelease || !newer(r.tag_name, current)) return null;
    return { version: r.tag_name.replace(/^v/, ""), notes: null, manual: true };
  } catch {
    return null;
  }
}

export type Install =
  | { step: "ready" }
  | { step: "downloading"; pct: number | null }
  | { step: "installing" }
  /** Android: waiting for the user to allow installs; tapping again installs. */
  | { step: "permission" }
  | { step: "failed"; error: string };

interface Updates {
  /** The newer release, if the last check found one. */
  offer: Offer | null;
  /** "current" = the last check found nothing newer. */
  check: "idle" | "checking" | "current";
  install: Install;
  /** Ask the feed again. Shared by the TopBar's timer and Settings' button. */
  checkNow: () => Promise<void>;
  /** Download, install and restart; a failed or manual update opens the download page. */
  installNow: () => void;
}

export const useUpdates = create<Updates>((set, get) => ({
  offer: null,
  check: "idle",
  install: { step: "ready" },
  checkNow: async () => {
    const { check, install } = get();
    if (check === "checking" || install.step === "downloading" || install.step === "installing") return;
    set({ check: "checking" });
    const offer = await checkForUpdate();
    set({ offer, check: offer ? "idle" : "current", install: { step: "ready" } });
  },
  installNow: () => {
    const { offer, install } = get();
    if (!offer) return;
    if (install.step === "failed" || offer.manual) {
      openUrl(RELEASES_URL);
      return;
    }
    if (install.step !== "ready" && install.step !== "permission") return;
    set({ install: { step: "downloading", pct: null } });
    api
      .installUpdate((p) => {
        if (p.event === "installing") set({ install: { step: "installing" } });
        else if (p.event === "needsPermission") set({ install: { step: "permission" } });
        else if (p.event === "downloading")
          set({ install: { step: "downloading", pct: p.total ? Math.min(100, Math.round((p.done / p.total) * 100)) : null } });
      })
      // Desktop restarts before this. Android has opened its installer: if
      // the user backs out of it, the button works again.
      .then(() => get().install.step === "installing" && set({ install: { step: "ready" } }))
      .catch((e) => set({ install: { step: "failed", error: String(e) } }));
  },
}));

/** A few seconds after launch, then every 6 hours: is there a newer release? */
export function useUpdateChecks() {
  const checkNow = useUpdates((s) => s.checkNow);
  useEffect(() => {
    const check = () => void checkNow();
    const t = setTimeout(check, 4000);
    const id = setInterval(check, 6 * 60 * 60 * 1000);
    return () => {
      clearTimeout(t);
      clearInterval(id);
    };
  }, [checkNow]);
}
