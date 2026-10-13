import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";
import { api, type BuildInfo, type Callouts, type ChatterLevel, type Settings, type VoiceOption } from "../lib/api";
import { useStore } from "../lib/store";
import { Btn } from "./Connect";
import { Speaker } from "../components/icons";
import { Brand } from "../components/Brand";
import { UpdateButton } from "../components/UpdateButton";
import { useUpdates } from "../lib/updates";

const CALLOUTS: { key: keyof Callouts; label: string; hint: string }[] = [
  { key: "lapTimes", label: "Lap times", hint: "Every lap, personal bests and the final lap" },
  { key: "sectors", label: "Sector coaching", hint: "Time gained or lost per sector, purple sectors" },
  { key: "pace", label: "Pace & encouragement", hint: "Consistency, improving trends, a strong lap building" },
  { key: "fuel", label: "Fuel", hint: "Usage per lap, laps left, critical warnings" },
  { key: "pitStrategy", label: "Pit strategy", hint: "Pit window, target lap, box calls" },
  { key: "tyres", label: "Tyres", hint: "Temperatures, imbalance, estimated degradation" },
  { key: "wheelSlip", label: "Wheelspin & lock-ups", hint: "Big slides and repeated slip" },
  { key: "incidents", label: "Incidents", hint: "Spins and what to watch after one" },
  { key: "raceStart", label: "Race start", hint: "Lights out call" },
  { key: "position", label: "Race position", hint: "Grid slot, places gained or lost, chasing the next car" },
];

const VOICE_GROUPS: { group: VoiceOption["group"]; label: string }[] = [
  { group: "kokoro", label: "Kokoro" },
  { group: "kokoroMore", label: "More Kokoro voices" },
  { group: "custom", label: "Your Kokoro voices" },
  { group: "piper", label: "Your Piper voices" },
];

const CHATTER: { value: ChatterLevel; label: string; hint: string }[] = [
  { value: "minimal", label: "Minimal", hint: "Critical information only." },
  { value: "normal", label: "Normal", hint: "Useful race updates without excessive chatter." },
  { value: "chatty", label: "Chatty", hint: "Frequent coaching and strategy updates." },
  { value: "crewChief", label: "Crew Chief", hint: "Maximum useful race-engineer feedback." },
];

/** Same as `VoiceSettings::default()` in settings.rs. */
const DEFAULT_VOLUME = 0.9;
const DEFAULT_LENGTH_SCALE = 0.9;

export function SettingsView() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const [voices, setVoices] = useState<VoiceOption[]>([]);
  const [voicesDir, setVoicesDir] = useState("");
  const [builtin, setBuiltin] = useState(false);
  const [version, setVersion] = useState("");
  const [build, setBuild] = useState<BuildInfo | null>(null);
  const [testing, setTesting] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const loadVoices = () => void api.listVoices().then(setVoices);
  useEffect(loadVoices, [settings?.voice.modelPath]);
  useEffect(() => {
    api.voicesDir().then(setVoicesDir);
    api.voiceBuiltin().then(setBuiltin);
    getVersion().then(setVersion);
    api.buildInfo().then(setBuild);
    // Pick up voice files dropped into the folder while the app was in the background.
    window.addEventListener("focus", loadVoices);
    return () => window.removeEventListener("focus", loadVoices);
  }, []);

  if (!settings) return null;

  const update = (fn: (s: Settings) => void) => {
    const next: Settings = structuredClone(settings);
    fn(next);
    setSettings(next).catch((e) => setMessage({ ok: false, text: String(e) }));
  };

  const browse = async (kind: "piper" | "model") => {
    const path = await open({
      multiple: false,
      directory: false,
      filters: kind === "model" ? [{ name: "Voice (Kokoro .pt/.npy/.bin, Piper .onnx)", extensions: ["pt", "pth", "npy", "bin", "onnx"] }] : undefined,
    });
    if (typeof path === "string") {
      update((s) => {
        if (kind === "model") s.voice.modelPath = path;
        else s.voice.piperPath = path;
      });
    }
  };

  const test = async () => {
    setTesting(true);
    setMessage(null);
    try {
      const name = settings.driverName?.trim();
      await api.testVoice(name ? `Radio check, ${name}. Box this lap, box box.` : "Radio check. Box this lap, box box.");
      setMessage({ ok: true, text: "Voice is working." });
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    } finally {
      setTesting(false);
    }
  };

  const v = settings.voice;
  const file = (p: string) => p.split(/[\\/]/).pop()?.replace(/\.onnx$/, "") ?? p;
  const current = voices.find((o) => o.value === v.modelPath);
  const voiceHint = !current
    ? v.modelPath || `Put voices in ${voicesDir}`
    : current.group === "piper"
      ? "Piper voice"
      : current.group === "custom"
        ? `Kokoro voice file: ${v.modelPath}`
        : "Kokoro, built in";

  return (
    <div className="scroll-thin h-full overflow-y-auto pb-16">
      <div className="mx-auto max-w-3xl px-6 pt-8">
        <h1 className="brand text-[44px] leading-none">
          SET<span className="text-accent drop-shadow-[0_0_12px_rgba(57,255,122,0.5)]">TINGS</span>
        </h1>
        <div className="tech mt-2 text-[13px] italic tracking-[0.3em] text-white/60">Connection, units and your race engineer</div>

        <Section title="Connection" color="var(--color-pos)">
          <Row label="PlayStation IP" hint="Saved automatically after the first connection">
            <input
              defaultValue={settings.lastIp ?? ""}
              placeholder="Auto-discover"
              onBlur={(e) => update((s) => void (s.lastIp = e.target.value.trim() || null))}
              className="num w-48 rounded-lg border border-white/10 bg-black/50 px-3 py-2 text-[14px] outline-none focus:border-accent"
            />
          </Row>
          <Row label="Connect on launch">
            <Toggle checked={settings.autoConnect} onChange={(c) => update((s) => void (s.autoConnect = c))} />
          </Row>
          <Row label="Units">
            <Segmented
              value={settings.units}
              options={[
                ["metric", "km/h"],
                ["imperial", "mph"],
              ]}
              onChange={(u) => update((s) => void (s.units = u as Settings["units"]))}
            />
          </Row>
        </Section>

        <Section title="Race engineer voice" color="var(--color-thr)">
          <Row label="Voice callouts" hint={builtin ? "Built-in Kokoro voices, fully offline. Nothing to install." : "Spoken with Piper, fully offline"}>
            <Toggle checked={v.enabled} onChange={(c) => update((s) => void (s.voice.enabled = c))} />
          </Row>
          {!builtin && (
            <Row label="Piper program" hint={v.piperPath || "Not found"}>
              <Btn onClick={() => browse("piper")}>Choose…</Btn>
            </Row>
          )}
          <Row label="Voice" hint={voiceHint}>
            <div className="flex items-center gap-2">
              {voices.length > 0 && (
                <select
                  value={v.modelPath}
                  onChange={(e) => update((s) => void (s.voice.modelPath = e.target.value))}
                  className="max-w-60 rounded-lg border border-white/10 bg-black/50 px-3 py-2 text-[14px] outline-none focus:border-accent"
                >
                  {!current && <option value={v.modelPath}>{v.modelPath ? file(v.modelPath) : "Select…"}</option>}
                  {VOICE_GROUPS.map(({ group, label }) => {
                    const list = voices.filter((o) => o.group === group);
                    return (
                      list.length > 0 && (
                        <optgroup key={group} label={label}>
                          {list.map((o) => (
                            <option key={o.value} value={o.value}>
                              {o.label}
                            </option>
                          ))}
                        </optgroup>
                      )
                    );
                  })}
                </select>
              )}
              <Btn onClick={() => browse("model")}>Browse…</Btn>
            </div>
          </Row>
          {builtin && (
            <Row
              label="Add your own voices"
              hint="Kokoro .pt / .npy files (or your own Piper .onnx) in the voices folder join the list"
            >
              <Btn onClick={() => voicesDir && revealItemInDir(voicesDir).catch((e) => setMessage({ ok: false, text: String(e) }))}>Open folder</Btn>
            </Row>
          )}
          <Row label="Volume">
            <div className="flex items-center gap-3">
              <ResetDefault show={v.volume !== DEFAULT_VOLUME} onClick={() => update((s) => void (s.voice.volume = DEFAULT_VOLUME))} />
              <Slider value={v.volume} min={0} max={1} step={0.05} onChange={(x) => update((s) => void (s.voice.volume = x))} />
            </div>
          </Row>
          <Row label="Speaking speed">
            <div className="flex items-center gap-3">
              <ResetDefault
                show={v.lengthScale !== DEFAULT_LENGTH_SCALE}
                onClick={() => update((s) => void (s.voice.lengthScale = DEFAULT_LENGTH_SCALE))}
              />
              <Slider
              value={2 - v.lengthScale}
              min={0.6}
              max={1.4}
              step={0.05}
              onChange={(x) => update((s) => void (s.voice.lengthScale = Math.round((2 - x) * 100) / 100))}
              />
            </div>
          </Row>
          <div className="flex items-center gap-3 py-3">
            <Btn primary onClick={test} disabled={testing} className="flex items-center gap-2">
              <Speaker width={18} height={18} /> {testing ? "Generating…" : "Test voice"}
            </Btn>
            {message && <span className={`text-[13px] ${message.ok ? "text-green-400" : "text-red-300"}`}>{message.text}</span>}
          </div>
        </Section>

        <Section title="Race engineer" color="var(--color-lap)">
          <Row label="Driver name" hint="Your engineer uses it now and then: “Good luck out there, Michael.”">
            <input
              defaultValue={settings.driverName ?? ""}
              placeholder="Your name"
              maxLength={24}
              onBlur={(e) => update((s) => void (s.driverName = e.target.value.trim()))}
              onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
              className="w-48 rounded-lg border border-white/10 bg-black/50 px-3 py-2 text-[14px] outline-none focus:border-accent"
            />
          </Row>
          <Row label="Engineer chatter" hint={CHATTER.find((c) => c.value === v.chatter)?.hint}>
            <Segmented
              value={v.chatter}
              options={CHATTER.map((c) => [c.value, c.label])}
              onChange={(c) => update((s) => void (s.voice.chatter = c as ChatterLevel))}
            />
          </Row>
        </Section>

        <Section title="Callout categories" color="var(--color-laps)">
          {CALLOUTS.map((c) => (
            <Row key={c.key} label={c.label} hint={c.hint}>
              <Toggle
                checked={v.callouts[c.key]}
                onChange={(on) => update((s) => void (s.voice.callouts[c.key] = on))}
              />
            </Row>
          ))}
        </Section>

        <Section title="Version & updates" color="var(--color-accent)">
          <Updates version={version} build={build} />
        </Section>

        <div className="mt-10 flex flex-col items-center gap-2 opacity-60">
          <Brand scale={0.6} />
          <p className="tech text-[11px] text-white/50">
            {version && `v${version} · `}Eleven North · GT7 telemetry over UDP 33739/33740
          </p>
        </div>
      </div>
    </div>
  );
}

/** This app's version and channel, a manual check, and the update when there is one. */
function Updates({ version, build }: { version: string; build: BuildInfo | null }) {
  const offer = useUpdates((s) => s.offer);
  const check = useUpdates((s) => s.check);
  const install = useUpdates((s) => s.install);
  const checkNow = useUpdates((s) => s.checkNow);
  const beta = build?.channel === "beta";
  const label = `ApexNorth v${version}${beta && build?.build ? ` beta ${build.build}` : ""}`;
  const channel = beta ? "Beta channel: test builds from the beta branch" : "Stable channel: regular releases";
  return (
    <>
      <Row label={label} hint={channel}>
        <div className="flex items-center gap-3">
          {check === "current" && !offer && <span className="tech text-[12px] text-accent">Up to date</span>}
          <Btn onClick={() => void checkNow()} disabled={check === "checking"}>
            {check === "checking" ? "Checking…" : "Check for updates"}
          </Btn>
        </div>
      </Row>
      {offer && (
        <Row
          label={`Update available: v${offer.version}`}
          hint={offer.manual ? "Opens the download page" : (offer.notes ?? "Downloads, installs and restarts the app")}
        >
          <UpdateButton />
        </Row>
      )}
      {install.step === "failed" && (
        <p className="pb-3 text-[12px] text-[#ff9f1a]">Update failed: {install.error}</p>
      )}
    </>
  );
}

/** Puts a slider back to its default; only shown once it has been moved. */
function ResetDefault({ show, onClick }: { show: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      title="Back to the default"
      className={`tech text-[12px] text-white/50 transition hover:text-accent ${show ? "" : "invisible"}`}
    >
      Reset
    </button>
  );
}

function Section({ title, color, children }: { title: string; color: string; children: React.ReactNode }) {
  return (
    <section className="relative mt-9">
      <h2 className="tech absolute left-5 top-0 z-10 -translate-y-1/2 bg-ink px-2 text-[13px] font-semibold text-white">{title}</h2>
      <div
        className="divide-y divide-white/[0.06] rounded-[14px] px-5 pt-2"
        style={{
          border: `1.5px solid color-mix(in srgb, ${color} 75%, transparent)`,
          background: "linear-gradient(180deg, rgba(16,24,22,0.9), rgba(5,8,7,0.95))",
          boxShadow: `0 0 14px color-mix(in srgb, ${color} 22%, transparent)`,
        }}
      >
        {children}
      </div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6 py-3.5">
      <div className="min-w-0">
        <div className="text-[15px] font-medium">{label}</div>
        {hint && <div className="truncate text-[12px] text-white/45">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (c: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative h-7 w-12 rounded-full transition ${checked ? "bg-accent shadow-[0_0_10px_rgba(57,255,122,0.6)]" : "bg-white/15"}`}
    >
      <span className={`absolute top-1 h-5 w-5 rounded-full bg-white transition-all ${checked ? "left-6" : "left-1"}`} />
    </button>
  );
}

function Slider(props: { value: number; min: number; max: number; step: number; onChange: (v: number) => void }) {
  const [local, setLocal] = useState(props.value);
  useEffect(() => setLocal(props.value), [props.value]);
  return (
    <input
      type="range"
      min={props.min}
      max={props.max}
      step={props.step}
      value={local}
      onChange={(e) => setLocal(Number(e.target.value))}
      onPointerUp={() => props.onChange(local)}
      onKeyUp={() => props.onChange(local)}
      className="w-44 accent-[#39ff7a]"
    />
  );
}

function Segmented({ value, options, onChange }: { value: string; options: [string, string][]; onChange: (v: string) => void }) {
  return (
    <div className="inline-flex rounded-xl border border-white/10 bg-black/40 p-1">
      {options.map(([v, label]) => (
        <button
          key={v}
          onClick={() => onChange(v)}
          className={`tech rounded-lg px-3 py-1.5 text-[12px] font-semibold ${value === v ? "bg-accent text-black shadow-[0_0_10px_rgba(57,255,122,0.5)]" : "text-white/70 hover:text-white"}`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
