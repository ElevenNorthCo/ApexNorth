# Third-party notices

ApexNorth's own code is under the [PolyForm Noncommercial License 1.0.0](LICENSE). It includes or downloads the following, which stay under their own licenses. Those licenses, not ApexNorth's, apply to these parts.

## Shipped in the app

| Component | Used for | License |
|---|---|---|
| [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) 1.12.9 | Runs the Piper voices | Apache-2.0 |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) 1.17.1 | Runs the voice model | MIT |
| [espeak-ng](https://github.com/espeak-ng/espeak-ng) (built into sherpa-onnx) and its `espeak-ng-data` | Turns text into phonemes for the voice | GPL-3.0-or-later. Source: https://github.com/espeak-ng/espeak-ng, and the build used by sherpa-onnx: https://github.com/csukuangfj/espeak-ng |
| [Piper](https://github.com/rhasspy/piper) voices (Ryan bundled; Alan, Northern English, Lessac and Amy downloadable) | The race engineer's voice | Per voice: see each voice's `MODEL_CARD` on [Hugging Face](https://huggingface.co/rhasspy/piper-voices) (copied next to the bundled voice) |
| [Tauri](https://tauri.app), [React](https://react.dev) and the other Rust crates and npm packages in `Cargo.lock` / `package-lock.json` | App framework and libraries | Their own licenses, mostly MIT or Apache-2.0 |
| Fonts: Inter, Saira, Chakra Petch, JetBrains Mono (via Fontsource) | UI text | SIL Open Font License 1.1 |

## Data

| Source | Used for | License |
|---|---|---|
| [ddm999/gt7info](https://github.com/ddm999/gt7info) | Car list (car code → name) | MIT-0 |
| [jbhoorasingh/gt7-datalogger-track-data](https://github.com/jbhoorasingh/gt7-datalogger-track-data) | Track catalog and reference lines | CC0-1.0 |
| [zetetos/gt-telemetry](https://github.com/zetetos/gt-telemetry) | Captures behind the track data | MIT |
| [vthinsel/GT7Tracks](https://github.com/vthinsel/GT7Tracks) | Test laps | MIT |
| [Nenkai/PDTools](https://github.com/Nenkai/PDTools), [Bornhall/gt7telemetry](https://github.com/Bornhall/gt7telemetry) | Telemetry protocol research | See each project |

## Images

The venue photos (`src/assets/tracks/`) and car renders (`src/assets/cars/`) show real circuits and cars, which belong to their owners. They are included to identify the track and car you drove, and are not licensed under ApexNorth's license.

## Trademarks

Gran Turismo is a trademark of Sony Interactive Entertainment Inc. ApexNorth is not affiliated with or endorsed by Sony Interactive Entertainment or Polyphony Digital. Car and track names belong to their owners.

The ApexNorth name and logo belong to Eleven North. The license lets you use and share the software, not the name or logo for your own projects.
