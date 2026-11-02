# Issue #30: afleveringsrapport

WER-modul, manifestloader, rapportkørsel, fixture-baseline, regressionsgate og
separat CI-workflow er implementeret på `issue/30-wer-regression` fra `main`
(`52ff15a74f0a00034324077fbd6daaa09b0f647c`). Issue #30 er **ikke lukket**.
Sidste commit bruger `Refs #30`.

## Acceptkriterier

- **Opfyldt lokalt:** WER-modul med kendte facit. 19 tests omfatter 15
  Levenshtein-facit, vægtet total, tomme tekster, 12 navngivne
  normaliseringstilfælde samt manifest- og WAV-validering.
- **Implementeret, CI-bekræftelse afventer:** Linux-workflowet
  `.github/workflows/wer.yml` kører modulets tests, Python-gatens tests og
  fixture-kørslen uden talemodeller og uploader rapport pr. klip og samlet.
  Samme kørsel består lokalt. GitHubs delte API-kvote gav HTTP 403 ved
  kontrol efter første push. Se seneste status i den afsluttende chatrapport.
- **Delvist opfyldt:** bevidst slettede ord får fixture-gaten til at fejle
  med præcis exit 1. Gode hypoteser: 0 % WER. Forværrede: 60 % og 50 %
  pr. klip, samlet **54,545 %** (6 fejl / 11 referenceord). En virkelig
  VAD-/kædeforringelse er **ikke bevist** uden modeller og Windows-kørsel.

## Kontroller

Kørt med `PATH=$HOME/.cargo/bin:$PATH` og
`CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver` før Cargo.
Ved xwin tilføjes `/usr/lib/llvm-19/bin` til PATH.

- `cargo test --locked -p tingskriver-core --lib 'wer::'`:
  `19 passed; 0 failed`.
- `cargo test --locked -p tingskriver-core`:
  `467 passed; 0 failed; 2 ignored`; doc-tests:
  `1 passed; 0 failed; 1 ignored`.
- `python3 scripts/test-wer-gate.py`: `Ran 8 tests ... OK`.
  Inkluderer faktisk CLI-exitkode for manglende baseline (2), præcis 1,0 pp
  (0), 1,1 pp (1) og forbedring (0), samt nye/manglende klip.
- `bash scripts/wer-fixture-check.sh /tmp/tingskriver-wer-proof-final`:
  `Positive control passed: removed words trigger regression exit 1.`
- `cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-app`:
  `Finished dev profile` med LLVM 19. Første forsøg uden LLVM 19 på PATH
  fejlede i den urørte hosts build-script med `NotAttempted("llvm-rc")`.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core --example wer_run`:
  `Finished dev profile`; begge modeladaptere er typekontrolleret.
- Målrettet `rustfmt --edition 2021 --check` på alle nye Rust-filer,
  `bash -n scripts/wer-fixture-check.sh` og `git diff --check` består.
- `cargo clippy --locked -p tingskriver-core --lib --example wer_run -- -D warnings`
  er **ikke grøn**: 150 arvede fejl ved denne strenge indstilling. Ingen
  fund i de nye filer efter rettelse af macro-genereret `double_must_use`.
  Samme clippy-kørsel på `main` i en midlertidig, separat worktree gav også
  150 fejl med identisk fordeling af fejlbeskeder. Worktreen er fjernet.
  Der er ikke ændret eller undertrykt arvede diagnostics.

## Rigtige klip og Windows

Der er **ingen lydklip og ingen modelbaseline** i ændringen. Det faktiske
lydmanifest er tomt og afvises af loaderen. FLEURS-kildekortets CC BY 4.0 er
kontrolleret; enkeltklip-endpointet gav HTTP 500 `TooBigContentError`.
CoRal-kildekortet kunne ikke åbnes på den undersøgte URL. Ingen optagelser,
licenser eller samtykker er opfundet. Tekstfixtures er repository-forfattede
MIT-testdata og holdes adskilt fra lydmålinger.

Køreklar PowerShell-opskrift til installerede Parakeet-/Whisper-modeller,
ONNX-DLL, baselines og sammenligning står i [wer.md](wer.md#windows-med-installerede-modeller-manuel).
Udvælgelseskrav og metadata pr. klip står i
[KILDER.md](../testsaet/dansk-wer/KILDER.md). Der mangler op til ca. 20 korte
rettighedsklare klip med oplæsning/samtale og dialekter, samt samtykke til
nye optagelser og eventuel offentlig deling.

Runneren kalder de eksisterende **model-API'er direkte**, herunder samme
`transcribe_audio_with_confidence` som den normale Whisper-worker. Den går ikke gennem
VAD, bidder eller polering. Der findes ikke en stabil offentlig fil-til-
fuld-kæde-API med VAD-kontakt i de tilladte filer. Opskriften i
[wer.md](wer.md#bevis-for-den-fulde-transskriptionskæde-mangler) beskriver den
manuelle optagelses-/kædekontrol, rapportformatet og kravet om reel exit 1.
At VAD slås fra kan ikke i sig selv garantere en WER-forværring.

Der skal desuden konfigureres branch protection, hvis WER-jobbet skal være
et obligatorisk mergekrav. Fixture-gaten alene beviser ingen modelnøjagtighed.

## Omfang og pakker

- Ingen ændringer i `audio/**`, `providers/**`, `dictation/**`, `rust.yml`,
  Cargo-manifester, lockfil, frontend eller `docs/arkitektur.md`.
- Eneste ændring i en delt eksisterende fil er `pub mod wer;` i core's
  `src/lib.rs`. Runner-eksemplet er en ny fil i core's `examples/`.
- Ingen nye pakker. Konklusion: brug standardbiblioteket og eksisterende
  afhængigheder; ingen installation eller ny `cargo deny`-kontrol nødvendig.
- Frisk review af hele PR-diffen er overladt til Mads' PC som foreskrevet
  i opgaven. Ingen PR, merge, deploy, version eller release er oprettet.

Linjen til `docs/arkitektur.md`:

`- Dansk WER-evaluering og regressionsgate: se [wer.md](wer.md).`

## Forslag til PR

Titel: `Add Danish WER evaluation and deterministic regression gate`

Beskrivelse:

Tilføj fælles normalisering, ord-Levenshtein med S/I/D og vægtet WER,
strikt manifestloader, JSON-rapporter og en gate for forværring over 1,0
procentpoint. Et særskilt Linux-workflow tester tekstfixtures med gode og
bevidst forværrede hypoteser uden talemodeller.

De eksisterende Parakeet-/Whisper-API'er kan kaldes fra runneren med lokalt
installerede modeller. Rigtige lydklip, modelbaselines og fuld VAD-/kædekontrol
mangler; begrænsninger og Windows-opskrift er dokumenteret. Core-tests,
gate-tests og fixture-kontrol består lokalt. Streng clippy har de samme
150 arvede fejl som main.

Refs #30
