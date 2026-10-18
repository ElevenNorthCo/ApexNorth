import { useSyncExternalStore } from "react";
import { api } from "./api";

/**
 * Screen shape, for adapting layouts to phones. Desktop windows can't get
 * this small (the window minimum is 900 × 520), so `compact` means a phone or
 * a small tablet, whatever the OS.
 */
export interface Viewport {
  compact: boolean;
  portrait: boolean;
}

const query = () => window.matchMedia("(max-width: 899px), (max-height: 519px)").matches;

let current: Viewport = read();

function read(): Viewport {
  return { compact: query(), portrait: window.innerHeight > window.innerWidth };
}

function subscribe(onChange: () => void) {
  const update = () => {
    const next = read();
    if (next.compact !== current.compact || next.portrait !== current.portrait) {
      current = next;
      onChange();
    }
  };
  window.addEventListener("resize", update);
  window.addEventListener("orientationchange", update);
  return () => {
    window.removeEventListener("resize", update);
    window.removeEventListener("orientationchange", update);
  };
}

export function useViewport(): Viewport {
  return useSyncExternalStore(subscribe, () => current);
}

declare global {
  interface Window {
    /** MainActivity's bridge on Android (see gen/android). */
    ApexNorthAndroid?: { insets(): string };
  }
}

/**
 * Keep `--an-inset-*` up to date with the space the camera cutout and system
 * bars take. Android's WebView doesn't fill in CSS env(safe-area-inset-*) for
 * an edge-to-edge app, and iOS's keeps the portrait values in a landscape-only
 * app, so the app reports them itself (Android: the activity's bridge; iOS:
 * the `safe_area` command).
 */
export function watchInsets() {
  const set = (i: Record<"top" | "right" | "bottom" | "left", number>) => {
    for (const side of ["top", "right", "bottom", "left"] as const) {
      document.documentElement.style.setProperty(`--an-inset-${side}`, `${Math.round(i[side] ?? 0)}px`);
    }
  };
  const bridge = window.ApexNorthAndroid;
  if (bridge) {
    const apply = () => {
      try {
        set(JSON.parse(bridge.insets()));
      } catch (e) {
        console.warn("insets", e);
      }
    };
    apply();
    window.addEventListener("an-insets", apply);
    window.addEventListener("resize", apply);
    return;
  }
  if (!/iPhone|iPad|iPod/.test(navigator.userAgent)) return;
  const ask = () =>
    api
      .safeArea()
      .then((i) => i && set(i))
      .catch((e) => console.warn("safe area", e));
  // Rotation settles after the resize event: ask now and once it has.
  const update = () => {
    void ask();
    setTimeout(ask, 400);
  };
  update();
  window.addEventListener("resize", update);
}
