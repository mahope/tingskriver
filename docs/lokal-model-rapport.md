# Leverance til issue #16

Map-reduce, evalueringsharness, fiktive testdata og målemetode er implementeret.
Referencemaskinens hastighed og danske kvalitet er ikke målt. Issue #16 lukkes
ikke af denne leverance; sidste commit bruger `Refs #16`.

## Acceptkriterier

| Kriterium | Status | Bevis / resterende arbejde |
|---|---|---|
| Tider for 30 og 90 minutter | Ikke opfyldt | Afventer Mads' CPU- og Vulkan-kørsler; alle resultatceller siger “ikke målt”. |
| Mindst tre manuelt vurderede referater | Ikke opfyldt | Rubrik og tre tomme vurderingsrækker er klar; afventer referencemaskinen. |
| 90 minutter i baggrunden under 15 minutter | Ikke opfyldt | Målet er beskrevet; kræver både tidsmåling og prøve af appens baggrundssti. |
| Map-reduce udelader ingen transskriptdele | Opfyldt i det dokumenterede tokenestimat | Fake registrerer alle map-input; hvert segment findes helt i mindst én bid, alle map/reduce-input holder budgettet, og resultat/kald er deterministiske. Kanttilfælde og flere reduktionsniveauer er testet. Modellens faktiske tokenizer og faktabevarelse vurderes separat. |
| Endelig beslutning om større model | Ikke opfyldt | Beslutningsramme og modelkatalog er dokumenteret; beslutningen afventer måling. |

## Kontrol på Linux

Alle Cargo-kommandoer blev kørt med:

```bash
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver
```

- `cargo test -p tingskriver-core`: **485 passed; 0 failed; 2 ignored**.
  Dokumentationstests: **1 passed; 0 failed; 1 ignored**.
- `cargo test -p tingskriver-core summary::map_reduce --lib`:
  **6 passed; 0 failed**.
- `cargo test -p tingskriver-core --example lokal_model_eval`:
  **1 passed; 0 failed**. Tester komplette/manglende helperstatistikker,
  nul afrundet generationstid og kravet om positiv Vulkan-offload.
- Fake-CLI blev kørt med generatorens 2-, 30- og 90-minutters testfiler.
  JSON og Markdown blev skrevet, og særskilte referatfiler blev gemt. Alle tre
  gav de forventede to plantede beslutninger og to opgaver. Faktiske tokens/s
  forblev `null`, fordi fake ikke er en modelmåling. Ingen kilde- eller
  referattekst blev udskrevet af CLI'en.
- `rustfmt --edition 2021 --check` på det nye modul (inklusive underfiler) og
  eksemplet samt `git diff --check`: bestået. Processorens øvrige arvede
  formatering blev bevaret.
- Almindelig `cargo clippy -p tingskriver-core --example lokal_model_eval`
  stopper på den arvede `clippy::inherent_to_string_shadow_display` i
  `audio/capture/backend_config.rs:55`. Den fil er identisk med grenens base.
  En særskilt kørsel med `-- --force-warn clippy::inherent_to_string_shadow_display`
  gennemførte kontrollen af eksemplet; ingen advarsler vedrørte de nye filer.
  Det er ikke en påstand om en grøn samlet Clippy-port.

Windows-kompilering og sidste commits CI-status fremgår af afleveringen i chatten.
`cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-core -p tingskriver-overlay -p tingskriver-app`
er den krævede workspace-kontrol; eksemplet kontrolleres desuden særskilt med
`cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-core --example lokal_model_eval`.
Det anonyme CI-script kan ramme den fælles API-kvote; GitHub-forbindelsen kan læse
status uden at hente issue-teksten igen. Ingen PR er oprettet.

## Manuel Windows-opskrift

Følg PowerShell-kommandoerne i [lokal-model.md](lokal-model.md#køreklar-opskrift).
Brug eksisterende model i `<data-dir>\models\summary\`, byg separate CPU- og
Vulkan-helperbinærer, generér testdata, og kør hver backend tre gange med nye
rapportnavne. Noter hardware og faktisk backend. Bedøm mindst kort, 30 og 90
minutter med rubrikken, prøv appens baggrundsgenerering og udfyld resultattabellen.
Tag først derefter beslutningen om plan B. Ingen model er hentet her.

## Filejerskab, pakker og arkitektur

Ingen ændringer uden for opgavens filejerskab. Delte ændringer er én `pub mod`-linje
i `summary/mod.rs` samt den lille lokale gren og to `pub(crate)`-synligheder for
promptgenbrug i `processor.rs`. `sidecar.rs`, `llama-helper/`, `providers/`,
`dictation/`, Cargo-manifester og lockfil er urørte.

Nye pakker: **ingen**. Eksisterende `async-trait`, `serde`, `tokio`, `sysinfo`
og `tempfile` genbruges. Ingen installation, opgradering eller modeldownload.

Foreslået enkelt linje til `docs/arkitektur.md` (filen er ikke ændret):

> Built-in meeting reports use context-budgeted hierarchical map-reduce in `summary/map_reduce.rs`; the local-model evaluation harness and measurement procedure are documented in [lokal-model.md](lokal-model.md).

## PR-tekst til Mads

Titel: `Add bounded local summaries and a Danish evaluation harness`

Beskrivelse:

```text
Long built-in-model transcripts now use segment-aware map-reduce with prompt,
answer and framing reservations, optional overlap and hierarchical reduction.
Fake-model tests verify segment coverage, context bounds and deterministic calls.

Add synthetic Danish fixtures and a local/fake evaluation example that records
phase timing, sampled process/helper RSS, helper-reported throughput when
available, estimated token counts and automatic planted-fact quality signals.
The harness uses the existing report prompts and local helper client.

Document CPU/Vulkan runs, a manual quality rubric and the larger-model decision
framework. Reference-machine measurements, three manual ratings and the final
model decision remain explicitly unmeasured.

Validation: 485 core tests passed (2 ignored), 1 doctest passed (1 ignored),
6 focused map-reduce/harness tests passed, 1 example test passed, and the fake CLI
completed all three fixture sizes. See the handover for Windows and final CI.

Refs #16
```
