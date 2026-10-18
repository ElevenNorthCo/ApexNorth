import { useEffect, useState } from "react";
import { api } from "../lib/api";
import { useStore } from "../lib/store";
import { Check } from "../components/icons";

/** Three-step connection wizard: get on track → find PlayStation → connected. */
export function Connect() {
  const status = useStore((s) => s.status);
  const settings = useStore((s) => s.settings);
  const go = useStore((s) => s.go);
  const os = useStore((s) => s.build?.os);
  const mobile = !!useStore((s) => s.build?.mobile);
  const device = mobile ? "phone" : "computer";
  const [manual, setManual] = useState(false);
  const [ip, setIp] = useState(settings?.lastIp ?? "");
  const [error, setError] = useState<string | null>(null);
  const [localIps, setLocalIps] = useState<string[]>([]);
  const [searchingSince, setSearchingSince] = useState<number | null>(null);
  const [, tick] = useState(0);

  useEffect(() => {
    api.localIps().then(setLocalIps).catch(() => {});
  }, []);

  const searching = status.state === "searching" || status.state === "connecting" || status.state === "lost";
  const live = status.state === "live";
  const step = live ? 3 : searching ? 2 : 1;

  useEffect(() => {
    if (!searching) {
      setSearchingSince(null);
      return;
    }
    setSearchingSince((s) => s ?? Date.now());
    const id = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, [searching]);

  useEffect(() => {
    if (status.state === "error") setError(status.error);
  }, [status]);

  const run = async (fn: () => Promise<void>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(String(e));
    }
  };

  const slow = searchingSince != null && Date.now() - searchingSince > 12000;

  return (
    <div className="scroll-thin h-full overflow-y-auto px-6 pb-16 pt-8 max-sm:px-4 max-sm:pt-5">
      <div className="mx-auto max-w-2xl">
        <h1 className="brand text-[44px] leading-none">
          CONNECT <span className="text-accent drop-shadow-[0_0_12px_rgba(57,255,122,0.5)]">GT7</span>
        </h1>
        <p className="mt-2 text-[15px] text-muted">
          Live telemetry streams over your Wi-Fi straight from the PlayStation. No extra hardware needed.
        </p>

        <ol className="mt-8 space-y-4">
          <Step n={1} title="Get on track" state={step > 1 ? "done" : "active"}>
            <ul className="list-disc space-y-1 pl-5 text-[14px] text-muted">
              <li>Make sure this {device} and your PS5/PS4 are on the <b className="text-white">same network</b>{mobile && " (Wi-Fi, not mobile data)"}.</li>
              <li>In GT7, start any on-track session: race, time trial, lobby or practice.</li>
            </ul>
            {step === 1 && (
              <div className="mt-4 flex flex-wrap gap-3">
                <Btn primary onClick={() => run(() => api.connect())}>Find my PlayStation</Btn>
                <Btn onClick={() => setManual((m) => !m)}>Enter IP manually</Btn>
                <Btn onClick={() => run(api.startDemo)}>Try demo mode</Btn>
              </div>
            )}
          </Step>

          <Step n={2} title={status.state === "connecting" ? `Connecting to ${status.ip}` : "Searching for your PlayStation"} state={step > 2 ? "done" : step === 2 ? "active" : "todo"}>
            {step === 2 && (
              <div className="space-y-3">
                <div className="flex items-center gap-3 text-[14px] text-[#ffc81a]">
                  <span className="inline-block h-4 w-4 animate-spin rounded-full border-2 border-white/20 border-t-[#ffc81a]" />
                  {status.state === "lost" ? "Signal lost. Waiting for GT7 to resume…" : "Listening for telemetry…"}
                </div>
                {slow && (
                  <div className="rounded-xl border border-white/10 bg-white/5 p-4 text-[13px] text-muted">
                    <div className="mb-1 font-semibold text-white">Not finding it?</div>
                    <ul className="list-disc space-y-1 pl-5">
                      <li>Be on track in GT7. Menus don't send telemetry.</li>
                      <li>Turn off "AP / client isolation" on your router, and avoid guest networks.</li>
                      {os === "windows" && <li>Allow ApexNorth through the Windows firewall (UDP port 33740).</li>}
                      {os === "ios" && (
                        <li>
                          Allow local network access: iPhone Settings → Apps → ApexNorth → <b className="text-white">Local Network</b> on.
                        </li>
                      )}
                      {mobile && <li>Keep ApexNorth open on screen: phones pause apps in the background.</li>}
                      {mobile && <li>Only one telemetry app at a time: close ApexNorth (or other GT7 apps) on other devices.</li>}
                      <li>Enter the PlayStation IP manually: Settings → Network → View Connection Status.</li>
                      <li>5 GHz Wi-Fi or Ethernet gives the smoothest data.</li>
                    </ul>
                  </div>
                )}
                <div className="flex flex-wrap gap-3">
                  <Btn onClick={() => setManual(true)}>Enter IP manually</Btn>
                  <Btn onClick={() => run(api.disconnect)}>Cancel</Btn>
                </div>
              </div>
            )}
          </Step>

          <Step n={3} title="Connected" state={step === 3 ? "done" : "todo"}>
            {live && (
              <div className="space-y-3">
                <div className="text-[14px] text-muted">
                  Receiving <b className="text-white">{Math.round(status.hz)} packets/s</b> from{" "}
                  <b className="text-white">{status.ip === "127.0.0.1" ? "demo simulator" : status.ip}</b>
                  {status.variant && <> · packet format {status.variant === "Tilde" ? "~" : status.variant}</>}
                </div>
                <div className="flex gap-3">
                  <Btn primary onClick={() => go({ name: "dashboard" })}>Open dashboard</Btn>
                  <Btn onClick={() => run(api.disconnect)}>Disconnect</Btn>
                </div>
              </div>
            )}
          </Step>
        </ol>

        {manual && (
          <form
            className="mt-6 rounded-[14px] border border-pos/60 bg-black/50 p-5 shadow-[0_0_14px_rgba(30,167,255,0.2)]"
            onSubmit={(e) => {
              e.preventDefault();
              run(() => api.connect(ip));
            }}
          >
            <label className="tech text-[13px] font-medium text-muted">PlayStation IP address</label>
            <div className="mt-2 flex gap-3">
              <input
                autoFocus
                value={ip}
                onChange={(e) => setIp(e.target.value)}
                placeholder="192.168.1.50"
                inputMode="decimal"
                autoComplete="off"
                className="num flex-1 rounded-xl border border-white/10 bg-black/40 px-4 py-2.5 text-[15px] outline-none focus:border-accent"
              />
              <Btn primary type="submit">Connect</Btn>
            </div>
            <p className="mt-2 text-[12px] text-muted">
              Find it on the PS5 under Settings → Network → Connection Status.
              {localIps.length > 0 && <> This {device}: {localIps.join(", ")}.</>}
            </p>
          </form>
        )}

        {error && <div className="mt-4 rounded-xl border border-red-500/30 bg-red-500/10 p-4 text-[14px] text-red-300">{error}</div>}
      </div>
    </div>
  );
}

function Step({ n, title, state, children }: { n: number; title: string; state: "todo" | "active" | "done"; children?: React.ReactNode }) {
  return (
    <li
      className={`rounded-[14px] border p-5 transition ${
        state === "active"
          ? "border-accent/70 bg-black/50 shadow-[0_0_16px_rgba(57,255,122,0.2)]"
          : state === "done"
            ? "border-white/15 bg-black/40"
            : "border-white/5 bg-black/20 opacity-50"
      }`}
    >
      <div className="flex items-center gap-3">
        <span
          className={`flex h-7 w-7 items-center justify-center rounded-full text-[13px] font-bold ${
            state === "done" ? "bg-accent text-black" : state === "active" ? "bg-accent text-black shadow-[0_0_10px_rgba(57,255,122,0.6)]" : "bg-white/10"
          }`}
        >
          {state === "done" ? <Check width={16} height={16} strokeWidth={3} /> : n}
        </span>
        <h2 className="tech text-[16px] font-semibold">{title}</h2>
      </div>
      {children && <div className="mt-3 pl-10 max-sm:pl-0">{children}</div>}
    </li>
  );
}

export function Btn({
  primary,
  danger,
  children,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { primary?: boolean; danger?: boolean }) {
  return (
    <button
      {...rest}
      className={`tech rounded-[10px] px-4 py-2.5 text-[13px] font-semibold transition disabled:opacity-40 ${
        primary
          ? "bg-accent text-black shadow-[0_0_14px_rgba(57,255,122,0.45)] hover:brightness-110"
          : danger
            ? "border border-red-500/50 text-red-300 hover:bg-red-500/10"
            : "border border-white/20 text-white hover:border-accent/60 hover:text-accent"
      } ${rest.className ?? ""}`}
    >
      {children}
    </button>
  );
}
