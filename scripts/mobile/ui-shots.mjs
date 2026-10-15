// Screenshots of every view at phone and desktop sizes, with no Tauri
// backend: a fake __TAURI_INTERNALS__ answers commands from a demo-race
// fixture (made by the `ui_fixture` Rust test).
//
//   APEXNORTH_FIXTURE=/tmp/fixture.json cargo test --manifest-path src-tauri/Cargo.toml ui_fixture -- --ignored
//   npx vite --port 1420 &
//   node scripts/mobile/ui-shots.mjs /tmp/fixture.json /tmp/shots [http://localhost:1420]
//
// Needs Playwright (NODE_PATH pointing at a global install is fine).
import { readFileSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";

// require() so NODE_PATH can point at a global Playwright.
const { chromium } = createRequire(import.meta.url)("playwright");

const [fixturePath, outDir, url = "http://localhost:1420"] = process.argv.slice(2);
const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));
mkdirSync(outDir, { recursive: true });

const SIZES = {
  "phone-portrait": { width: 393, height: 852, mobile: true },
  "phone-landscape": { width: 852, height: 393, mobile: true },
  desktop: { width: 1280, height: 720, mobile: false },
};
const only = process.env.SIZES?.split(",");
const views = (process.env.VIEWS ?? "dashboard,connect,activity,session,lap,settings").split(",");

function mock({ fixture, mobile }) {
  const callbacks = new Map();
  const listeners = [];
  let next = 1;
  const session = fixture.sessions[0];
  const settings = {
    lastIp: "192.168.1.20",
    autoConnect: true,
    units: "metric",
    driverName: "",
    voice: {
      enabled: true,
      piperPath: "",
      modelPath: mobile ? "system:" : "kokoro:bf_emma",
      volume: 0.9,
      lengthScale: 0.9,
      chatter: "normal",
      callouts: { lapTimes: true, sectors: true, pace: true, fuel: true, pitStrategy: true, tyres: true, wheelSlip: true, incidents: true, raceStart: true, position: true },
    },
  };
  const handlers = {
    build_info: () => ({ channel: "stable", build: null, os: mobile ? "android" : "macos", mobile }),
    check_update: () => null,
    get_status: () => ({ state: "live", ip: "192.168.1.20", variant: "Tilde", hz: 59.9, error: null }),
    get_settings: () => settings,
    save_settings: () => null,
    local_ips: () => ["192.168.1.31"],
    list_sessions: () => fixture.sessions,
    session_laps: () => fixture.laps,
    track_laps: () => fixture.laps.filter((l) => l.isValid),
    get_lap: ({ lapId }) => fixture.details.find((d) => d.id === lapId) ?? fixture.details[1],
    track_catalog: () => [],
    list_voices: () =>
      mobile
        ? [
            { value: "system:", label: "Automatic", group: "system" },
            { value: "system:en-gb-x-gbd-local", label: "United Kingdom · GBD", group: "system" },
          ]
        : [{ value: "kokoro:bf_emma", label: "Emma (British)", group: "kokoro" }],
    voices_dir: () => "/data/voices",
    voice_builtin: () => !mobile,
    demo_running: () => false,
    "plugin:app|version": () => "0.3.1",
    "plugin:event|listen": ({ event, handler }) => {
      listeners.push({ event, handler });
      return next++;
    },
    "plugin:event|unlisten": () => null,
  };
  window.__TAURI_INTERNALS__ = {
    transformCallback(cb) {
      const id = next++;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    async invoke(cmd, args) {
      const h = handlers[cmd];
      if (!h) {
        console.warn("mock: unhandled", cmd);
        return null;
      }
      return h(args ?? {});
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    convertFileSrc: (p) => p,
  };
  window.__TAURI_OS_PLUGIN_INTERNALS__ = {};
  window.__emit = (event, payload) => {
    for (const l of listeners) if (l.event === event) callbacks.get(l.handler)?.({ event, id: 0, payload });
  };
}

const browser = await chromium.launch();
for (const [name, size] of Object.entries(SIZES)) {
  if (only && !only.includes(name)) continue;
  const page = await browser.newPage({
    viewport: { width: size.width, height: size.height },
    deviceScaleFactor: 2,
    isMobile: size.mobile,
    hasTouch: size.mobile,
    locale: "en-US",
  });
  page.on("pageerror", (e) => console.error(`[${name}] page error:`, e.message));
  page.on("console", (m) => m.type() === "error" && console.error(`[${name}] console:`, m.text()));
  await page.addInitScript(mock, { fixture, mobile: size.mobile });
  await page.goto(url);
  await page.waitForTimeout(1500);
  const live = fixture.live;
  const go = (v) => page.evaluate((v) => import("/src/lib/store.ts").then((m) => m.useStore.getState().go(v)), v);
  const frame = () =>
    page.evaluate((live) => {
      window.__emit("connection", { state: "live", ip: "192.168.1.20", variant: "Tilde", hz: 59.9, error: null });
      window.__emit("telemetry", live);
      window.__emit("engineer", "Sector two, 4 tenths up.");
    }, live);
  const targets = {
    dashboard: { name: "dashboard" },
    connect: { name: "connect" },
    activity: { name: "activity" },
    session: { name: "session", sessionId: fixture.sessions[0].id },
    lap: { name: "lap", lapId: fixture.details[1].id, sessionId: fixture.sessions[0].id },
    settings: { name: "settings" },
  };
  for (const v of views) {
    await go(targets[v]);
    await frame();
    await page.waitForTimeout(700);
    await page.screenshot({ path: `${outDir}/${name}-${v}.png` });
    // Anything wider than the screen means sideways scrolling on a phone.
    const overflow = await page.evaluate(() => {
      const w = document.documentElement.clientWidth;
      return [...document.querySelectorAll("body *")]
        .filter((e) => {
          const r = e.getBoundingClientRect();
          return r.width > 0 && r.right > w + 1 && getComputedStyle(e).position !== "fixed";
        })
        .slice(0, 3)
        .map((e) => `${e.tagName.toLowerCase()}.${String(e.className).slice(0, 60)} right=${Math.round(e.getBoundingClientRect().right)}`);
    });
    if (overflow.length && v !== "dashboard") console.log(`[${name}/${v}] wider than the screen:`, overflow.join(" | "));
  }
  await page.close();
}
await browser.close();
console.log(`screenshots in ${outDir}`);
