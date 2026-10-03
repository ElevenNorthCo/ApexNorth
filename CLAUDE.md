# ApexNorth: guide for Claude

ApexNorth is a GT7 live telemetry dashboard and race engineer. It is a Tauri 2 desktop app (Rust backend, React + TypeScript frontend) for macOS, Windows and Linux. It is modeled on the free tier of the iOS app "Victory: Race Engineer for GT7". The long-term goal is an AI race engineer you can chat with.

The README has the full feature list, the protocol details, and the **phase roadmap**. Check it before starting new feature work, and tick roadmap items off when they ship.

## Commands

```bash
npm run tauri dev                 # run the app (hot reload: UI instantly, Rust triggers a rebuild + restart)
cd src-tauri && cargo test        # Rust unit tests (fast, ~63 tests)
npx tsc --noEmit                  # type-check the frontend
npm run build                     # frontend production build
npm run tauri build -- --bundles app,dmg   # macOS release → src-tauri/target/release/bundle/
```

Before calling a change done:
- Run `cargo test` for Rust changes.
- Run `npx tsc --noEmit` for frontend changes.

Notes on running and building:
- **Only one copy of the app can run at a time.** It binds UDP port 33740. Quit the installed app before `tauri dev`, or the link reports "Could not open UDP port".
- **Background `tauri dev` gets killed at the tool time limit.** If Claude starts it in the background, tell the user. For long play sessions the user should run it in their own terminal.
- **The release build takes about 4–5 minutes** with LTO and is Intel x64 on this Mac.

## Testing without a PlayStation

On the Connect screen, choose **Try demo mode** (the `start_demo` command). This starts `telemetry/sim.rs`, a fake PS on 127.0.0.1. It answers heartbeats with encrypted packets from a 10-lap race on a generated ~3.7 km track.

The demo race includes a grid start, fuel burn, tyre wear (pace fades about 0.8% a lap), wheelspin and lock-ups, a pit stop, and a spin on lap 8. Use it to check dashboard, recording, analysis and voice changes from start to finish.

The sim tests in `sim.rs` also run the engine and engineer headlessly. `cargo test simulated_race_engineer -- --nocapture` prints the full Crew Chief transcript for a race, which is the quickest way to judge a callout change.

## Architecture

```
telemetry/udp.rs   link thread: heartbeat → PS:33739, recv :33740, LAN discovery, ConnStatus
  ├ crypto.rs      Salsa20 decrypt; variant by size (A 296 B / B 316 B / ~ 344 B)
  ├ packet.rs      Packet::parse (+ to_bytes for the sim). Offsets verified on a real PS5.
  └ Handler (lib.rs LiveHandler) per packet:
       session/engine.rs  → LiveState + EngineEvents (SessionStarted/Ended, RaceStart, LapCompleted,
                            SectorCompleted, LapInvalidated, Incident, PositionChanged, RacePositionChanged)
         ├ fuel.rs        FuelModel (+ fuel-off detection) → RaceStrategy (window, stops, target)
         ├ tyres.rs       TyreModel → TyreEstimate (lap-time trend, fuel-corrected)
         ├ strategy.rs    fuel + tyres → PitPlan
         └ incidents.rs   SpinDetector
       storage/mod.rs     → SQLite (tracks, sessions, laps; samples = zstd(bincode(Vec<Sample>)))
         └ catalog/       built-in GT7 data: cars.rs (car_code → name), tracks.rs (121 layouts, lap → layout)
       engineer/rules.rs  → candidate Calls (+ live.rs: tyre temps, wheel slip)
         → director.rs    chatter level, category toggle, anti-nag cooldown, routine gap, calm-moment hold
         → voice.rs       priority queue (High clears Low) → piper CLI → cached WAV → rodio
       emits Tauri events: "telemetry" (LiveState, 30 Hz), "connection", "lap", "lap_invalid", "engineer", "voice_error"
```

### Backend (Rust) rules
- Heavy logic stays in Rust. The React UI only renders.
- **Lap timing:**
  - The current lap time comes from `packet_id` ticks (60 Hz).
  - A finished lap uses the game's `last_lap_ms`. Sample `t` values are rescaled to match it.
  - A lap is invalid only with an `InvalidReason` (joined mid-lap, rewind/reset, confirmed telemetry jump, too short, pit lap). A position jump has to be impossible over the real elapsed time (packet-id gap) *and* confirmed by 3 packets. Stale or reordered packets are dropped. Don't loosen this back to single-packet checks; it made real laps "not count".
- **Track identity** (GT7 sends no track ID):
  - First, the lap's (x, z) line is matched against `catalog/tracks.rs`. It needs two-way coverage, the same direction, and the least divergence, which tells chicane variants apart.
  - Matched tracks store `tracks.layout_id`. Otherwise it falls back to a fingerprint of start/finish position plus lap length, with a placeholder name. See `Db::identify_track`.
  - On launch, `Db::name_known_tracks` renames placeholder tracks and gives tracks to sessions that lacked one.
  - User-typed names are never overwritten. Picking a catalog name in rename links the layout.
- **Car names** come from `catalog/cars.rs` and are sent as `carName` on sessions and laps.
- **Refresh the catalogs** with `scripts/build_gt7_data.py <dir with gt7info + gt7-datalogger-track-data clones>`.
- **Settings** (`settings.rs`) are JSON in the app config dir, held in `Arc<RwLock<Settings>>`.
- **Adding a setting:**
  1. Add the field with a `#[serde(default)]`-compatible default.
  2. Mirror it in `src/lib/api.ts`.
- **Adding a command:**
  1. Write the `#[tauri::command]` in `lib.rs`.
  2. Register it in `generate_handler!`.
  3. Add a typed wrapper in `src/lib/api.ts` (`api.*`).
- Serde uses `rename_all = "camelCase"` for everything sent to the UI.
- **Adding a callout:**
  1. Emit a `Call` from `engineer/rules.rs` (events) or `engineer/live.rs` (60 Hz state). Give it a `Topic`, the lowest `ChatterLevel` that should hear it, a priority, a fact `key` and a cooldown. Speak on changes of state, not on a timer.
  2. The `Topic` maps to a `Callouts` toggle in `settings.rs`. For a new category, add the toggle there, in `src/lib/api.ts`, and as a row in `CALLOUTS` in `src/views/Settings.tsx`.
  3. Use `Phrases::pick` for wordings that repeat. Vary the words only, never the facts.
  4. Write the text the way it should be spoken: `speak_time()` gives "1 34 point 7", not "1:34.7". Use `speak_gap`, `speak_laps`, and `word()` for sector numbers.
  5. Check it in the demo transcript test.

### Frontend (React) rules
- **Navigation** is a simple view state in the Zustand store (`src/lib/store.ts`, `go({ name: ... })`), not a router.
  - Views live in `src/views/`, shared pieces in `src/components/`.
  - `startBridge()` wires backend events into the store.
- **Dashboard (`views/Dashboard.tsx`):**
  - It is laid out on a fixed **1000×540 design canvas** with absolute pixel positions, scaled by `ScaleToFit` (ApexNorth's own neon layout, not Victory's).
  - Its logo drops `components/TopBar.tsx` down over it; every other view shows the TopBar permanently (Lap analysis has none).
  - Tweak positions and sizes in those canvas pixels. Check text fits inside the tiles (fuel labels and lap time padding have needed fixes before).
- **Styling:**
  - Tailwind v4, with theme tokens in `src/index.css` (`@theme`).
  - Tile colors: `--color-pos/lap/laps/laptime/brk/thr/fuel`; accent is neon green `--color-accent`.
  - Fonts: `.tech` (Chakra Petch, uppercase labels/tabs), `.brand` (Saira italic 900, wordmark and page titles), `.an-bg` page backdrop.
  - `components/Tile.tsx` is the neon panel (`notch` puts the label in the top border).
  - Numbers use the `.num` class (JetBrains Mono, slashed zero).
- **Artwork:** venue photos go in `src/assets/tracks/<venue-slug>.jpg` (file list in the README there); `lib/media.ts` picks them up by glob, with generated fallback art (`components/Art.tsx`). No car images: the user didn't like the generated silhouette, so cards show make and model text. Flags are SVGs in `components/Flag.tsx`.
- **Session type:** `sessions.kind` (practice / qualifying / race / time_trial, NULL = inferred from `totalLaps`), set with `set_session_kind`. Activity filters on it; Session view compares best pace per type at that track.
- **Charts:**
  - Charts use uPlot through `components/TelemetryChart.tsx`.
  - The `series` and `x` arrays must be memoized, or the chart rebuilds.
  - The cursor and marker are controlled by the parent.
- **Track map:**
  - `components/TrackMap.tsx` is a canvas that plots `(x, -z)`.
  - Orientation hasn't been checked against a real track yet. If it looks mirrored, flip one axis there and in `TrackOutline.tsx`.
- Don't use `window.alert` or `window.confirm`. Use inline confirms instead (see delete session in `views/Session.tsx`).

## GT7 facts that shape the design
- GT7 doesn't send:
  - a track ID or car name (only `car_code`; both come from the built-in catalog)
  - any data on other cars (no gaps ahead or behind), PP or tuning.
- Race position: `pre_race_position` updates live during races (confirmed on the user's PS5). The engine only emits `RacePositionChanged` after `POSITION_CONFIRM` (30) agreeing packets; `rules.rs` announces the net change once it's held for `POSITION_SETTLE` (3 s, 6 s in the opening 25 s). The demo sim scripts a running order to exercise this.
  - tyre wear
  - the current lap time
- `gear` 0 = reverse. If the `IN_GEAR` flag is clear, show N. `suggested_gear` 15 = none.
- The game streams to whoever sent the last heartbeat, and stops after a few seconds without one.

## User environment
- The user's PS5 is at 192.168.12.194, saved in settings as `lastIp`. They test live on the PS5.
- Piper:
  - The CLI is `~/.local/bin/piper` (Python, pipx). It takes about 2.5 s per new phrase.
  - Voices are in `~/Projects/ElevenNorth/PiperTest/models` (`en_US-ryan-high` is selected).
- App data is in `~/Library/Application Support/com.elevennorth.apexnorth/` (`apexnorth.db`, `settings.json`, `voices/`).

## Workflow
- Repo: private `github.com/ElevenNorthCo/ApexNorth`, branch `main`.
  - Commit and push when the user asks.
  - End commit messages with the Co-Authored-By line from the system attribution.
- **The user likes quick, small tweaks with visible results:**
  - Make the change.
  - Make sure it type-checks and tests pass.
  - Tell them what to look at.
- At the end of a meaningful session, offer a Brainlog. Use the `elevennorth-project-brainlog` skill, which saves to the vault `00_Inbox` via `~/.local/bin/brainlog`.
