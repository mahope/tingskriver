# PR #130: afslutning af den rene mødedetektor

Main er flettet ind med merge-commit `1d088d9` uden rebase eller force-push.
Konflikten i core-modulets eksportliste bevarer main's moduler og tilføjer
`meeting_detect`. Ingen Windows-indsamling eller appintegration er tilføjet.

Begge P1-fund er bekræftet med regressioner før rettelsen:

- Kalenderforlængelse gav fejlagtigt stop ved den gamle slutfrist. Sluttiden
  opdateres nu ved hvert snapshot fra den bundne aftale, inklusive forkortelse.
  Sen kalenderinformation kan bindes, selv efter aftalen er afsluttet, hvis den
  overlapper den observerede mødeperiode. Tomme opslag sletter ikke fristen.
- En stabil Teams-kandidat erstattede en igangværende Zoom-session. Den aktive
  session bevares nu; andre processer forbliver separate kandidater. Genåbning
  af samme mikrofonproces tilknyttes mødet uden at nulstille observationstid,
  kalender eller stoptimer.

Seks nye JSON-fixtures dækker forlængelse, forkortelse, sent kalenderopslag,
konkurrerende mikrofonbrug med kalenderfrist, kort mikrofonfravær og en stabil
anden kilde under den oprindelige kildes fraværsstop. Udvalgte trin kontrollerer
også hele den aktive mødetilstand. Den eksisterende fixture
`new_meeting_before_stop` er rettet: en ny mikrofonkørsel må ikke fjerne den
oprindelige kalenderfrist.

## Verifikation

Alle kommandoer brugte `CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver`.
Ingen nye afhængigheder er tilføjet, og ingen pakker er installeret ud over
Cargo-hentning fra den eksisterende lockfil.

Det rene modul er kompileret direkte med `rustc --edition 2021 -D warnings
--test`, mod de serde/serde_json-artefakter som Cargo byggede fra lockfilen.
`CARGO_MANIFEST_DIR` peger på core-craten, så præcis den indcheckede fixture-test
og alle JSON-filer køres. Testbinæren blev kørt efter den sidste kodeændring.
Sidste ikke-tomme outputlinje:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

Dette ene test indeholder **66 fixtures med 120 snapshots**, inklusive gentagne
kald, ombytning af inputrækkefølge og serialisering af tilstanden.

`rustfmt --edition 2021 --check` består for alle detektorens Rust-filer og
`lib.rs` (med `skip_children=true` på modulrødder). `git diff --check` og
`git diff origin/main --check` består for PR'ens ændringer. Main indeholder
selv en eksisterende trailing whitespace i `frontend/src-tauri/src/utils.rs`;
den er ikke ændret som del af denne opgave.

Både `cargo test -p tingskriver-core meeting_detect --lib --locked` og den
fulde berørte crates suite, `cargo test -p tingskriver-core --locked`, er
forsøgt. Begge stopper før tests med exit 101, fordi `alsa-sys` mangler
systembiblioteket ALSA (`alsa.pc`). Sidste outputlinje i det fulde forsøg:

```text
warning: build failed, waiting for other jobs to finish...
```

Core-cratens fulde suite og Cargo-baseret clippy er derfor ikke verificeret.
Ingen workspace-builds eller andre crates er bygget som mål.

## Issue #33 og Fixes

Det ønskede `gh issue view 33 -R mahope/tingskriver --comments` blev forsøgt,
men gh manglede GH_TOKEN. Issue og kommentar blev i stedet læst via GitHubs
uautentificerede offentlige REST API.

Alle tre acceptkriterier er dækket på modulniveau: over 30 JSON-fixtures,
alle seks app-/browserkilder samt Discord, diktering og medieafspilning;
stopgrænser med eksplicit falsk tid; og platformfri kompilering/test på Linux.
Kommentaren foreslår også virkelige Windows-målinger og UI Automation-etiketter.
Det er integrationsarbejde til #59, ikke leveret af dette rene modul.

Der er ingen kendt resterende implementeringsopgave i #33's angivne scope.
Før PR'en markeres klar og bruger `Fixes #33`, mangler grøn Cargo-test/CI i et
miljø med core-cratens systemafhængigheder samt det aftalte samlede review fra
Mads' PC. Review og ændring af PR'ens draft-status er ikke udført her.

Ingen af de to P1-fund er afvist. Den snævre påstand om, at kalenderbinding kun
kan ske ved start, er allerede delvist løst på den oprindelige gren: senere
aktive overlap kunne bindes. Ændrede sluttider og opslag efter aftaleslut var
stadig fejl og er rettet her.

Kalenderinputtet har intet stabilt begivenheds-id. Bindingen bruger starttiden;
ændring af selve starttiden og to aftaler med identisk starttid kan ikke
håndteres entydigt uden en udvidet inputmodel. Det er dokumenteret som en
begrænsning, ikke skjult som verificeret funktionalitet.

## Forslag til PR-tekst

Titel: `Preserve active meetings and refresh calendar stop deadlines`

Beskrivelse:

```text
Add a pure meeting detector with 66 JSON fixtures covering all six meeting
sources, false positives, start modes and deterministic stop rules.

Preserve active meetings when another microphone candidate appears, and
refresh calendar stop deadlines without replacing the bound meeting.

Validation: the Linux detector test passes all 66 fixtures / 120 snapshots;
rustfmt and diff checks pass. Full core Cargo tests are blocked locally by
the missing ALSA system library. Complete CI and the planned PR review before
marking this draft ready.

Refs #33
```
