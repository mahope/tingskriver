# Levering af issue #15

Core-noter er implementeret uden frontendændringer eller tilkobling til
opstarten. De fire acceptkriterier er opfyldt i lokale tests. CI-status skal
stadig verificeres efter push; GitHubs delte API-kvote gav HTTP 403.

## Acceptkriterier og bevis

- **Round trip: opfyldt.** Faste tests for alle otte blokvarianter, tomt dokument,
  tomme blokke, escaping, danske tegn og emoji. `arbitrary_unicode_round_trip`
  kører 512 property-eksempler med flere blokke og vilkårlige Unicode-spans,
  alle stilkombinationer og valgfrie u64-tidsstempler.
- **Migrering: opfyldt.** 11 syntetiske JSON-fixtures med faste markdown-facit.
  In-memory SQLite med de rigtige migrationer beviser byteidentisk original-JSON,
  to kørsler uden dubletter, tomme dokumenter, doven indlæsning og at et defekt
  møde ikke stopper de andre. Tre ukendte bloktyper tælles særskilt.
- **Tidsstempler og opslag: opfyldt.** Gemning/genindlæsning og inklusive
  vinduesgrænser er testet, inklusive utimede blokke, før første blok og
  u64-overløbsgrænser. Gemning over SQLite-i64-grænsen afvises atomisk.
- **Autosave: opfyldt.** Pauset Tokio-klokke og hukommelsesfake beviser gemning
  præcis ved 2 s under ændringer hver 100 ms. Abort ved 3,999 s bevarer sidste
  gemning og mister under 2 s. En separat SQLite-test beviser samme lagring
  og abort uden flush. Ro efter en ændring giver gemning efter 500 ms;
  normal kanallukning giver straks-flush.

## Kontroller

Alle Cargo-kommandoer bruger
`PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver`.
Windows-krydstjekket har desuden `/usr/lib/llvm-19/bin` i PATH.

- `cargo test --locked -p tingskriver-core --test notes`:
  `10 passed; 0 failed` (herunder 512 property-eksempler).
- `cargo test --locked -p tingskriver-core`: 479 enhedstests, 10 notetests
  og 1 dokumentationstest bestod; 3 eksisterende tests ignoreres.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-app`:
  bestod. Første forsøg manglede `llvm-rc`; LLVM-PATH løste det.
- `cargo deny --locked check licenses bans`: `bans ok, licenses ok`.
- Rustfmt på de nye Rust-filer og `git diff --check`: bestod.
- Almindelig Clippy stopper på den eksisterende
  `audio/capture/backend_config.rs:55`, `inherent_to_string_shadow_display`.
  Fejlen blev reproduceret på en separat `git archive origin/main`-kopi.
  Den almindelige Clippy-gate er derfor **ikke grøn**. En særskilt diagnostisk
  kørsel med kun denne arvede fejl som warning bruges til at kontrollere
  notes-modulet og integrationstesten; ingen repo-lintregler er ændret.
- `scripts/ci-status.sh issue/15-noter-tidsstempel`: første opslag gav HTTP 403
  med rate-limit-reset 2026-11-01 20:17:38 UTC. Det er et manglende CI-resultat,
  ikke en konstateret CI-fejl. Se grenens runs:
  https://github.com/mahope/tingskriver/actions?query=branch%3Aissue%2F15-noter-tidsstempel

## Pakker

Manifestet har præcis `proptest = "1"` under dev-dependencies, låst til 1.9.0.
Nyeste kandidat 1.12.0 ville opgradere eksisterende Borsh-afhængigheder;
1.9.0 bevarer alle eksisterende pakkeversioner. Tokios eksisterende version får
kun dev-feature `test-util`. Fem nye lockfilspakker i alt:

- **proptest 1.9.0: installér.** Vedligeholdes af proptest-rs, crate siden 2017,
  version udgivet 2025-10-26. Ingen build-script eller proc-macro i standardfeatures.
  Læser testkonfiguration fra miljøet ved brug, ingen netværkskode ved indlæsning.
- **quick-error 1.2.3: installér som transitiv testafhængighed.** tailhook,
  crate siden 2015, version fra 2020-01-02. Deklarative makroer uden build-script,
  proc-macro, miljølæsning eller netværkskald.
- **rand_xorshift 0.4.0: installér som transitiv testafhængighed.** rust-random,
  crate siden 2018, version fra 2025-01-27. Ren PRNG uden build-script,
  proc-macro eller netværkskald.
- **rusty-fork 0.3.1: installér som transitiv testafhængighed.** altsysrq,
  crate siden 2018, version fra 2025-10-04. Ingen build-script/proc-macro/netværk;
  kan starte test-underprocesser og læser RUSTY_FORK-konfiguration ved brug.
- **unarray 0.1.4: installér som transitiv testafhængighed.** cameron1024,
  crate siden 2022, version fra 2022-11-30. Ingen Cargo-build-hook eller proc-macro;
  `src/build.rs` er et almindeligt modul. Ingen netværkskode ved indlæsning.

Kilder: crates.io-versionernes metadata og de fem offentliggjorte kildearkiver;
licenser og bans blev kontrolleret med Cargo Deny. Ingen eksisterende pakker
opgraderes, og ingen afhængigheder tilføjes til produktionsdelen.

## Integration og konflikter

Uden for eget modul ændres kun `Cargo.lock`, de to dev-dependency-linjer i
core-manifestet og én `pub mod notes;` i core-lib.rs. Frontend,
`database/init.rs`, `database/manager.rs` og `docs/arkitektur.md` er urørte.

Opstart-db-grenen indsætter
`tingskriver_core::notes::migrate_legacy_notes(&pool).await?` lige efter den
fælles `sqlx::migrate!("./migrations").run(&pool).await?`, før poolen udleveres.
På denne base er det i `DatabaseManager::new` i `database/manager.rs`; ved
flytning til `database/init.rs` følger kaldet samme migrationspunkt.

Én linje til `docs/arkitektur.md`:

`- Egne noter: UI-uafhængige blokke, markdown-lite, autosave, tidsopslag og tabsfri bevaring af gamle BlockNote-originaler; se [noter.md](noter.md).`

## Manuel Windows-kontrol

1. Kør `cargo test --locked -p tingskriver-core --test notes` på grenen.
   Forvent 10 beståede tests, inklusive den pauserede klokke og SQLite-migrering.
2. Kør `cargo test --locked -p tingskriver-core` og sammenhold med resultaterne ovenfor.
3. Der er intet nyt UI at afprøve endnu. Når #47 tilkobles, brug et syntetisk
   møde, opret afsnit under optagelsen, genstart efter gemning og kontrollér
   tekst/stil/tidsopslag. Den opskrift er en fremtidig integrationstest,
   ikke et krav om at ændre den nuværende Tauri-editor.

Ingen PR, deploy, tag, release eller versionsændring er foretaget.
Fresh-context review af hele diffen foretages fra Mads' PC som aftalt.

## PR-tekst

Titel: `Add timestamped core notes and migrate legacy BlockNote documents`

Beskrivelse:

Store meeting notes as UI-independent blocks with bold/italic spans and recording
offsets. Add lossless markdown-lite serialization, transactional SQLite storage,
bounded autosave, inclusive time lookup, and idempotent lazy/batch migration of
legacy BlockNote JSON while preserving every original JSON value.

Validate all four acceptance criteria with 11 synthetic legacy fixtures, 512
property cases, SQLite integration tests, and paused-clock crash/debounce tests.
Core tests, Windows cross-check and dependency policy checks pass locally.
GitHub CI status is pending because the shared public API quota was exhausted.
Startup migration integration is documented for the startup-db branch; no frontend
or startup-owned files are changed.

Fixes #15
