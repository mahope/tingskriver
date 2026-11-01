# Issue #37: afleveringsrapport

Core-diagnose og `--diagnose [--json]` er implementeret. CPU-beslutningen er
koblet til Whispers faktiske kontekstparametre. Issue #37 lukkes ikke automatisk:
Windows-distributionens native AVX2-krav kan ikke fjernes med et runtime-flag.

## Acceptkriterier

- **CPU ved manglende Vulkan/AVX2: delvist opfyldt.** Fake-probe tester alle
  16 kombinationer af OK/mangler/ukendt/ikke relevant. Simuleringsværdier testes
  uden at ændre globalt miljø. Begge simuleringsflag er testet helt frem til
  Whispers `use_gpu=false` og `flash_attn=false`. Den byggede Linux-CLI er kørt
  otte gange: tekst/JSON for alle fire kombinationer af flagene, med kontrol af
  exitkode og CPU-valg. Native Windows-opstart og transskription uden AVX2 kan
  ikke bevises: den eksisterende distribution kompilerer Whisper med AVX2.
  En kompileret Vulkan-loaderafhængighed kan heller ikke løses fra Rust-main.
  Der kræves en portabel CPU-distribution eller dynamisk backend og en test
  af distributionens DLL-importer. Parakeet bruger allerede CPUExecutionProvider;
  ONNX/VAD's native CPU-krav er ikke ændret eller verificeret på gammel hardware.
- **Dansk forklaring og rettelseshandling: opfyldt.** Hver kontrol testes med
  OK/mangler/ukendt/ikke relevant. Hvert fund kontrolleres for ikke-tom titel og
  forklaring; fejlfund skal have en handling forskellig fra `None`. Handlinger
  serialiseres som data; diagnosen udfører ingen rettelseshandlinger.
- **Eksport uden lyd/transskription: opfyldt.** Begge eksportformater testes med
  genkendelig lyd, transskription og brugernavnssti i en nærliggende struct.
  Probe-grænsefladen kan kun levere afgrænsede tilstande og numerisk OS-version.
  Rapporten tager ikke imod indhold, stier, enhedsnavne eller vilkårlige fejltekster.

## Verifikation

Alle Cargo-kommandoer blev kørt med `~/.cargo/bin` i PATH og
`CARGO_TARGET_DIR=~/dev/cargo-target/tingskriver`.

- `cargo test -p tingskriver-core diagnose --lib`: **4 passed; 0 failed**.
- `cargo test -p tingskriver-core`: første fulde kørsel **452 passed; 0 failed;
  2 ignored**. Doc-tests: **1 passed; 0 failed; 1 ignored**.
- En senere fuld kørsel: **451 passed; 1 failed; 2 ignored**. Den uændrede test
  `dictation::logic::cleanup::tests::long_text_is_fast` tog 64,13 ms mod en
  grænse på 50 ms. Ingen filer i dictation er ændret. Fejlen skjules ikke.
  Genkørsel uden kodeændringer: **452 passed; 0 failed; 2 ignored**;
  doc-tests **1 passed; 0 failed; 1 ignored**.
- `cargo test -p tingskriver-app --bin tingskriver-app`: **5 passed; 0 failed**.
- `cargo build -p tingskriver-app --bin tingskriver-app`: bestået.
- Bygget CLI, otte subprocess-kald uden display: tekst/JSON, alle kombinationer
  af simuleringsflag, CPU-valg og exitkode 0/1 bestået.
- `PATH=/usr/lib/llvm-19/bin:$PATH cargo xwin check --target
  x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay
  -p tingskriver-app`: bestået efter sidste Windows-ændring. Den første kørsel
  manglede `llvm-rc` i PATH; ingen pakkeinstallation var nødvendig.
- Målrettet `rustfmt --check` på diagnosefiler, acceleration, args og main samt
  `git diff --check`: bestået. Store eksisterende lib/whisper-filer er kun ændret
  i de nødvendige linjer; deres arvede formatteringsstøj er bevaret.
- `cargo clippy -p tingskriver-core -p tingskriver-app --all-targets`: fejler på
  de tre kendte fejl: `AudioCaptureBackend::to_string` og to tests med
  `socket.read` uden kontrol af antal bytes. De fejlbehæftede udtryk er også
  til stede i `origin/main`, og de berørte kodestykker er uændrede. Ingen
  clippy-fund peger på diagnosemodulet, acceleration, args eller main.
- CI-link og resultat for det sidste commit gives i afleveringen sammen med
  commit-id. Fresh-context review er overladt til Mads' PC efter opgavens regel.

## Windows og begrænsninger

Den manuelle opskrift står i [diagnose.md](diagnose.md#manuel-windows-test).
Den omfatter release-konsol/omdirigering, mikrofon, lyd, reserveret genvej,
låst udklipsholder, administratorvindue samt en ren VM uden Vulkan og fysisk
hardware uden AVX2. Linux-tests og xwin er ikke en erstatning for disse tests.

CLI kan ikke kende den valgte dikteringsgenvej uden at læse det andet issues
filer. Derfor er genvejen ukendt i CLI; UI leverer bindingen som parameter.
Fejl i enhedsopslag er ukendte, da den eksisterende API ikke skelner manglende
enhed fra navneopslagsfejl. Mikrofonkontrollen undersøger ikke alle gruppepolitikker.

## Filejerskab og afhængigheder

- Delt core-lib: præcis én ny `pub mod diagnose;`-linje.
- Additiv kobling uden for diagnosemodulet: en policy-metode og test i
  `whisper_engine/acceleration.rs`, samt ét metodekald ved oprettelse af
  Whisper-konteksten i `whisper_engine/whisper_engine.rs`.
- App: kun `args.rs` og `main.rs`. Ingen ændringer i optagelse, dictation,
  shortcuts, providers, logging, app-lib, update, Cargo.toml eller Cargo.lock.
- Nye pakker: **ingen**. Standardbiblioteket og eksisterende serde/chrono bruges.
  `cargo deny` er derfor ikke kørt på grund af denne ændring.
- Ingen deploy, release, tag, versionsændring eller PR er udført.

Linje til `docs/arkitektur.md`:

`- Selvdiagnose: core-modulet diagnose leverer strukturerede miljøfund, indholdsfri eksport og CPU-politik; se [diagnose.md](diagnose.md).`

## Foreslået PR

Titel: `Add Windows self-diagnosis and conservative CPU selection`

Beskrivelse:

Add structured environment diagnostics and `--diagnose [--json]` before GPUI
startup. Probe Windows CPU/Vulkan, microphone consent, audio output, shortcut
availability, clipboard access and foreground integrity, and expose Danish
explanations and repair actions without exporting user content.

Apply the diagnostic CPU policy to Whisper context parameters. Cover probe
states, backend decisions, simulation flags, serialization privacy and CLI
parsing with tests; cross-check the Windows code with cargo xwin. Native AVX2
requirements in the existing Windows distribution remain a documented limitation.

Refs #37
