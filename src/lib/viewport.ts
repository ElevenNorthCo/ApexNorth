import { useSyncExternalStore } from "react";

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
 * bars take. iOS fills in CSS env(safe-area-inset-*) itself; Android's
 * WebView doesn't, so the app's activity reports them.
 */
export function watchInsets() {
  const bridge = window.ApexNorthAndroid;
  if (!bridge) return;
  const apply = () => {
    try {
      const i = JSON.parse(bridge.insets()) as Record<"top" | "right" | "bottom" | "left", number>;
      for (const side of ["top", "right", "bottom", "left"] as const) {
        document.documentElement.style.setProperty(`--an-inset-${side}`, `${Math.round(i[side] ?? 0)}px`);
      }
    } catch (e) {
      console.warn("insets", e);
    }
  };
  apply();
  window.addEventListener("an-insets", apply);
  window.addEventListener("resize", apply);
}
