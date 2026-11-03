# Tingskriver Android

Native Kotlin voice IME with permission-gated, tap-to-start/tap-to-stop PCM
capture. Mono 16 kHz signed 16-bit audio stays in a single 480,000-sample RAM
buffer (960 kB), plus a 640-sample scratch buffer and the platform capture buffer.
Capture ends at 30 seconds of samples or elapsed time, whichever comes first.
**Speech recognition is not connected:** stopping discards the audio and inserts
no text. Cancel, hidden IME, field changes, screen off/lock, permission loss,
read errors and service destruction stop and release the microphone. No network,
files, audio/transcript logs, analytics, history or backup are used.

See [research](../../docs/android/RESEARCH.md) and the locked
[v0 specification](../../docs/android/SPEC.md) for the planned local Whisper
engine and later Rust text-processing bridge.

The [Whisper benchmark protocol and status](../../docs/android/BENCHMARK.md)
describe the separate offline scoring tools and physical-device measurements
still required before native integration. No model has passed that gate yet.

## Build and test

Install JDK 17 and Android SDK platform 36 / build-tools 36.0.0. Set
`ANDROID_HOME` to your SDK or put `sdk.dir=...` in an untracked `local.properties`.
The wrapper pins Gradle 9.6.0 with a distribution checksum. AGP 9.5.0 uses
built-in Kotlin, upgraded to 2.4.21 on the build classpath.

```sh
cd apps/android
./gradlew --no-daemon :app:assembleDebug :app:testDebugUnitTest :app:lintDebug
```

APK: `app/build/outputs/apk/debug/app-debug.apk`. Tests:
`app/build/reports/tests/testDebugUnitTest/`. The Android workflow runs those
same gates on pushes and pull requests touching this directory or itself.
It uploads the APK and test/lint reports. No release signing or secrets needed.

JVM tests cover stub behavior, input-field policy and recording ownership:
permission denial/open failure, duplicate start, stale worker generations,
cancel/restart, stop, read failure/revocation, partial chunks, sample capacity
and elapsed-time cutoff even when no samples arrive. They use synthetic sample
values, never voices. Android lifecycle wiring and physical microphone release
still require the device checks below.

## Release

Android has its own version and `android-v` tag prefix, independent of the
desktop apps. The first release is `versionName = "0.1.0-dev.1"` with
`versionCode = 1` in `app/build.gradle.kts`. Increase `versionCode` for every
later release, including pre-releases; never reset or reuse it.

The orchestrator creates and retains the keystore outside this repository and
configures these GitHub Actions secrets:

- `ANDROID_KEYSTORE_BASE64`: base64-encoded release keystore.
- `ANDROID_KEYSTORE_PASSWORD`: keystore password.
- `ANDROID_KEY_ALIAS`: signing key alias.
- `ANDROID_KEY_PASSWORD`: signing key password.

After updating and committing the version, tag that commit and push the tag:

```sh
git tag android-v0.1.0-dev.1
git push origin android-v0.1.0-dev.1
```

`android-release.yml` rejects a tag that differs from `android-v<versionName>`
and fails tag builds if any signing secret is missing. It runs JVM tests and
lint, builds the release APK, verifies its signature with `apksigner`, and
publishes `Tingskriver-Android-<version>.apk` plus its `.apk.sha256` checksum.
Updates require the same signing key, so keep the keystore safe.

Manual runs (`workflow_dispatch`) upload artifacts only, even when run on a
tag. With all secrets they verify and upload a signed APK; otherwise they
upload an unsigned APK, which cannot be installed until signed.

For local signing, Gradle reads `TINGSKRIVER_ANDROID_KEYSTORE` (file path),
`TINGSKRIVER_ANDROID_KEYSTORE_PASSWORD`, `TINGSKRIVER_ANDROID_KEY_ALIAS` and
`TINGSKRIVER_ANDROID_KEY_PASSWORD`. If any are missing, `assembleRelease`
builds unsigned. Debug builds need no secrets. Never commit keystores or passwords.

Android releases are **always pre-releases and never latest**, including versions
without a suffix: the Tauri updater uses `releases/latest/download/latest.json`.
They still count toward Velopack's ten newest releases, before pre-release
filtering. Keep fewer than ten non-Velopack releases between Velopack releases
so its feed remains visible; see [Updates](../../installer/README.md#updates).

## Try on a device

1. Install the debug APK, then open Tingskriver.
2. Read the Danish microphone explanation, tap **Giv adgang til mikrofon** and
   allow access. This never starts capture. Use **Åbn tastaturindstillinger**
   and enable Tingskriver. Android displays its standard trusted-keyboard warning.
3. Focus a normal text field and select Tingskriver in the system keyboard picker.
4. Tap **Mikrofon**, then **Stop optagelse**. Status must show recording and
   then that audio was discarded; no text is inserted. Password/PIN fields
   disable the microphone.
5. Tap **Skift tastatur** to return to another enabled keyboard.

### Physical microphone release gate (not yet performed)

On a current physical Android phone, verify the system microphone indicator
turns on only after an explicit start and turns off promptly after each case:

- Manual stop, cancel and the 30-second limit (also while silent).
- Hide IME with Back, switch keyboard/app, change fields in the same app,
  rotate, turn off/lock the screen, and destroy/force-stop the IME process.
- Deny permission initially, deny again, and revoke it later in Android app
  settings. Return to the IME: no crash or capture; granting permission must
  never auto-start recording. Revoke during capture if the OS permits it.
- Repeat start/stop and cancel/start quickly. Only the current session may run.
- Disable microphone access with Android's privacy toggle; verify the time
  limit still releases capture, even if the platform supplies silent samples.

Record only pass/fail, Android version and device model. Do not save speech,
transcripts, raw logcat or screenshots containing field text. Check app-private
storage/backup configuration without exporting voice data. API 26 has no system
microphone indicator: also check release with platform microphone diagnostics.
A green JVM suite cannot certify OEM lifecycle behavior or the indicator.

Other device checks still required: API 26 and current Android, light/dark, TalkBack,
large text, landscape, rotation, system bars, focus changes and switching away.
No device screenshots or APK build are claimed from a JVM-only run.

## License

Original skeleton code follows the repository's [MIT license](../../LICENSE.md).
Gradle wrapper scripts/JAR retain upstream Apache-2.0 notices. No FUTO, GPL IME
code, STT runtime or model weights are included.
