# tingskriver-core

Kernen: al logik, ingen UI. Overblik og afhængighedsretning: [docs/arkitektur.md](../../docs/arkitektur.md). Rodens regler: [AGENTS.md](../../AGENTS.md).

## Ejer

- Optagelse (`audio/`): fangst, skrivetråd, endelige filer, genopretning efter nedbrud ([CLAUDE.md](../../CLAUDE.md#audio-recording-path-critical-understanding)).
- Transskription (`whisper_engine`, `parakeet_engine`, `stt`, `transcription_queue`), VAD (`vad_trim`), diktering (`dictation/`).
- Database og migrationer (`database/`, `migrations/`), noter, søgning, kalender, levering (`delivery/`).
- Resumé og AI-udbydere (`summary/`, `ollama`, `openai`, `anthropic`, `groq`, `openrouter`).
- Danske tekster og formater (`strings/`, `format_da`).
- `AppHost`-grænsefladen (`host.rs`), som UI'et implementerer.

## Ejer ikke

- Vinduer, skærme, tray, genveje og dialoger: det ejer `tingskriver-app`, `tingskriver-host` og `tingskriver-overlay` (og Tauri-wrapperne i `frontend/src-tauri`).
- Installer og opdatering: `installer/` (core ejer kun hook-logikken i `install/`).

## Invarianter

- Core afhænger af ingen UI-crate (`tauri*`, `gpui*`, `wry`, `webview2`). CI fejler, hvis `cargo tree --locked -p tingskriver-core -e normal,build --prefix none --target all` indeholder en af dem (kommandoen med grep står i [AGENTS.md](../../AGENTS.md)).
- Optagelsen afhænger ikke af transskription. Under en optagelse kører hverken VAD, transskriptionsmodel eller ffmpeg; et nedbrud dér kan ikke koste lyd. Stop betyder, at lyden ligger på disken.
- Fangstens lydcallback allokerer, logger og låser ikke, og den blokerer aldrig: fuld kø giver tabte bidrag, ikke ventetid ([docs/audio-callback.md](../../docs/audio-callback.md)).
- Mute udløser aldrig enhedsskift, og lydindhold afgør aldrig skift ([docs/enheder.md](../../docs/enheder.md)).
- Rå spor slettes først, når den færdige fil er verificeret (status `ok`).
- Databasen åbnes uden panik (`DbInitError`), og `-wal`/`-shm`-filer slettes aldrig.
- Async kode bruger ikke blokerende `reqwest` eller `std::fs` (`clippy.toml`, `scripts/check-async-io-guard.sh`).
- `audio` afviser `unwrap`/`expect` uden for tests.
- Tastaturhooken (`dictation/win.rs`) svarer på hvert tastetryk inden for ca. 1 ms, uanset hvad UI'et laver. Intet tungt arbejde på hooktråden.
- Core sender ingen tastetryk, lyd eller tekst nogen steder hen uden at brugeren har bedt om det.
- Brugertekst kommer fra `strings`, ikke fra `format!` i komponenter ([docs/dansk-tekst.md](../../docs/dansk-tekst.md)).

## Test

`cargo test --locked -p tingskriver-core`. Hvilke fejl der kræver en test: [docs/testing.md](../../docs/testing.md). Windows-kode tjekkes med `cargo xwin check` ([docs/agenter.md](../../docs/agenter.md)).
