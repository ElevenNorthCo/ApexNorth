//! apexnorth-voice: the voice helper ApexNorth starts to speak callouts.
//!
//! Copyright (C) 2026 Eleven North
//!
//! This program is free software: you can redistribute it and/or modify it
//! under the terms of the GNU General Public License as published by the Free
//! Software Foundation, either version 3 of the License, or (at your option)
//! any later version. It is distributed WITHOUT ANY WARRANTY; see COPYING.
//!
//! It speaks Piper and Kokoro voices, and loads espeak-ng (GPL-3.0) through sherpa-onnx, which is why it is a
//! separate GPL program. The app talks to it over stdin/stdout, one JSON
//! object per line:
//!
//!   → {"op":"load","model":"/path/voice.onnx","cache":"/cache/dir"}
//!   ← {"ok":true,"sampleRate":22050}
//!   → {"op":"load","engine":"kokoro","model":"/path/kokoro-dir","voice":"am_michael","lang":"en-us","cache":"/cache/dir"}
//!   ← {"ok":true,"sampleRate":24000}
//!     (`voice` is a speaker in the model, or a .pt / .npy / .bin voice file)
//!   → {"op":"speak","text":"Box this lap.","lengthScale":0.9,"out":"/tmp/x.wav"}
//!   ← {"ok":true,"seconds":1.4}
//!   ← {"ok":false,"error":"..."}
//!
//! Usage: apexnorth-voice <runtime dir with lib/ and espeak-ng-data/>

// No console window when the app starts it on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod tts;

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

fn main() {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: apexnorth-voice <runtime dir>");
        std::process::exit(2);
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut synth: Option<tts::Synth> = None;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&dir, &req, &mut synth).unwrap_or_else(|e| json!({ "ok": false, "error": e })),
            Err(e) => json!({ "ok": false, "error": format!("bad request: {e}") }),
        };
        if writeln!(stdout, "{reply}").and_then(|_| stdout.flush()).is_err() {
            break;
        }
    }
}

fn handle(dir: &Path, req: &Value, synth: &mut Option<tts::Synth>) -> Result<Value, String> {
    let s = |k: &str| req[k].as_str().ok_or_else(|| format!("missing {k}"));
    match req["op"].as_str() {
        Some("load") => {
            if !tts::runtime_present(dir) {
                return Err(format!("voice engine files missing in {}", dir.display()));
            }
            let rt = tts::Runtime::get(dir)?;
            let (model, cache) = (Path::new(s("model")?), Path::new(s("cache")?));
            let spec = match req["engine"].as_str().unwrap_or("piper") {
                "piper" => tts::prepare_piper(model, cache)?,
                "kokoro" => tts::prepare_kokoro(model, s("voice")?, req["lang"].as_str().unwrap_or("en-us"), cache)?,
                e => return Err(format!("unknown voice engine {e}")),
            };
            match synth {
                // Another speaker of the model that's loaded: no reload.
                Some(v) if v.key == spec.key() => v.sid = spec.sid(),
                _ => {
                    *synth = None;
                    *synth = Some(tts::Synth::load(rt, &spec)?);
                }
            }
            let rate = synth.as_ref().map_or(0, |v| v.sample_rate);
            Ok(json!({ "ok": true, "sampleRate": rate }))
        }
        Some("speak") => {
            let v = synth.as_mut().ok_or("no voice loaded")?;
            let samples = v.speak(s("text")?, req["lengthScale"].as_f64().unwrap_or(1.0) as f32)?;
            if samples.is_empty() {
                return Err("the voice produced no audio".into());
            }
            std::fs::write(s("out")?, tts::wav_bytes(&samples, v.sample_rate)).map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true, "seconds": samples.len() as f64 / v.sample_rate as f64 }))
        }
        _ => Err("unknown op".into()),
    }
}
