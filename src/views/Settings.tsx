import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Callouts, type Settings } from "../lib/api";
import { useStore } from "../lib/store";
import { Btn } from "./Connect";
import { Speaker } from "../components/icons";

const CALLOUTS: { key: keyof Callouts; label: string; hint: string }[] = [
  { key: "lapTimes", label: "Lap times", hint: "Every lap, plus personal bests and the final lap" },
  { key: "fuel", label: "Fuel", hint: "Laps of fuel left and critical warnings" },
  { key: "pitWindow", label: "Pit window", hint: "Window open, box this lap" },
  { key: "tyres", label: "Tyres", hint: "Overheating or cold tyres" },
  { key: "wheelSlip", label: "Wheelspin & lock-ups", hint: "Repeated slip on exit or under braking" },
  { key: "raceStart", label: "Race start", hint: "Lights out call" },
  { key: "position", label: "Grid position", hint: "Starting slot before the race" },
];

export function SettingsView() {
  const settings = useStore((s) => s.settings);
  const setSettings = useStore((s) => s.setSettings);
  const [voices, setVoices] = useState<string[]>([]);
  const [voicesDir, setVoicesDir] = useState("");
  const [testing, setTesting] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    api.listVoices().then(setVoices);
    api.voicesDir().then(setVoicesDir);
  }, [settings?.voice.modelPath]);

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
      filters: kind === "model" ? [{ name: "Piper voice", extensions: ["onnx"] }] : undefined,
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
      await api.testVoice("Radio check. Box this lap, box box.");
      setMessage({ ok: true, text: "Voice is working." });
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    } finally {
      setTesting(false);
    }
  };

  const v = settings.voice;
  const name = (p: string) => p.split(/[\\/]/).pop()?.replace(/\.onnx$/, "") ?? p;

  return (
    <div className="scroll-thin h-full overflow-y-auto bg-panel pb-32">
      <div className="mx-auto max-w-2xl px-6 pt-10">
        <h1 className="text-[32px] font-semibold tracking-tight">Settings</h1>

        <Section title="Connection">
          <Row label="PlayStation IP" hint="Saved automatically after the first connection">
            <input
              defaultValue={settings.lastIp ?? ""}
              placeholder="Auto-discover"
              onBlur={(e) => update((s) => void (s.lastIp = e.target.value.trim() || null))}
              className="num w-48 rounded-lg border border-white/10 bg-black/30 px-3 py-2 text-[14px] outline-none focus:border-accent"
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

        <Section title="Race engineer voice">
          <Row label="Voice callouts" hint="Spoken with Piper, fully offline">
            <Toggle checked={v.enabled} onChange={(c) => update((s) => void (s.voice.enabled = c))} />
          </Row>
          <Row label="Piper program" hint={v.piperPath || "Not found"}>
            <Btn onClick={() => browse("piper")}>Choose…</Btn>
          </Row>
          <Row label="Voice" hint={v.modelPath ? v.modelPath : `Put .onnx voices in ${voicesDir}`}>
            <div className="flex items-center gap-2">
              {voices.length > 0 && (
                <select
                  value={v.modelPath}
                  onChange={(e) => update((s) => void (s.voice.modelPath = e.target.value))}
                  className="max-w-52 rounded-lg border border-white/10 bg-black/30 px-3 py-2 text-[14px] outline-none"
                >
                  {!voices.includes(v.modelPath) && <option value={v.modelPath}>{v.modelPath ? name(v.modelPath) : "Select…"}</option>}
                  {voices.map((p) => (
                    <option key={p} value={p}>
                      {name(p)}
                    </option>
                  ))}
                </select>
              )}
              <Btn onClick={() => browse("model")}>Browse…</Btn>
            </div>
          </Row>
          <Row label="Volume">
            <Slider value={v.volume} min={0} max={1} step={0.05} onChange={(x) => update((s) => void (s.voice.volume = x))} />
          </Row>
          <Row label="Speaking speed">
            <Slider
              value={2 - v.lengthScale}
              min={0.6}
              max={1.4}
              step={0.05}
              onChange={(x) => update((s) => void (s.voice.lengthScale = Math.round((2 - x) * 100) / 100))}
            />
          </Row>
          <div className="flex items-center gap-3 py-3">
            <Btn primary onClick={test} disabled={testing} className="flex items-center gap-2">
              <Speaker width={18} height={18} /> {testing ? "Generating…" : "Test voice"}
            </Btn>
            {message && <span className={`text-[13px] ${message.ok ? "text-green-400" : "text-red-300"}`}>{message.text}</span>}
          </div>
        </Section>

        <Section title="Callouts">
          {CALLOUTS.map((c) => (
            <Row key={c.key} label={c.label} hint={c.hint}>
              <Toggle
                checked={v.callouts[c.key]}
                onChange={(on) => update((s) => void (s.voice.callouts[c.key] = on))}
              />
            </Row>
          ))}
        </Section>

        <p className="mt-8 text-center text-[12px] text-white/35">ApexNorth · Eleven North · GT7 telemetry over UDP 33739/33740</p>
      </div>
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="mt-8">
      <h2 className="mb-2 text-[13px] font-semibold tracking-[0.08em] text-white/50">{title.toUpperCase()}</h2>
      <div className="divide-y divide-white/5 rounded-2xl border border-white/10 bg-[#232634] px-5">{children}</div>
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
      className={`relative h-7 w-12 rounded-full transition ${checked ? "bg-accent" : "bg-white/15"}`}
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
      className="w-44 accent-[#b18cf2]"
    />
  );
}

function Segmented({ value, options, onChange }: { value: string; options: [string, string][]; onChange: (v: string) => void }) {
  return (
    <div className="inline-flex rounded-xl bg-black/30 p-1">
      {options.map(([v, label]) => (
        <button
          key={v}
          onClick={() => onChange(v)}
          className={`rounded-lg px-3 py-1.5 text-[13px] font-medium ${value === v ? "bg-accent text-black" : "text-white/70"}`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
