# ApexNorth: guide for Claude

ApexNorth is a GT7 live telemetry dashboard and race engineer. It is a Tauri 2 desktop app (Rust backend, React + TypeScript frontend) for macOS, Windows and Linux. The long-term goal is an AI race engineer you can chat with.

The README has the full feature list, the protocol details, and the **phase roadmap**. Check it before starting new feature work, and tick roadmap items off when they ship.

## Commands

```bash
python3 scripts/fetch-voice.py    # once: built-in voice into src-tauri/resources/tts (git-ignored)
npm run tauri dev                 # run the app (hot reload: UI instantly, Rust triggers a rebuild + restart)
cd src-tauri && cargo test        # Rust unit tests (fast, ~64 tests)
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
         → voice.rs       priority queue (High clears Low) → Speaker → cached WAV → rodio
             └ tts.rs     built-in Piper: sherpa-onnx C API via libloading, voice kept loaded
                          (falls back to an external piper CLI when resources/tts is empty)
       emits Tauri events: "telemetry" (LiveState, 30 Hz), "connection", "lap", "lap_invalid", "engineer", "voice_error"
```

### Backend (Rust) rules
- Heavy logic stays in Rust. The React UI only renders.
- **Lap timing:**
  - Laps are joined at the line (`join_at_line` in `engine.rs`): a packet gap across the line is split with the official lap time, so the old lap ends and the new one starts (t = 0, d = 0) at the line. Before this, real PS5 laps started ~2.7 s / 150 m past the line.
  - The current lap time comes from `packet_id` ticks (60 Hz).
  - A finished lap uses the game's `last_lap_ms`. Sample `t` values are rescaled to match it.
  - A lap is invalid only with an `InvalidReason` (joined mid-lap, rewind/reset, confirmed telemetry jump, too short, pit lap). A position jump has to be impossible over the real elapsed time (packet-id gap) *and* confirmed by 3 packets. Stale or reordered packets are dropped. Don't loosen this back to single-packet checks; it made real laps "not count".
  - A grid-start lap (`standing_start`) stays the live reference but is held out of the session best until the next valid lap: under `GRID_LAP_MIN_FRACTION` (97 %) of that lap's length it's revoked (`EngineEvent::LapRevoked` → `Db::revoke_lap`, `insufficient_distance`). GT7's own `best_lap_ms` is ignored when it matches the revoked time. `Db::revoke_short_grid_laps` repairs old data on launch. Fuji's grid is ~300 m past the line.
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
  - It is laid out on a fixed **1000×540 design canvas** with absolute pixel positions, scaled by `ScaleToFit` (ApexNorth's neon layout).
  - Its logo drops `components/TopBar.tsx` down over it; every other view shows the TopBar permanently (Lap analysis has none).
  - Tweak positions and sizes in those canvas pixels. Check text fits inside the tiles (fuel labels and lap time padding have needed fixes before).
- **Styling:**
  - Tailwind v4, with theme tokens in `src/index.css` (`@theme`).
  - Tile colors: `--color-pos/lap/laps/laptime/brk/thr/fuel`; accent is neon green `--color-accent`.
  - Fonts: `.tech` (Chakra Petch, uppercase labels/tabs), `.brand` (Saira italic 900, wordmark and page titles), `.an-bg` page backdrop.
  - `components/Tile.tsx` is the neon panel (`notch` puts the label in the top border).
  - Numbers use the `.num` class (JetBrains Mono, slashed zero).
- **Artwork:** all 41 venues have a 1280×720 photo in `src/assets/tracks/<venue-slug>.jpg` (file list in the README there); `lib/media.ts` picks them up by glob. `TrackArt` (`components/Art.tsx`) shows it on Activity cards (`fade="right"`) and the Session header (`fade="left"`); unrecognised tracks get generated art. Don't add a Settings section for artwork (the user asked for none). Flags are SVGs in `components/Flag.tsx`.
- **Car images:**
  - 574 transparent 600×350 WebP renders live in `src/assets/cars/<slug>.webp` (about 19 MB, bundled). They are made in the separate repo `~/Projects/ElevenNorth/apexnorthCarAssets` (done, 574 of 574).
  - `src/lib/carImages.json` maps GT7 `car_code` → file name (built from that repo's `gt7-cars.csv`, `GT7 ID` column `carNNNN`). `carImage(code)` in `lib/media.ts` resolves it through a Vite glob. A code with no image (about 10 newer cars in `data/gt7_cars.csv`) just shows no image.
  - Shown as a thumbnail above the make/model text in the Activity card's Car column; the text stays. Not yet used on the Session header. The image is centred, up to 112 px tall, and shrinks (`flex-1 min-h-0`) so long car names and condition chips never overflow the 208 px card. The track outline sits in the title column next to the date (no column of its own), so "Fuji International Speedway" fits on one line at 21 px. The kind chip isn't repeated in the Car column (it's already in the date line).
  - To add or refresh images: copy the new `.webp` files into `src/assets/cars/` and add their `car_code` → file entries to `carImages.json`. Don't generate or fetch car art.
- **Race conditions:** `sessions.conditions` (JSON `SessionConditions`: mode offline/online, bop, tyreWear, fuelRate), set with `set_session_conditions` from the Session view; Activity shows them as chips. GT7 sends none of these. `LiveHandler::record_lap` re-reads them each lap and calls `Engine::set_conditions` (wear 0 → no tyre estimate, fuel 0 → no fuel plan, wear N → `Db::tyre_fade_per_wear` × N as a prior in `TyreModel`). Once the track is known, a session with none inherits them from the last session with the same track, car and `total_laps` (`Db::previous_conditions`). Pace comparisons use `conditionsMatch` (`lib/media.ts`): BoP, tyre wear and fuel must agree where both are noted.
- **Session type:** `sessions.kind` (practice / qualifying / race / time_trial, NULL = inferred from `totalLaps`), set with `set_session_kind`. Activity filters on it; Session view compares best pace per type at that track.
- **Charts:**
  - Charts use uPlot through `components/TelemetryChart.tsx`.
  - The `series` and `x` arrays must be memoized, or the chart rebuilds.
  - The cursor and marker are controlled by the parent.
- **Track map:**
  - `components/TrackMap.tsx` is a canvas that plots `(x, -z)`.
  - Orientation hasn't been checked against a real track yet. If it looks mirrored, flip one axis there and in `TrackOutline.tsx`.
- Don't use `window.alert` or `window.confirm`. Use inline confirms instead (see delete session in `views/Session.tsx`).

## Built-in voice & releases
- `engineer/tts.rs` loads sherpa-onnx **1.12.9** (`#[repr(C)]` structs must match that version's `c-api.h`; the npm packages `sherpa-onnx-<platform>@1.12.9` carry the libraries). Piper voices (`.onnx` + `.onnx.json`) are prepared once into the cache: metadata appended to the ONNX protobuf (field 14) and `tokens.txt` from the phoneme map (single-character tokens only: sherpa calls `exit()` on a bad token file, so validate before `SherpaOnnxCreateOfflineTts`).
- `scripts/fetch-voice.py --target <rust triple>` fills `src-tauri/resources/tts/` (`lib/`, `espeak-ng-data/`, `voices/en_US-ryan-high`). Bundled via `bundle.resources` → `tts/`; `voices::builtin_dir` finds it (resources, then the source tree in debug).
- `src-tauri/voices.json` is the voice catalog (bundled + 4 downloadable from HuggingFace `rhasspy/piper-voices` v1.0.0), read by Rust and the fetch script.
- `.github/workflows/release.yml`: tag `v*` → 4 installers with fixed names (`ApexNorth-macOS-AppleSilicon.dmg`, `-macOS-Intel.dmg`, `-Windows-Setup.exe`, `-Linux.AppImage`/`.deb`), voice smoke test (`speaks_with_the_bundled_voice`, ignored locally) on mac arm64, Windows, Linux, then a GitHub Release. Dash tags are pre-releases. **Actions → Release → Run workflow** with a version publishes too (the release action creates the tag); Claude's sessions can't push tags or dispatch workflows, but a pushed commit whose message contains `[build installers]` builds without publishing. `install.sh` / `install.ps1` download `releases/latest/download/<fixed name>`; `lib/updates.ts` shows the TopBar **Update** button from `releases/latest`. All of these need the repo to be public.
- macOS builds are ad-hoc signed (`signingIdentity: "-"`, `hardenedRuntime: false` so the bundled dylibs load).

## README screenshots
- `docs/screenshots/*.jpg` (dashboard, activity, session, lap-analysis) were taken from the Vite frontend in headless Chrome, with a mocked Tauri backend serving real sessions dumped from the user's DB, and a real Fuji SF19 race (session 22) as the showcase: lap 15 against lap 14 as a staged live frame. Drive it over CDP: inject a fake `__TAURI_INTERNALS__` (invoke + transformCallback, emit events by calling the stored listener callbacks) and switch views with `import('/src/lib/store.ts')`. Snapshot the DB with `sqlite3 "file:...apexnorth.db?mode=ro" ".backup copy.db"` (a plain copy misses the WAL), dump it through `storage::Db` with a temporary ignored test, and serve it to the mock. Last retaken 2026-10-09 at 1600×900, 1.5× scale. Retake them when the UI changes noticeably.

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
  - The CLI is `~/.local/bin/piper` (Python, pipx), only used when the built-in voice isn't fetched. It takes about 2.5 s per new phrase.
  - Voices are in `~/Projects/ElevenNorth/PiperTest/models` (`en_US-ryan-high` is selected).
- App data is in `~/Library/Application Support/com.elevennorth.apexnorth/` (`apexnorth.db`, `settings.json`, `voices/`).

## License
- ApexNorth is **PolyForm Noncommercial 1.0.0** (`LICENSE`, verbatim text plus `Required Notice:` lines). Eleven North keeps commercial rights (future paid hosted-AI tier). Contributions come in under the grant in `CONTRIBUTING.md` (PR template checkbox) so they can be relicensed.
- Third-party components and data are listed in `THIRD_PARTY_NOTICES.md`; add new bundled libraries, voices or datasets there. `.github/FUNDING.yml` drives the Sponsor button.

## Workflow
- Repo: private `github.com/ElevenNorthCo/ApexNorth`, branch `main`.
  - Commit and push when the user asks.
  - End commit messages with the Co-Authored-By line from the system attribution.
- **The user likes quick, small tweaks with visible results:**
  - Make the change.
  - Make sure it type-checks and tests pass.
  - Tell them what to look at.
- At the end of a meaningful session, offer a Brainlog. Use the `elevennorth-project-brainlog` skill, which saves to the vault `00_Inbox` via `~/.local/bin/brainlog`.
