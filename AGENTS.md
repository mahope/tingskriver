# AGENTS.md

Regler for alle, der ændrer kode i Tingskriver: agenter og mennesker. Filen er kort med vilje. Den peger på kilden i stedet for at gentage den.

Tingskriver er en lokal mødeassistent til Windows (GPUI-app, Rust). Optagelse, transskription og resumé kører på brugerens egen maskine. Overblik: [README.md](README.md). Crate-grænser: [docs/arkitektur.md](docs/arkitektur.md). Byggekommandoer for agenter på Linux: [docs/agenter.md](docs/agenter.md).

## Regler pr. crate

Hver crate har sin egen `AGENTS.md` med Ejer / Ejer ikke / Invarianter. Læs den, før du rører crate'en.

| Crate | Regler |
|---|---|
| `crates/tingskriver-core` | [AGENTS.md](crates/tingskriver-core/AGENTS.md) |
| `crates/tingskriver-app` | [AGENTS.md](crates/tingskriver-app/AGENTS.md) |
| `crates/tingskriver-overlay` | [AGENTS.md](crates/tingskriver-overlay/AGENTS.md) |
| `crates/tingskriver-host` | [AGENTS.md](crates/tingskriver-host/AGENTS.md) |
| `installer/` | [AGENTS.md](installer/AGENTS.md) |

`frontend/` (Tauri) udfases til fordel for GPUI-appen. Ret kun dér for at holde den kørende, og læg ny logik i `tingskriver-core`.

## Genbrug før du bygger (GPUI-appen)

[docs/design.md](docs/design.md) lister fundamentet, alle komponenter i `crates/tingskriver-app`, produktbegreberne og oplevelserne.

1. Find den eksisterende komponent og et skærmbillede af en lignende skærm først.
2. Lav kun en ny komponent, hvis intet dækker behovet. Mangler en komponent en tilstand, så udvid den.
3. Tjek forbrugerne (`git grep -n "<navn>(" crates/`), før du ændrer en delt komponent.
4. Opdatér `docs/design.md` i samme ændring, hvis en komponents ansvar eller en oplevelse ændres. `python3 scripts/check-design-doc.py` håndhæver komponentlisten.
5. Svar på "genbrugt eller ny komponent, og hvorfor" i PR-skabelonen.

## Fra ændret komponent til CI-kontrol

Find rækken for det, du har ændret. Kontrollen i anden kolonne skal være grøn (lokalt, hvor det kan lade sig gøre, ellers i CI) før du melder færdig. Jobnavnene står i [.github/workflows/rust.yml](.github/workflows/rust.yml).

| Du ændrer | Kontrol | Kommando |
|---|---|---|
| `crates/tingskriver-core` (platformuafhængig kode) | Job "Core tests and crate boundaries (Linux)" | `cargo test --locked -p tingskriver-core` |
| `crates/tingskriver-core`, en ny afhængighed | Samme job: core må ikke afhænge af UI-crates | Kommandoen under tabellen (ingen `tauri`, `gpui`, `wry`, `webview2`) |
| `crates/tingskriver-core`, I/O eller HTTP i async kode | Async-I/O-vagten | `bash scripts/check-async-io-guard.sh` |
| `crates/tingskriver-core/src/audio`, optagelse | Lydtests og optagesoak | `cargo build --release --locked -p tingskriver-core --example recording_soak`, derefter `python3 scripts/recording-soak.py --binary target/release/examples/recording_soak --output <ny mappe> --seconds 8 --interval 1` ([docs/recording-soak.md](docs/recording-soak.md)) |
| Enhedsskift eller mute | Beslutningstabellen | `cargo test --locked -p tingskriver-core --test device_decisions` |
| Windows-kode (`#[cfg(windows)]`) i en hvilken som helst crate | Job "Windows check from Linux (cargo xwin)" og "Windows workspace build and tests" | `cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-host -p tingskriver-app` |
| `crates/tingskriver-overlay` | Overlayets tests | `cargo test --locked -p tingskriver-overlay` (Windows-testen kører kun på Windows) |
| `crates/tingskriver-host` | Hosttests | `cargo test --locked -p tingskriver-host` (hook-testen kører kun på Windows) |
| `crates/tingskriver-app`, GPUI-afhængigheder | GPUI-låsen | `python3 scripts/check-gpui-lock.py` |
| `crates/tingskriver-app`, skærme og tekst | Job "Windows workspace build and tests" og "GPUI app artifact" | Byg på Windows; tekst via `tingskriver_core::strings` ([docs/dansk-tekst.md](docs/dansk-tekst.md)) |
| Leveringsformat (`delivery`) | Skemakontrollen | `python3 scripts/tests/test_delivery_schema.py` |
| `Cargo.toml`, `Cargo.lock`, `deny.toml` | Job "Dependency licences (cargo deny)" | `cargo deny --locked check licenses bans` |
| `installer/`, `.github/workflows/app-release.yml` | Job "GPUI app artifact" og branch-build | Se [installer/README.md](installer/README.md); installeren kan kun prøves på Windows |
| `frontend/` (Tauri) | Job "Windows workspace build and tests" | Byg på Windows |
| Ny tekst, docs, `NOTICE.md` | Ingen automatisk kontrol | Læs den igennem; stavning og æøå skal stemme |

Grænsekontrollen er CI-trinnet "Core has no UI dependencies" ([.github/workflows/rust.yml](.github/workflows/rust.yml)). Kør den lokalt sådan, og den må ikke finde noget:

```bash
cargo tree --locked -p tingskriver-core -e normal,build --prefix none --target all \
  | grep -E '^(tauri|tauri-[a-z-]+|gpui|gpui-[a-z-]+|wry|webview2[a-z-]*) v'
```

Formatering: kør `rustfmt --edition 2021 <fil>` på de filer, du rører, aldrig på hele træet (se [docs/agenter.md](docs/agenter.md) for hvorfor). Ny og ændret kode skal være clippy-ren.

## En sprunget kontrol er ikke bestået dækning

Kan en kontrol ikke køre (fx fordi du er på Linux og ændringen kun kan prøves på Windows), så skriv kommandoen og årsagen i PR-beskrivelsen: hvilken kommando, hvorfor den ikke kunne køre, og hvad der dækker ændringen i stedet. "Ikke kørt" uden begrundelse tæller som fejlet. En kontrol, der fejler på `main` af miljøårsager, dokumenteres på samme måde, med bevis for at den fejler på basen.

## Test, review og portering

- Testpolitik: [docs/testing.md](docs/testing.md). Skriv en test for hver rettet fejl, hvert datatab og hvert race i optagelsens eller transskriptionens livscyklus.
- Review: [REVIEW.md](REVIEW.md).
- Portering: kopiér aldrig fremmed kode uden at registrere den under "Porteret fra" i [NOTICE.md](NOTICE.md) (repo, commit, filer, ophavsretsindehaver, licens). Ingen AGPL, GPL eller ukendt licens kommer ind uden skriftlig godkendelse fra ejeren. Licenser for bundlede og downloadede komponenter står i [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## Generelle regler

- Gren fra `main`, én ændring pr. PR, `Fixes #N` i PR-teksten. Se [CONTRIBUTING.md](CONTRIBUTING.md).
- Repoet er offentligt: ingen hemmeligheder, personoplysninger eller private stier i kode, docs, commits eller skærmbilleder. Test kun med syntetiske data.
- Ingen telemetri og ingen netværkskald, som brugeren ikke har bedt om eller slået til.
- Brugertekst er dansk med æøå og står i `tingskriver_core::strings`.
- Versionsnummeret i `Cargo.toml` ejes af udgivelsesprocessen. Bump det ikke i en almindelig PR.

## graphify

Code graph (graphify, AST only, no LLM). If `~/graphify/tingskriver/` exists on this machine, use it before broad grep:
- Overview: `~/graphify/tingskriver/BRIEF.md` (about 1.5 KB).
- Lookups: `~/.local/bin/graphify query "<question>" --graph ~/graphify/tingskriver/graphify-out/graph.json --budget 1500`; also `explain "<symbol>"` and `affected "<symbol>"` with the same `--graph`.
- Do not read `GRAPH_REPORT.md` in full (about 100 KB).
- The graph reflects the default branch, rebuilt every 30 minutes by a timer. It is not your branch: check the code in your worktree before changing it. Do not run graphify inside the repo, and never commit `graphify-out/`.
