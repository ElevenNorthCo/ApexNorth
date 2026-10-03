<p align="center">
  <img src="docs/icon.png" alt="ApexNorth" width="220" />
</p>

<h1 align="center">ApexNorth</h1>

<p align="center"><b>Live GT7 telemetry dashboard &amp; AI race engineer</b><br/>macOS · Windows · Linux</p>

---

**A live telemetry dashboard and AI race engineer for Gran Turismo 7.** It runs on macOS, Windows and Linux.

ApexNorth connects to your PlayStation over Wi-Fi with no extra hardware. It shows a clean real-time dashboard, records every lap automatically, and lets you replay and compare laps on a track map. A race engineer voice calls out lap times, fuel, the pit window and tyre status as you drive.

The long-term goal is a race engineer you can talk to: an AI that reads your telemetry, tells you where you're losing time, and coaches you to get faster.

> Built with Tauri 2 (Rust) and React + TypeScript. Voice is generated locally and offline with [Piper](https://github.com/rhasspy/piper).

---

## Contents
- [Features](#features)
- [Quick start](#quick-start)
- [Connecting to your PlayStation](#connecting-to-your-playstation)
- [Race engineer voice (Piper)](#race-engineer-voice-piper)
- [How it works](#how-it-works)
- [GT7 telemetry protocol](#gt7-telemetry-protocol)
- [Project structure](#project-structure)
- [Development](#development)
- [Roadmap: phases & milestones](#roadmap-phases--milestones)
- [Known limitations](#known-limitations)
- [Credits](#credits)

---

## Features

### Live dashboard
- Position, lap and total laps.
- A large gear indicator with RPM, suggested shift, and speed in km/h or mph.
- A 16-LED shift light bar driven by the car's own rev alert range. It flashes at the limiter.
- Current, last and best lap times, with a **live delta** to your best lap measured by distance.
- Tyre temperatures for all four corners, colour-coded cold / optimal / hot.
- Brake and throttle readouts with fill bars.
- Fuel: percentage left, % used per lap, laps of fuel left, and pit window status.
- Side indicator LEDs: TCS, ASM, pit window open, box now, ahead of best, tyres hot, wheelspin and lock-up.
- A connection status readout (`SEARCHING`, `LIVE 60HZ`, `SIGNAL LOST`) and a clock.
- The screen stays awake while data is live.

### Simple connection
- A three-step wizard: get on track, find the PlayStation, connected.
- **Auto-discovery** finds the PS5/PS4 on your LAN. You can also enter the IP manually.
- The console's IP is remembered and reconnects automatically on launch.
- Supports all three GT7 packet formats (`A`, `B`, `~`) and falls back automatically for older game versions.
- **Demo mode** runs a built-in simulator so you can try the whole app with no PlayStation.

### Session recording & history
- Every lap is recorded automatically at 30 Hz and saved locally in SQLite.
- Sessions are grouped by track and date, with a track outline thumbnail, lap count, best lap and car.
- The lap table shows 5 sector splits, with the purple best lap and purple best sectors highlighted. It also shows fuel used per lap.
- **Pit detection:** a refuel or a stop flags the lap as a pit lap and keeps it out of best-lap records.
- Invalid laps (a rewind or reset, or a lap you joined mid-way) are flagged and excluded.
- **Real track and car names.** GT7 doesn't send a track ID, so ApexNorth recognises the track from the line you drove. It checks the lap against a built-in catalog of all 121 GT7 layouts, including reverse layouts and chicane / no-chicane variants. Cars are named from the full GT7 car list (584 cars), e.g. "Porsche 911 GT3 RS (992) '22".
- A track the catalog can't place gets a placeholder name. Rename it from the session page, where the box suggests every GT7 track.

### Lap analysis & playback
- A track map showing the track surface, your racing line, sector markers and the car position, with zoom that follows the car and an overview mini-map.
- **Playback** animates the lap in real time at 1×, 2× or 4×.
- **Compare** against any lap on the same track, from any session:
  - sector-by-sector deltas
  - a ghost line and ghost car on the map
  - overlaid traces
- Synced charts for delta, speed, throttle, brake and gear. Hover any chart to scrub the map.
- Sector filter (All, or S1–S5) zooms both the charts and the map.

### Race engineer voice
Callouts are spoken offline with Piper. The engineer follows the session in real time: sector by sector, through fuel and tyre strategy, to the flag.

**Engineer chatter** sets how often it talks: *Minimal* (critical information only), *Normal*, *Chatty* or *Crew Chief*. **Callout categories** set what it talks about, and a category that's off stays off at every chatter level.

| Category | Example |
|---|---|
| Lap times | "1 34 point 7. Three tenths off your best." / "Purple lap! New personal best by four tenths." / "Final lap." |
| Sector coaching | "Sector one, two tenths up." / "Purple sector." / "You lost three tenths there." |
| Pace & encouragement | "Good pace. Keep it up." / "That's good consistency." / "Good recovery. You made most of that time back." |
| Fuel | "Fuel usage is 9.6 percent per lap. One stop expected." / "Fuel for about six laps." / "Fuel is critical." |
| Pit strategy | "Pit window laps 5 to 7. Lap 6 is the target." / "Fuel usage has increased. Target stop is now lap 5." / "Box this lap." |
| Tyres | "Rear tyres are getting hot." / "Front-left is running hotter than the others." / "Tyre performance is starting to drop." |
| Wheelspin & lock-ups | "Wheelspin on exit." / "You're consistently locking the fronts. Ease the brake pressure." |
| Incidents | "You've had a spin." / "Take it easy for the next couple of corners." |
| Race start | "Lights out. Go, go, go!" |
| Grid position | "You start P5 of 16." |

How it stays useful rather than noisy:
- **Anti-nag memory:** each fact is remembered when spoken. It isn't repeated unless it gets worse, changes, or enough time has passed.
- **Priorities:** box calls, critical fuel, overheating and spins speak immediately and clear queued chatter.
- **Context timing:** routine calls wait (briefly) for a straight, not mid-braking or mid-corner.
- **Phrase variation:** common calls rotate between equivalent wordings. Facts and numbers never change.

### Strategy engine
- **Fuel:** a rolling average that reacts quickly to a real change in consumption. Strategy switches off automatically when the game isn't burning fuel (time trials), so you never hear "hundreds of laps of fuel".
- **Pit window:** earliest, target and latest pit lap, plus the number of stops, from the current fuel, burn rate, full-tank range, laps remaining and a 0.3-lap reserve.
- **Tyre degradation (estimated):** GT7 sends no wear data, so ApexNorth reads the stint's lap-time trend, corrected for the car getting lighter as fuel burns. It reports *Fresh / Stable / Declining / Poor / Critical*, a loss per lap, a lap where pace is expected to drop off, and a confidence. It never claims a wear percentage.
- **Combined plan:** when the tyre estimate is confident, it can bring the stop forward ("Fuel can make lap 7, but tyre degradation favors stopping on lap 6"). Otherwise fuel decides.

---

## Quick start

### Prerequisites
- [Rust](https://rustup.rs) (stable)
- [Node.js](https://nodejs.org) 20+
- Tauri system dependencies for your OS. See the [Tauri prerequisites](https://tauri.app/start/prerequisites/).
- Optional, for voice: [Piper TTS](https://github.com/rhasspy/piper) and a voice model (`.onnx` plus `.onnx.json`).

### Run
```bash
git clone https://github.com/ElevenNorthCo/ApexNorth.git
cd ApexNorth
npm install
npm run tauri dev
```

### Build an installer
```bash
npm run tauri build
```
The output goes to `src-tauri/target/release/bundle/`: `.app`/`.dmg` on macOS, `.msi`/`.exe` on Windows, and `.deb`/`.AppImage`/`.rpm` on Linux.

---

## Connecting to your PlayStation

1. Put your computer and your PS5/PS4 on the **same network**.
2. In GT7, get **on track**: a race, time trial, lobby or practice. The game doesn't send telemetry from menus.
3. In ApexNorth, open **Connect**, then choose **Find my PlayStation**.

If it can't find the console:
- Enter the IP manually. On the PS5 it's under *Settings → Network → View Connection Status*.
- Turn off **AP / client isolation** on your router, and avoid guest networks.
- **Windows:** allow ApexNorth through the firewall (UDP port **33740** inbound).
- **macOS:** allow *Local Network* access when prompted.
- Use 5 GHz Wi-Fi or Ethernet for the smoothest 60 Hz stream.

---

## Race engineer voice (Piper)

ApexNorth calls the `piper` command-line program. It finds Piper on your `PATH` and in `~/.local/bin`.

```bash
pipx install piper-tts   # or download a release binary
```

1. Download a voice from [Piper voices](https://huggingface.co/rhasspy/piper-voices). You need both the `.onnx` file and its `.onnx.json` file. `en_US-ryan-high` makes a good race engineer.
2. In **Settings → Race engineer voice**, choose the voice with **Browse…**, or drop it into the app's `voices` folder.
3. Press **Test voice**.

Generated audio is cached, so repeated callouts play instantly. If the voice can't play, the dashboard shows a red **Voice off** line explaining why.

---

## How it works

```
PS5/PS4 ──UDP 33740──▶ telemetry/udp.rs   heartbeat + discovery + receive loop
                         │
                         ├─▶ crypto.rs     Salsa20 decrypt, packet-variant detection
                         ├─▶ packet.rs     parse into a typed Packet
                         ▼
                       session/engine.rs   lap detection, timing, delta, live sectors,
                         │                 lap validity, pit detection, G-forces
                         ├─ fuel.rs / tyres.rs / strategy.rs / incidents.rs
                         ├─▶ "telemetry" event (30 Hz) ─────▶ React dashboard
                         ├─▶ storage (SQLite + zstd lap blobs) ◀─ Activity / Lap Analysis
                         └─▶ engineer/rules.rs   what to say (structured calls)
                               └─ director.rs    chatter level, toggles, anti-nag, timing
                                    └─ voice.rs  Piper CLI → WAV cache → rodio
```

- All the heavy work happens in Rust: decrypting, lap logic, recording and callouts. The UI only renders.
- **Lap timing:** GT7 sends no running lap time, so the current lap time comes from packet IDs (60 per second, so dropped packets are handled). The completed lap uses the game's official time, and the recorded samples are rescaled to match it.
- **Delta:** the elapsed time is compared with the reference lap's time at the same distance. The reference is your best lap in this session, or your stored best for this track and car.
- **Lap validity:** a lap is only thrown out on strong evidence. A single bad packet, a Wi-Fi dropout or a late packet is ignored. A position jump only counts once several packets agree the car really moved, and it's impossible over the real elapsed time. The reason is saved with the lap (hover *INVALID* in the session table).
- **Sectors:** five equal-distance sectors of the reference lap. Each one fires a live event with the sector time, the delta to the best sector and to the reference, and the running lap delta. A standing-start lap is never used as a sector benchmark.
- **Pit window:** the states are *No stop*, *Closed*, *Open* and *Box now*. The dashboard shows the window and target lap.
- **Storage:** data lives in your OS app-data folder (`apexnorth.db`). Settings live in the app config folder (`settings.json`).

---

## GT7 telemetry protocol

Sources: Nenkai's PDTools, gt7telemetry, and the GTPlanet community.

| | |
|---|---|
| Heartbeat | Send `A`, `B` or `~` to `PS_IP:33739` about once a second |
| Receive | `0.0.0.0:33740`, about 60 packets/s |
| Cipher | Salsa20, key = the first 32 bytes of `Simulator Interface Packet GT7 ver 0.0` |
| Nonce | `iv1 = u32le @ 0x40`; `iv2 = iv1 ^ XOR`; nonce = `iv2 ‖ iv1` |
| XOR / size | `A` = `0xDEADBEAF` / 296 B · `B` = `0xDEADBEEF` / 316 B · `~` = `0x55FABB4F` / 344 B |
| Magic | `0x47375330` after decryption |

The field offsets are documented in [`src-tauri/src/telemetry/packet.rs`](src-tauri/src/telemetry/packet.rs).

---

## Project structure

```
src-tauri/src/
  lib.rs                 app setup, live packet handler, Tauri commands
  settings.rs            settings, Piper / voice discovery
  telemetry/
    crypto.rs            Salsa20 encrypt/decrypt, packet variants
    packet.rs            GT7 packet parser (+ serializer for the simulator)
    udp.rs               link: heartbeats, LAN discovery, connection status
    sim.rs               demo-mode PlayStation simulator
  session/
    engine.rs            lap engine and live state
    fuel.rs              fuel model, fuel-off detection, pit window strategy
    tyres.rs             estimated tyre degradation
    strategy.rs          fuel + tyre pit plan
    incidents.rs         spin detection
    models.rs            Sample, CompletedLap, InvalidReason, sector math
  storage/mod.rs         SQLite: tracks, sessions, laps
  catalog/
    cars.rs              car code → name (data/gt7_cars.csv)
    tracks.rs            all GT7 layouts + recognising a lap's track (data/gt7_tracks.json)
  engineer/
    rules.rs             decision layer: events → candidate calls
    live.rs              tyre temperature and wheel-slip watchers
    director.rs          chatter level, toggles, anti-nag, context timing
    phrases.rs           phrase variation, spoken numbers
    voice.rs             Piper synthesis, WAV cache, playback queue
src/
  views/                 Connect, Dashboard, Activity, Session, LapAnalysis, Settings
  components/            Tile, TrackMap, TelemetryChart (uPlot), TrackOutline, icons
  lib/                   api (typed commands), store (Zustand), format helpers
```

---

## Development

```bash
npm run tauri dev           # app with hot reload
cd src-tauri && cargo test  # Rust unit tests
npx tsc --noEmit            # type-check the frontend
```

The tests cover:
- crypto round-trips for all three packet variants
- packet parsing
- lap engine timing, live sectors, and lap validity (spikes, dropouts, late packets, real resets)
- a full simulated race through the engine and engineer, at every chatter level
- SQLite storage
- car names, and track recognition for every layout with a reference line (forward, reverse, chicane variants, real PS5 captures)
- fuel strategy: fuel off, no stop, one stop, multi-stop, the window moving, box now
- tyre degradation, spin detection and the combined pit plan
- chatter levels, category toggles, cooldowns and priority
- spoken time formatting

**Demo mode** (Connect, then *Try demo mode*) starts a fake PlayStation on `127.0.0.1`. It answers heartbeats with encrypted packets from a 10-lap race simulation with a standing start, a speed profile, fuel burn, tyre wear and temperatures, wheelspin and lock-ups, a pit stop, and a spin on lap 8. This tests the full pipeline end to end.

---

## Roadmap: phases & milestones

### ✅ Phase 1: Connect & live data
- [x] Tauri 2 + React + TypeScript scaffold
- [x] GT7 Salsa20 decryption and packet parsing (A / B / ~), with unit tests
- [x] UDP link with heartbeat, auto-discovery (LAN sweep + broadcast) and variant fallback
- [x] Three-step connection wizard, remembered IP and auto-connect
- [x] Live dashboard modelled on Victory's layout: gear, RPM, speed, shift lights, lap times, delta, tyres, fuel, inputs, clock
- [x] Demo mode simulator
- [x] **Milestone:** live data from a real PS5 at 60 Hz ✔

### ✅ Phase 2: Record, history & playback
- [x] Lap engine: official lap times, 30 Hz samples, 5 sectors, pit and invalid detection
- [x] SQLite storage, track fingerprinting, renamable tracks
- [x] Activity feed grouped by track and date, with track outlines
- [x] Session view with sector table and best laps/sectors highlighted
- [x] Lap analysis: track map, playback, compare against any lap, synced charts
- [x] **Milestone:** drive a session, then replay and compare laps ✔

### ✅ Phase 3: Piper race engineer
- [x] Offline Piper TTS with a WAV cache and priority queue
- [x] Callouts for lap times, PBs, final lap, fuel, pit window, tyres, wheelspin, lock-ups, race start and grid slot
- [x] Per-category toggles, volume, speaking speed, test button, and voice errors shown in the UI
- [x] **Milestone:** engineer callouts working live on track ✔

### ✅ Phase 3.5: Dynamic race engineer & strategy
- [x] Lap validity rebuilt: reasons per lap, no more real laps thrown out by a single bad packet or dropout
- [x] Live sector events and sector coaching with thresholds
- [x] Pace and encouragement backed by timing data
- [x] Chatter levels (Minimal / Normal / Chatty / Crew Chief) on top of per-category toggles
- [x] Severity-based wheelspin and lock-up calls
- [x] Spin detection from heading vs. direction of travel
- [x] Tyre temperature states, imbalance, rate of rise and post-incident recovery
- [x] Fuel strategy: rolling burn, stops required, earliest / target / latest pit lap, fuel-off detection
- [x] Estimated tyre degradation, combined with fuel for the pit plan
- [x] Context-aware timing, priorities, anti-nag memory and phrase variation
- [x] Late-lap "box this lap" reminder
- [x] Live race position investigated: GT7's packet has no live position or data on other cars, so position calls stay grid-only
- [ ] **Milestone:** a full live race where the engineer feels involved without being annoying

### 🔜 Phase 4: Polish & distribution
- [ ] Keep Piper loaded in the background so new callouts are near-instant (currently about 2.5 s for an uncached phrase)
- [ ] Bundle Piper and a default voice as a Tauri sidecar
- [x] GT7 car-name lookup (car code → "Porsche 911 GT3 (996) '01"), all 584 cars (verified live on PS5, 2026-10-03)
- [x] Built-in track database: all 121 layouts listed, 113 recognised automatically from the driven line (forward/reverse and chicane variants included), older sessions renamed on launch (verified live on PS5, 2026-10-03)
- [ ] Track database extras: real outlines and track-specific sectors
- [ ] Multiple dashboard layouts, plus speed/G-force/steering widgets
- [ ] Pre-race strategy briefing (fuel needed, planned stops)
- [ ] GitHub Actions builds for macOS, Windows and Linux, plus auto-update
- [x] App icon and branding

### 🔮 Phase 5: AI coach
- [ ] Lap analysis by Claude: compare against your best, find braking points, minimum corner speeds and throttle application
- [ ] A per-corner "where you lost time" report after each session
- [ ] Spoken coaching tips between laps
- [ ] Session summaries and progress tracking over time

### 🔮 Phase 6: Talk to your race engineer
- [ ] Push-to-talk voice input (speech-to-text)
- [ ] Chat with an engineer that knows your live telemetry and history ("How are my tyres?", "What do I need to do to beat my PB?")
- [ ] Engineer personalities and voices
- [ ] Optional Pitwall: multi-driver live view for leagues

---

## Known limitations
- **Tracks:** GT7 doesn't report a track ID, so tracks are recognised from the lap's line.
  - 8 of the 121 layouts have no reference line yet. Short layouts (Suzuka East, Nürburgring Sprint, Red Bull Ring Short Track, Autopolis Short Course) are matched by venue and length.
  - Barcelona Rallycross can't be recognised yet.
  - Nordschleife Tourist, Nürburgring Endurance II and Fuji (Short) drive the same line as their sister layouts, so they are named as those. Rename the track to fix that; the name sticks.
  - Anything unrecognised is named "Track N (x.xx km)" until you rename it.
- **Cars** that are newer than the bundled list show as `Car #<code>`. Run `scripts/build_gt7_data.py` to refresh it.
- **Position:** GT7 only sends grid position before the race, so POS shows the starting slot. The packet carries no live position and no data on other cars, so ApexNorth won't announce overtakes until a reliable source exists.
- **Tyre wear:** GT7 sends no tyre wear data. Degradation is estimated from the lap-time trend and stated with honest language, never as a percentage.
- **Pit entry:** track layouts differ and GT7 doesn't say where the pit lane is, so the "box this lap" reminder comes at about 75–90% of the lap.
- **Voice latency:** the Python Piper CLI loads the model for each new phrase. This will be fixed in Phase 4.

---

## Credits
- Feature set inspired by [Victory: Race Engineer for GT7](https://victorydash.io).
- Protocol research: [Nenkai/PDTools](https://github.com/Nenkai/PDTools), [Bornhall/gt7telemetry](https://github.com/Bornhall/gt7telemetry), and the [GTPlanet telemetry thread](https://www.gtplanet.net/forum/threads/overview-of-gt7-telemetry-software.418011/).
- Voice: [Piper](https://github.com/rhasspy/piper).
- Car list: [ddm999/gt7info](https://github.com/ddm999/gt7info) (MIT-0).
- Track catalog and reference lines: [jbhoorasingh/gt7-datalogger-track-data](https://github.com/jbhoorasingh/gt7-datalogger-track-data) (CC0), built on captures from [zetetos/gt-telemetry](https://github.com/zetetos/gt-telemetry) (MIT). Test laps from [vthinsel/GT7Tracks](https://github.com/vthinsel/GT7Tracks) (MIT).

Not affiliated with Sony Interactive Entertainment, Polyphony Digital or Victory. Gran Turismo is a trademark of Sony Interactive Entertainment Inc.

© Eleven North
