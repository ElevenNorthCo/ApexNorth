# Third-party notices

ApexNorth's own code is under the [PolyForm Noncommercial License 1.0.0](LICENSE). It includes or downloads the following, which stay under their own licenses. Those licenses, not ApexNorth's, apply to these parts.

## The voice helper (GPL)

The race engineer's voice runs in **`apexnorth-voice`**, a separate program shipped next to the app. It loads espeak-ng, which is GPL-3.0, so the helper is free software under the **GNU GPL v3 or later** ([`src-tauri/voice-helper/COPYING`](src-tauri/voice-helper/COPYING)). Its full source is in [`src-tauri/voice-helper/`](src-tauri/voice-helper/). ApexNorth starts it as its own process and only sends it text over a pipe; the two are separate programs distributed together.

## Shipped in the app

| Component | Used for | License |
|---|---|---|
| [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) 1.12.9 (loaded by the voice helper) | Runs the Kokoro voices (and any Piper voice a user adds) | Apache-2.0. Source of the exact version: https://github.com/k2-fsa/sherpa-onnx/tree/v1.12.9 |
| [ONNX Runtime](https://github.com/microsoft/onnxruntime) 1.17.1 | Runs the voice model | MIT |
| [espeak-ng](https://github.com/espeak-ng/espeak-ng) (built into the sherpa-onnx library, used only by the voice helper) and its `espeak-ng-data` | Turns text into phonemes for the voice | GPL-3.0-or-later. Source: https://github.com/espeak-ng/espeak-ng, and the build used by sherpa-onnx: https://github.com/csukuangfj/espeak-ng |
| [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M) v1.0 by hexgrad: the model and its voices (`voices.bin`), in the ONNX conversion published by sherpa-onnx ([`kokoro-multi-lang-v1_0`](https://github.com/k2-fsa/sherpa-onnx/releases/tag/tts-models)) | The default race engineer voices (Emma, Michael, Heart, George and more) | Apache-2.0 (copied as `tts/kokoro/LICENSE`). Trained on permissive and public-domain audio, per its model card |
| [jieba](https://github.com/yanyiwu/cppjieba) dictionary (`tts/kokoro/dict/`) | Required by sherpa-onnx to load the Kokoro model (only used for Chinese text) | MIT |
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
