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

JVM tests also cover generation-bound insertion: original connection, stale results,
cancel/restart, missing/rejected/throwing connections, blank/error results and
at-most-once commit including reentrant callbacks. The IME captures the original
InputConnection when recording starts and invalidates it on every lifecycle
cancellation. Completion must run on the IME main thread. The enabled STT stub
still inserts no text. No clipboard, surrounding-text reads or Enter are used.

JVM tests cover stub behavior, input-field policy and recording ownership:
permission denial/open failure, duplicate start, stale worker generations,
cancel/restart, stop, read failure/revocation, partial chunks, sample capacity
and elapsed-time cutoff even when no samples arrive. They use synthetic sample
values, never voices. Service lifecycle tests call the actual IME overrides for
same-app field changes, app changes, input restart, finish, hide, destruction and
cancel/new session, then deliver stale synthetic results to the service-owned
insertion controller. They use the Android Gradle plugin's default-returning
framework stubs and reflection to access the controller without a production
test hook. They check cancellation wiring, not Android framework or OEM behavior;
physical microphone release and actual InputConnection behavior still require
the device checks below.

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

### Physical insertion gate (not yet performed)

No device is attached in the worker environment. Before enabling real STT, use
synthetic final results in a local test build on a physical phone, in both a
standard EditText and a browser form. Verify one insertion at the original
cursor, duplicate callbacks, blank/error results, rejected connections, same-app
field changes, app switches, cancel/new session, hidden IME, screen lock and
service stop before completion. Late results must write to neither old nor new
fields. Test text, visible, web and numeric passwords and non-text fields.
Never ship the synthetic-result hook or use personal text in the checks.

Other device checks still required: API 26 and current Android, light/dark, TalkBack,
large text, landscape, rotation, system bars, focus changes and switching away.
No device screenshots or APK build are claimed from a JVM-only run.

## License

Original skeleton code follows the repository's [MIT license](../../LICENSE.md).
Gradle wrapper scripts/JAR retain upstream Apache-2.0 notices. No FUTO, GPL IME
code, STT runtime or model weights are included.
