# Bundled GT7 reference data

Compiled into the app by `src-tauri/src/catalog/`. Regenerate with
`scripts/build_gt7_data.py` (see the script header for the source repos).

| file | contents | source | licence |
|---|---|---|---|
| `gt7_cars.csv` | car code → "Maker Model 'YY" | [ddm999/gt7info](https://github.com/ddm999/gt7info) `_data/db/cars.csv` + `maker.csv` | MIT-0 |
| `gt7_tracks.json` | all 121 layouts; reference racing line (x, z every ~20 m) where known | [jbhoorasingh/gt7-datalogger-track-data](https://github.com/jbhoorasingh/gt7-datalogger-track-data) `index.json` + `signatures.json` | CC0 (lines derived in part from [zetetos/gt-telemetry](https://github.com/zetetos/gt-telemetry), MIT) |

`src-tauri/src/catalog/testdata/captured_laps.json` holds four laps thinned
from [vthinsel/GT7Tracks](https://github.com/vthinsel/GT7Tracks) dumps (MIT),
used only by tests.
