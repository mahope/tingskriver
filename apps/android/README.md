# Tingskriver Android

Native Kotlin voice IME skeleton. It shows a microphone button, status, keyboard
picker and a setup activity. **Speech recognition is not connected:** the button
does not record audio, call a service or insert text. Password and non-text
fields disable the microphone. There are no microphone/network permissions,
transcript storage, analytics or bundled models.

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

The JVM tests cover stub behavior and input-field policy, not Android lifecycle,
microphone permission, keyboard switching or real InputConnection insertion.

## Try on a device

1. Install the debug APK, then open Tingskriver.
2. Use **Åbn tastaturindstillinger** and enable Tingskriver. Android displays its
   standard trusted-keyboard warning.
3. Focus a normal text field and select Tingskriver in the system keyboard picker.
4. Tap **Mikrofon**: the status says recognition is not connected; nothing is
   recorded or inserted. In password/PIN fields the microphone is disabled.
5. Tap **Skift tastatur** to return to another enabled keyboard.

Device checks still required: API 26 and current Android, light/dark, TalkBack,
large text, landscape, rotation, system bars, focus changes and switching away.
No device screenshots or APK build are claimed from a JVM-only run.

## License

Original skeleton code follows the repository's [MIT license](../../LICENSE.md).
Gradle wrapper scripts/JAR retain upstream Apache-2.0 notices. No FUTO, GPL IME
code, STT runtime or model weights are included.
