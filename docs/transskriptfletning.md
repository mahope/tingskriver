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
