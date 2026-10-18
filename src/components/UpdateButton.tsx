import { useUpdates } from "../lib/updates";

/**
 * "Update v0.3.0": click to download (the button fills as it goes), install
 * and restart. If that fails, the next click opens the download page.
 */
export function UpdateButton() {
  const offer = useUpdates((s) => s.offer);
  const state = useUpdates((s) => s.install);
  const installNow = useUpdates((s) => s.installNow);
  if (!offer) return null;

  const pct = state.step === "downloading" ? state.pct : state.step === "installing" ? 100 : 0;
  const busy = state.step === "downloading" || state.step === "installing";
  const label =
    state.step === "ready"
      ? `Update v${offer.version}`
      : state.step === "downloading"
        ? state.pct == null
          ? "Downloading…"
          : `Downloading ${state.pct}%`
        : state.step === "installing"
          ? "Installing…"
          : state.step === "permission"
            ? "Allowed? Tap to install"
            : "Update failed · Download";
  const title =
    state.step === "ready"
      ? offer.manual
        ? `ApexNorth ${offer.version} is out. Opens the download page.`
        : `ApexNorth ${offer.version} is out. Click to download and install it; the app restarts when it's done.`
      : state.step === "permission"
        ? "Turn on \"Allow from this source\" for ApexNorth, come back, then tap again."
        : state.step === "failed"
          ? `${state.error}\nClick to open the download page instead.`
        : "Keep ApexNorth open; it restarts when the update is installed.";
  const failed = state.step === "failed";

  return (
    <button
      onClick={installNow}
      disabled={busy}
      title={title}
      className={`tech relative min-w-[150px] overflow-hidden rounded-full border px-3 py-1 text-[12px] font-semibold shadow-[0_0_10px_rgba(57,255,122,0.35)] ${
        failed ? "border-[#ff9f1a]/70 bg-[#ff9f1a]/10 text-[#ff9f1a] hover:bg-[#ff9f1a]/20" : "border-accent/70 bg-accent/10 text-accent hover:bg-accent/20"
      } ${busy ? "cursor-default" : ""}`}
    >
      {busy && (
        <span
          className={`absolute inset-y-0 left-0 bg-accent/30 transition-[width] duration-300 ${state.step === "downloading" && state.pct == null ? "w-full animate-pulse" : ""}`}
          style={state.step === "downloading" && state.pct == null ? undefined : { width: `${pct}%` }}
        />
      )}
      <span className="relative tabular-nums">{label}</span>
    </button>
  );
}
