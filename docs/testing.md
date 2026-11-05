# Testpolitik

Hvad der skal testes, hvad der ikke skal, og hvilke kommandoer der virker. Reglerne pr. crate står i hver crates `AGENTS.md`, og kortet fra ændring til CI-kontrol står i [AGENTS.md](../AGENTS.md).

## Skriv en test, når du

- retter en fejl: testen skal fejle på den gamle kode og bestå på den nye. Skriv den først.
- retter et datatab eller risikoen for et: tabt lyd, tabt transskript, tabt note, ødelagt database, overskrevet fil.
- retter eller ændrer et race i optagelsens, transskriptionens eller dikteringens livscyklus: start, stop, pause, mute, enhedsskift, nedbrud midt i en optagelse, genoptagelse efter genstart.
- ændrer et format, en migration eller en fil, som en ældre version skal kunne læse.
- ændrer en beslutningsregel (fx hvornår enheden skiftes, hvornår et spor må slettes).

## Test ikke

- Udseende: farver, afstande, skrifttyper, tekstens placering. Det ses på et skærmbillede.
- At en mock blev kaldt. En test, der kun viser, at koden kalder det, du lige har skrevet, fanger ingen fejl.
- Hvor en fil ligger, eller hvad en konstant hedder.
- Eksterne tjenester og modeller. Brug små, syntetiske fixtures i `crates/tingskriver-core/tests/fixtures`.
- Det, kompilatoren allerede garanterer.

## Navngiv regressionen

Hver ny test navngiver den fejl, den dækker, i testnavnet eller i en kommentar på én linje: hvad gik galt, og hvad skal være sandt nu (fx `mute_changes_nothing_after_two_seconds_of_zero_frames`). Skriv i PR-teksten, hvilke nye tests der dækker hvilke fejl. En test uden en fejl at pege på er sjældent værd at beholde.

## Hold testene ærlige

- Mutér testen: sæt fejlen tilbage i koden (eller brug en bevidst forkert variant, som `device_decisions` gør), og se testen fejle. En vagt, der aldrig har fejlet, er ikke bevist.
- Brug fixtures, hvor korrekt og forkert kode giver forskellige tal.
- Syntetiske data alene: ingen rigtige møder, navne, mails eller optagelser. Repoet er offentligt.
- Ingen tests på vægur-tid, hvor virtuel tid kan bruges ([enheder.md](enheder.md)).
- Fjern eller svæk aldrig en test for at blive grøn.

## Kommandoer

Fra repoets rod, på Linux eller Windows:

```bash
cargo test --locked -p tingskriver-core                 # platformuafhængig kerne
cargo test --locked -p tingskriver-core <filter>        # målrettet, fx: dictation
cargo test --locked -p tingskriver-core --test device_decisions
cargo test --locked -p tingskriver-core --example recording_soak   # kun verifier-testen, ikke soaken
cargo build --release --locked -p tingskriver-core --example recording_soak
python3 scripts/recording-soak.py --binary target/release/examples/recording_soak --output "$(mktemp -d)" --seconds 8 --interval 1   # hurtig soak
cargo test --locked -p tingskriver-overlay
cargo test --locked -p tingskriver-host
bash scripts/check-async-io-guard.sh
python3 scripts/check-gpui-lock.py
cargo deny --locked check licenses bans
```

Windows-kode (`#[cfg(windows)]`) kan typetjekkes fra Linux:

```bash
cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-host -p tingskriver-app
```

Tests, der kræver en Windows-session (hook, overlay, indsætning i et rigtigt vindue), kører kun der og står som `#[ignore]` eller `cfg(windows)`. Se [BUILDING.md](BUILDING.md#workspace-tests-and-ci-for-agents) og [agenter.md](agenter.md).

`cargo clippy` og `cargo fmt --check` på hele træet er endnu ikke en CI-port, fordi den arvede kode ikke er ren ([agenter.md](agenter.md)). Hold din egen kode ren og formatér kun de filer, du rører: `rustfmt --edition 2021 <fil>`.

## Hvad CI gør

[.github/workflows/rust.yml](../.github/workflows/rust.yml) kører ved hvert push: Windows-workspacet (bygning og tests), kernens tests og crate-grænserne på Linux, `cargo xwin check`, `cargo deny` og en Windows-build af appen. Læs status for din gren med `scripts/ci-status.sh <gren>`. Særlige workflows (`recording-soak.yml`, `wer.yml`, `search-benchmark.yml`, `ydeevne-port.yml`) måler optagelse, ordfejl, søgning og ydeevne.

## Det, der ikke kan automatiseres

Det, der kræver en rigtig Windows-skærm, en mikrofon eller en anden enhed, skrives som en manuel testplan i PR-beskrivelsen med præcise trin, så ejeren kan afkrydse den bagefter. Skriv også, hvad du ikke har kunnet køre, og hvorfor ([AGENTS.md](../AGENTS.md#en-sprunget-kontrol-er-ikke-bestået-dækning)).
