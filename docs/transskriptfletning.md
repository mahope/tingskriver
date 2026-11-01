# Transskriptfletning

`tingskriver_core::transcript_merge::merge(mic, system, config)` er en ren funktion:
inddata er lånte segmentlister, resultatet er en ny liste. Modulet bruger ingen
UI, database, lyd, netværk, logning eller I/O. Tider er `u64` millisekunder fra
samme optagelsesstart; adapteren skal sikre fælles tidsakse.

## Kontrakt

- Alle segmenter bevares én gang, inklusive tom tekst og dubletter. Input ændres ikke.
- Segmenterne sorteres stabilt på starttid. Ved samme starttid kommer mikrofon
  før system og derefter segmenternes oprindelige rækkefølge inden for sporet.
  Resultatets starttider er derfor monotone, også for usorterede input.
- Slut før start klampes til start (længde nul). Ingen tidspunkter lægges sammen,
  så også `u64::MAX` håndteres uden overløb.
- Overlap betyder positiv fælles varighed i halvt åbne intervaller `[start, slut)`.
  Begge segmenter markeres, også ved indlejrede intervaller og overlap inden for
  samme spor. Tekster sammenskrives aldrig. Berørende endepunkter og nul-længde
  tæller ikke som overlap.
- Mikrofonens taler er altid `Taler::Mig`, uanset inputtaler og diarisering.
  `taler_mangler` er altid falsk for mikrofonen.
- Når `system_diariseret` er sand, får hvert ikke-tomt system-id et nummer ved
  første optræden i det sorterede resultat. Samme id får samme nummer. Id'er
  sammenlignes præcist og uden fortolkning af fx `0` og `SPEAKER_01`.
- Manglende, tomme og whitespace-only id'er giver `Taler::Ukendt` og
  `taler_mangler = true`. Når diarisering er slået fra, gælder det alle
  systemsegmenter, også hvis inddata stadig indeholder id'er.
- Standardlabels er `Mig`, `Taler 1..n` og `Ukendt taler`. Konstanterne ligger
  foreløbig i modulet og kan senere flyttes til strengkataloget.
  `Taler::label(&config)` bruger konfigurationens mikrofon- og ukendt-label.
  Konfigurationen bør beholdes sammen med resultatet ved senere visning.
- Tomme spor er gyldige; begge tomme giver et tomt resultat.

Sorteringen koster O(n log n); talernummerering og overlapmarkering koster
forventet O(n). Arbejdslager er O(n): en samlet liste af referencer, stabil
sorteringsbuffer og et id-opslag med lånte strenge. Der allokeres ikke en ekstra
streng eller en separat arbejdsbuffer pr. segment. Tekst kopieres kun til det
ejede output; labelstrenge dannes først ved eksplicit visning af et talernummer.

## Fremtidig databaseadapter (ikke implementeret her)

`database::models::Transcript` har `transcript`, `audio_start_time` og
`audio_end_time` (valgfrie `f64` sekunder), men endnu intet `speaker`-felt.
Migration `20251110000001_add_speaker_field.sql` tilføjer `transcripts.speaker`
med **lydkilde** (`mic`/`system`), ikke diariserings-id. Transskriptionsworkerens
`TranscriptUpdate` bruger ligeledes sekunder; retransskription gemmer dem i
disse databasekolonner. Nuværende data giver derfor ikke automatisk diarisering.

En tynd adapter skal hente lydkilden særskilt, dele rækkerne i de to spor,
konvertere validerede, endelige, ikke-negative sekunder til millisekunder
(med dokumenteret afrunding og range-check), kopiere `transcript` til `tekst`
og hente et eventuelt diariserings-id fra den fremtidige diarisering.
Den må aldrig bruge `mic`/`system` som diariserings-id. Manglende eller ugyldige
tider skal håndteres eksplicit af adapteren, ikke opfindes fra `timestamp`
(vægurtid). Når diarisering mangler, bruges `None` og
`system_diariseret = false`. Navnet fra indstillinger gives som `mikrofon_label`.

## Test

14 syntetiske JSON-fixtures i `tests/fixtures/transcript_merge/` har eksplicitte
forventninger til samtlige outputfelter og labels: overlap, huller, hvert tomt
spor, begge tomme, helt/delvis manglende diarisering, tilbagevendende talere,
lige starttider, usorteret input, omvendt interval, danske tegn, indlejring og
berørende endepunkter/tom tekst.

Property-testen genererer begge spor med vilkårlige `u64`-tider og Unicode-tekst.
Den kontrollerer monotone starter, antal og tekst-multimængde samt overlap mod
en uafhængig parvis reference. En positiv kontrol bruger samme monotoni-predikat
og beviser, at proptests runner afviser en mutation, som sorterer på sluttid.

```sh
cargo test -p tingskriver-core transcript_merge --lib
```

## Afhængighedsvurdering og verifikation (issue #18)

Eneste direkte tilføjelse er dev-afhængigheden `proptest = "1"`, låst til
1.9.0 for at undgå opgraderinger af eksisterende pakker. Fem nye pakker i
lockfilen; ingen eksisterende versioner ændres. Vurderet mod crates.io-indekset
og de lokalt tilgængelige pakkekilder:

- `proptest` 1.9.0: proptest-rs/ Jason Lingle, eksisteret siden juni 2017,
  valgt udgivelse 26. oktober 2025; nyeste 1.12.0 fra 10. oktober 2026.
  Ingen build-script eller proc-makro i standardfeatures. Læser `PROPTEST_*`
  til testkonfiguration; ingen netværkskald ved indlæsning. Kræver Rust 1.82
  til tests (arbejdsmiljø og CI bruger stable). Konklusion: installér som dev-afhængighed.
- `quick-error` 1.2.3: Paul Colomiets/Colin Kiegel, siden september 2015,
  valgt udgivelse januar 2020; nyeste 2.0.1 fra maj 2021.
  Deklarative makroer, intet build-script eller proc-makro. Konklusion: installér transitivt.
- `rand_xorshift` 0.4.0: Rand Project, siden oktober 2018, valgt udgivelse
  januar 2025; nyeste 0.5.0 fra februar 2026. Ingen build-script eller
  proc-makro. Konklusion: installér transitivt.
- `rusty-fork` 0.3.1: Jason Lingle, siden februar 2018, seneste udgivelse
  4. oktober 2025. Ingen build-script eller proc-makro; procesisolering og
  timeout ved eksplicit testkørsel. Konklusion: installér transitivt.
- `unarray` 0.1.4: proptest-rs (repository i pakkemanifest), siden juni 2022,
  seneste udgivelse november 2022. Ingen build-script eller proc-makro.
  Konklusion: installér transitivt.

De fire transitive tilføjelser laver ikke netværkskald ved indlæsning.
Ingen af de seneste udgivelser er under tre døgn gamle. De eksisterende
transitive pakker genbruges; licenser og bans passerer `cargo deny --locked
check licenses bans` (`bans ok, licenses ok`).

Verificeret på Linux med stable Rust:

- Målrettet: `cargo test --locked -p tingskriver-core transcript_merge --lib`:
  **5 passed; 0 failed**, inklusive 14 fixtures og mutationskontrol.
- Fuld suite: `cargo test --locked -p tingskriver-core`:
  **484 passed; 0 failed; 2 ignored**. Doc-tests: **1 passed; 0 failed; 1 ignored**.
- `cargo xwin check --locked --target x86_64-pc-windows-msvc -p tingskriver-core
  -p tingskriver-overlay -p tingskriver-app`: bestået med LLVM 19 på PATH.
- De to nye Rust-filer passerer `rustfmt --edition 2021 --check`.
  `cargo clippy --locked -p tingskriver-core --lib --tests` har ingen fund i
  modulet, men stopper på tre fejl i urørte filer: `backend_config.rs:55`
  (`inherent_to_string_shadow_display`) og `whisper_engine.rs:1690,1890`
  (`unused_io_amount`). Begge filer er identiske med grenens main-base.

Ingen ændringer i audio, database, dictation eller arkitekturdokumentet.
De delte filer har kun én moduleksport og den nye dev-afhængighed med lockfil.
Tilføj senere denne linje til `docs/arkitektur.md`:

> `transcript_merge`: ren tidsfletning af mikrofon- og systemsegmenter med talerlabels og overlapmarkering; se [transskriptfletning.md](transskriptfletning.md).

Manuel Windows-kontrol: kør `cargo test --locked -p tingskriver-core
transcript_merge --lib` fra en udviklerterminal og kontrollér fem beståede tests.
Der er endnu ingen UI-integration eller adapter, som kan afprøves i appen.
Fresh-context PR-review udføres fra Mads' PC efter opgavens fælles regler.
