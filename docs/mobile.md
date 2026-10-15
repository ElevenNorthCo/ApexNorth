# ApexNorth on Android and iPhone

ApexNorth on a phone is the same app as on desktop. The phone connects to the PlayStation itself, runs the telemetry engine, records laps and runs the race engineer. No computer is needed.

There is one codebase: the React UI, the Rust telemetry engine, the race engineer, strategy and storage are shared by all five platforms. A fix to fuel strategy or a new callout reaches Windows, macOS, Linux, Android and iOS from the same commit.

> **Status (branch `mobile`):** development builds. Android APKs come from CI; iOS builds for the simulator in CI and needs your Apple signing for a real iPhone. Neither has been tested against a real PS5 yet. See [What's been tested](#whats-been-tested).

---

## What differs on a phone, and why

Almost nothing. The shared code is everything under `src/` (UI) and `src-tauri/src/` (engine), apart from these platform-specific pieces:

| | Desktop | Phone | Where |
|---|---|---|---|
| Race engineer voice | Built-in Kokoro (or Piper) via the `apexnorth-voice` helper, played with rodio | The phone's own offline text-to-speech (Android TextToSpeech, iOS AVSpeechSynthesizer) | `engineer/voice_wav.rs` / `engineer/voice_native.rs` + `plugins/device` |
| Keeping the screen on while live | `keepawake` crate | `FLAG_KEEP_SCREEN_ON` / `isIdleTimerDisabled` | `platform.rs` + `plugins/device` |
| Updates | Signed in-app updater | App store, or install a newer APK | `updater.rs` (desktop only) |
| Navigation | Top toolbar | Bottom tab bar (any screen under 900 × 520) | `components/MobileNav.tsx` |
| Dashboard | 1000 × 540 canvas | Same canvas in landscape; a 540 × 1140 portrait canvas with the same tiles | `views/Dashboard.tsx` (`LANDSCAPE` / `PORTRAIT`) |

**What the engineer says is identical.** Only the last step, turning text into sound, differs. The queue, priorities, interruptions, chatter levels, cooldowns and every rule live in shared Rust (`engineer/voice.rs` talks to a `Backend` trait). The phone backend has its own unit tests with a fake speech engine (`voice_native.rs`).

### Why not Kokoro on phones (yet)

Kokoro needs espeak-ng to turn text into phonemes. espeak-ng is GPL-3.0, so on desktop it only ever runs inside the separate `apexnorth-voice` program, which the app starts as a subprocess. That keeps ApexNorth itself under PolyForm.

- **iOS** doesn't allow an app to start other programs. Linking espeak-ng into the app would put GPL code in it, which is incompatible with ApexNorth's licence and with App Store distribution.
- **Android** can run a bundled executable (packaged as `lib*.so` in `jniLibs`), so the desktop helper approach could work there. It would add about 325 MB of model to the APK.

Paths to Kokoro on phones, in order of preference:

1. **A non-GPL phonemizer.** Kokoro's own G2P ([misaki](https://github.com/hexgrad/misaki), MIT) uses a lexicon plus a fallback. Port it, or use sherpa-onnx's Kokoro with a lexicon and no espeak-ng. Then the Kokoro model (Apache-2.0) and onnxruntime (MIT) can link into the app on both platforms. Check sherpa-onnx's build options to leave espeak-ng out entirely.
2. **Android only:** ship `apexnorth-voice` as an executable in the APK (the same GPL boundary as desktop), plus the model as an on-demand download, so the APK stays small.

Either way, the `Backend` trait is the seam: a `KokoroBackend` for phones slots in next to `NativeBackend` without touching the engineer.

The system voices are offline, free, need no account or API key, and cost nothing to run. On Android, the voice list in Settings shows only voices that are installed and need no network. Better voices come from the phone's settings: Android → Text-to-speech output → install voice data; iPhone → Accessibility → Spoken Content → Voices, where "Enhanced"/"Premium" voices sound much better.

---

## Connecting to the PlayStation from a phone

The phone uses the same Rust UDP link as desktop (`telemetry/udp.rs`): heartbeats to the console's port 33739, telemetry back on the phone's port 33740.

- **Manual IP** is the reliable path: Connect → Enter IP manually. The console's IP is under PS5 Settings → Network → Connection Status. ApexNorth remembers it and reconnects on launch.
- **Find my PlayStation** heartbeats every address on the phone's /24 by unicast (and tries a broadcast). Broadcast fails silently on iOS without Apple's multicast entitlement, but the unicast sweep needs no entitlement, so discovery should still work there. (Not yet verified on hardware.)
- **iOS** asks for **Local Network** access the first time the app reaches the LAN. The prompt text is in `src-tauri/Info.ios.plist` (`NSLocalNetworkUsageDescription`). If it was declined, turn it on under Settings → Apps → ApexNorth → Local Network. The Connect screen says so when the search is slow.
- **Android** needs only the `INTERNET` permission. `targetSdk` is 36 on purpose: apps targeting Android 17 (SDK 37) are subject to local network protection, and would need a runtime permission before they can reach the console. Before raising `targetSdk`, add that permission request (ideally to `plugins/device`).
- **Signal loss / Wi-Fi changes:** heartbeats continue while the link is up, so GT7 resumes streaming when the network comes back. If the OS breaks the socket (iOS can reclaim it while the app is in the background), the link closes and reopens it after 2 s of errors (`REOPEN_AFTER`).
- **One device at a time.** GT7 streams to whoever sent the most recent heartbeat. Two copies of ApexNorth (phone and laptop) heartbeating the same console steal the stream from each other every second. Use one device per session. Running both at once would need one device to relay to the other, which isn't built.

### App lifecycle

- While the link is live, the screen is kept on.
- **In the background**, iOS suspends the app within seconds: telemetry, recording and the voice stop until you come back. The link then picks up again (a lap in progress is marked invalid, as with any gap). Android keeps the process running for a while, but nothing is promised. ApexNorth doesn't claim background recording on phones. Keep it on screen while you race.
- Rotation: the dashboard switches between the portrait and landscape layouts. Both orientations are allowed. For a phone mounted by the wheel, landscape gives the full sim-dash.

---

## Building and testing locally

You need everything from the main [Development](../README.md#development) section, plus the Tauri mobile prerequisites: <https://v2.tauri.app/start/prerequisites/#configure-for-mobile-targets>.

### Android

Requirements: Android Studio (or the command-line SDK), SDK Platform 36, the NDK, JDK 17+.

```bash
export ANDROID_HOME=$HOME/Library/Android/sdk           # Linux: $HOME/Android/Sdk
export NDK_HOME=$ANDROID_HOME/ndk/<version>
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
npm install

npm run tauri android dev                                 # emulator or USB phone, hot reload
npm run tauri android build -- --apk --target aarch64     # release APK (unsigned)
npm run tauri android build -- --debug --apk              # debug APK, installable as-is
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

`tauri android dev` on a physical phone needs the phone and computer on the same network: Vite serves the UI to the phone (`TAURI_DEV_HOST` is handled by `vite.config.ts`).

The Android Studio project is committed in `src-tauri/gen/android` (generated by `tauri android init`). Things changed by hand there:
- `app/build.gradle.kts`: `targetSdk = 36`, `compileSdk = 36` (see above).
- `MainActivity.kt`: full screen, and the system insets passed to the page as `--an-inset-*`.
- `app/proguard-rules.pro`: keeps the insets bridge.
- `res/mipmap-*`: the app icon (from `src-tauri/icons/android`, made with `tauri icon src-tauri/icons/mobile/icon.png`).

### iOS (needs a Mac)

Requirements: Xcode 16+ and `brew install xcodegen libimobiledevice` (the Tauri CLI installs them if they're missing).

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
npm install
npm run tauri ios init            # generates src-tauri/gen/apple (not committed; see below)
npm run tauri ios dev             # simulator (pick one), hot reload
npm run tauri ios build -- --target aarch64-sim     # simulator .app
npm run tauri ios dev -- --open   # opens Xcode, to run on your iPhone
```

`src-tauri/gen/apple` is generated from `tauri.conf.json`, `tauri.ios.conf.json` and `Info.ios.plist`, so it isn't committed: CI regenerates it each build. If you customise the Xcode project by hand, commit `gen/apple` and drop the `ios init` step from `.github/workflows/mobile.yml`.

### Getting it onto your iPhone

Apple requires every app on a real device to be signed.

1. **Free Apple ID (personal team):** in Xcode → Settings → Accounts, add your Apple ID. Then:
   ```bash
   export APPLE_DEVELOPMENT_TEAM=<your team id>   # Xcode → Settings → Accounts → team, or developer.apple.com → Membership
   npm run tauri ios dev -- --open                # or: npm run tauri ios build -- --export-method debugging
   ```
   In Xcode, select your iPhone and press Run. Free provisioning lasts 7 days, then you run it again from Xcode. On the iPhone, enable **Developer Mode** (Settings → Privacy & Security) and trust your certificate (Settings → General → VPN & Device Management).
2. **Apple Developer Program ($99/year):** needed for TestFlight and the App Store, and for builds that last longer than 7 days. Create the app ID `com.elevennorth.apexnorth`, then `npm run tauri ios build -- --export-method app-store-connect` and upload with Xcode's Organizer or `xcrun altool`. For CI signing, add the certificate and provisioning profile as secrets (see [Tauri's iOS signing guide](https://v2.tauri.app/distribute/sign/ios/)); none are committed here.
3. The first connection asks for **Local Network** access: allow it, or ApexNorth can't see the PlayStation.

### Testing without a PlayStation

**Try demo mode** works on phones too: a simulated PS5 on 127.0.0.1 inside the app, racing 10 laps with fuel, tyre wear, a pit stop and a spin.

- `APEXNORTH_AUTODEMO=1` in the app's environment starts demo mode at launch and prints `apexnorth-smoke:` lines (connection, laps, engineer calls) to stdout. CI uses it in the iOS simulator: `SIMCTL_CHILD_APEXNORTH_AUTODEMO=1 xcrun simctl launch --console-pty booted com.elevennorth.apexnorth`.
- `scripts/mobile/android-smoke.sh` and `ios-smoke.sh` are the CI smoke tests; both run locally against an emulator or simulator.
- UI at phone sizes without any device: `scripts/mobile/ui-shots.mjs` screenshots every view at phone portrait, phone landscape and desktop sizes, with a fake backend serving a real demo race (instructions at the top of the script).

---

## CI and distribution

`.github/workflows/mobile.yml` runs on pushes to `main`, `beta` and `mobile` and on pull requests:

| Job | Does | Artifact |
|---|---|---|
| Android build | `tauri android build --apk --aab` for arm64 phones + x86_64 emulators, signs the APK | `ApexNorth-Android` (`.apk`, `.aab`) |
| Android emulator demo race | installs the APK on an API 34 emulator, taps **Try demo mode**, checks it goes live, records a lap that shows in Activity, survives rotation, no crash | screenshots, UI dumps, logcat |
| iOS simulator build + demo race | `tauri ios init` + `cargo check` for the device target + simulator build; checks the Local Network text is in the app; runs a demo race with `APEXNORTH_AUTODEMO=1` until a lap is recorded | simulator `.app` (zip), screenshots, console |

**Android signing:** set the secrets `ANDROID_KEYSTORE_BASE64` (`base64 -w0 release.jks`), `ANDROID_KEYSTORE_PASSWORD` and `ANDROID_KEY_ALIAS` to sign test APKs with your key (keep that key safe: Play Store updates must use it). Without them each run signs with a throwaway key, which works for sideloading, but you have to uninstall the previous test build first.

**Installing a test APK:** download `ApexNorth-Android` from the workflow run's artifacts, unzip, copy `ApexNorth-Android.apk` to the phone and open it (allow "install unknown apps" for your browser or file manager).

**Nothing is published.** The desktop release flow (`release.yml`, stable + beta, in-app updates) is unchanged. Desktop self-updates and phone app-store updates are different channels. When the phone apps are ready for testers:
- **Android:** attach the APK to the beta pre-release, or use Play Console internal testing with the AAB.
- **iOS:** TestFlight, which needs the paid developer account.
- The version comes from `tauri.conf.json` for all five platforms. Android `versionCode` is derived from it by the Tauri CLI.

### Licences for phone distribution

- **System voices:** part of the OS; nothing is bundled.
- **No GPL code is in the phone apps:** the voice helper (and so espeak-ng) is desktop-only.
- **Bundled assets** (car renders, track photos, fonts) are the same as desktop; check `THIRD_PARTY_NOTICES.md` before an app-store release, as stores show the licence terms to users.
- **PolyForm Noncommercial:** free distribution through the stores is fine. A paid app or in-app purchases would be commercial use (Eleven North's right).
- **App Store export compliance:** the app decrypts GT7's Salsa20 telemetry. That is standard encryption used for interoperability, not for securing user data. Answer Apple's export questions accordingly (`ITSAppUsesNonExemptEncryption`), after checking it yourself.

---

## What's been tested

<!-- Updated as hardware testing happens. -->

| | Desktop | Android | iOS |
|---|---|---|---|
| Builds | CI (Linux), as before | CI | CI (simulator); device target compiles |
| Demo race, recording, engineer | ✓ | CI emulator | CI simulator |
| UI at phone sizes | — | headless Chrome screenshots + emulator | headless Chrome screenshots + simulator |
| Live PS5 telemetry | ✓ (user's PS5) | **not yet** | **not yet** |
| System voice audible | — | **not yet** (emulator has no audio check) | **not yet** |
| Physical device | ✓ | **not yet** | **not yet** (needs signing) |

**Still to test on hardware:** a live PS5 session from a real phone, both by manual IP and with **Find my PlayStation**; the iOS Local Network prompt; voice output and ducking of music; screen kept on; and returning from the background mid-session.
