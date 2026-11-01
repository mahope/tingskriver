# Dansk WER-regression

`tingskriver_core::wer::{normalize, score, Score}` er den lille, fælles API
for ordfejlrate, som også kan genbruges af appens kommende måleværktøj (#69).
Der tilføjes ingen pakker eller talemodeller. JSON-kørslen bruger eksisterende
`serde`, `chrono`, `anyhow` og `tokio`.

## Beregning og normalisering

Begge tekster normaliseres identisk: Unicode-små bogstaver, bogstaver og tal
bevares, alle andre tegn bliver ordgrænser, og mellemrum samles. Æ, ø og å
bevares. Eksempel: `HEJ, ø!` bliver `hej ø`; `rød-grøn` bliver `rød grøn`.
Apostroffer deler også ord. Decimaltegn deler tal (`1,5` bliver `1 5`).
Tal omskrives ikke til ord, `aa` og `å` slås ikke sammen, og der sker ingen
Unicode NFC/NFKC, dialektnormalisering, fjernelse af fyldord eller sproglig
polering. Kombinerende accenter er ikke alfanumeriske og bliver ordgrænser.
Versionen hedder `unicode-alphanumeric-lower-v1`; skift den ved regelændringer.

Ord-Levenshtein tæller substitutioner, indsættelser og sletninger. Ved samme
afstand vælges diagonal (match/substitution), så sletning, så indsættelse.
Summen er altid minimal; ved flertydige alignments kan fordelingen variere
fra andre værktøjer. Hukommelse: to rækker af hypotesens ordantal.

WER = (S + I + D) / antal referenceord. `Score::wer()` og JSON's `wer` er
brøker (0,1 = 10 %). WER kan være over 100 %. Samlet WER er **sum af fejl /
sum af referenceord**, aldrig gennemsnittet af klippenes procenter.
Tom/tom giver 0; tom reference med hypotese giver `None`/JSON `null` (udefineret),
og ikke-tom reference med tom hypotese giver 100 %. Gaten afviser klip uden
normaliserede referenceord; de skal afklares før en baseline kan godkendes.

## Manifest og klip

`testsaet/dansk-wer/manifest.json` er en tom skabelon for **rigtig lyd**.
Ingen lyd er hentet; årsag, kildekontrol og udvælgelsesopskrift står i
[KILDER.md](../testsaet/dansk-wer/KILDER.md). Loaderen fejler på et tomt testsæt.
Eksempel på en post (erstat alle pladsholdere med dokumenterede værdier):

```json
{
  "schema_version": 1,
  "kind": "audio",
  "clips": [{
    "id": "da-001",
    "file": "clips/da-001.wav",
    "reference": "Den ordrette reference for dette klip.",
    "speech_type": "oplaesning",
    "dialect": "unknown",
    "source": "Præcis kilde-URL, revision, split og oprindeligt id",
    "license": "CC-BY-4.0 og licenslink",
    "attribution": "Den tilskrivning som kilden kræver",
    "consent": "source-documented"
  }]
}
```

Alle felter er obligatoriske; ukendte felter afvises. `speech_type` er
`oplaesning` eller `samtale`; `dialect` er tekst (`unknown` tilladt).
Samtykke er `documented` (egen dokumentation), `source-documented` (grundlag
i kilden) eller `not-applicable` (kun tekstfixtures). En streng i manifestet
er ingen juridisk kontrol; dokumentér grundlaget i KILDER.md før optagelser
bruges eller offentliggøres. Nye optagelser kræver samtykke fra alle medvirkende.

Loaderen kræver 1..20 unikke id'er med ASCII-bogstaver, tal, `_` og `-`,
ikke-tomme metadata, relative filstier og eksisterende filer inden for
manifestets mappe (også efter symlink-opløsning). Audio-kørslen afviser andet
end RIFF/WAVE PCM16, mono, 16 kHz, 0 < varighed <= 8 sekunder. Intet konverteres
eller downloades automatisk. Filstørrelsen begrænses før indlæsning.

## Deterministisk kørsel uden modeller

Kør fra repository-roden på Linux (på Windows sættes miljøvariablerne tilsvarende):

```bash
export PATH=$HOME/.cargo/bin:$PATH
export CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver
cargo test --locked -p tingskriver-core --lib 'wer::'
python3 scripts/test-wer-gate.py
bash scripts/wer-fixture-check.sh /tmp/wer
```

`fixtures/manifest.json` har `kind: "fixture-text"`. Filfeltet peger på en
tekstfixture, aldrig kunstig lyd eller en påstået optagelse. `FixtureEngine`
læser `<id>.txt` fra `--hypotheses`. Rapporten gemmer reference, hypotese,
S/I/D, antal ord og WER pr. klip samt total, motor, UTC-dato, fuld Git-SHA,
normaliseringsversion og testsættype. Hypoteser/referencer skrives kun i
JSON-rapportfiler, aldrig i terminal eller fejlbeskeder. Eksemplet installerer
bevidst ingen logger; eksisterende motorers debug-logning kan indeholde tekst.
Ved embedding af adapterne skal værten tilsvarende deaktivere tekstlogning.

```bash
cargo run --locked -p tingskriver-core --example wer_run -- \
  --manifest testsaet/dansk-wer/fixtures/manifest.json --engine fixture \
  --hypotheses testsaet/dansk-wer/fixtures/good --out /tmp/good.json
python3 scripts/wer-gate.py --rapport /tmp/good.json \
  --baseline testsaet/dansk-wer/fixtures/baseline.json --markdown /tmp/good.md
```

Den indcheckede fixture-baseline har 0 % WER. `bad/` fjerner ord og skal give
exit **1**, som scriptet kræver eksplicit. En ødelagt fil/opsætning (exit 2)
kan ikke bestå som positiv kontrol. CI-workflowet `wer.yml` tester modulet,
gaten og begge fixture-kørsler og uploader JSON og markdown. Det henter
ingen talemodeller. Denne kontrol beviser ikke, at virkelig VAD slår igennem.

## Gate og baseline

```bash
python3 scripts/wer-gate.py --rapport rapport.json --baseline baseline.json \
  --markdown sammenligning.md
# Kun på den valgte referencemaskine, efter gennemgang:
python3 scripts/wer-gate.py --rapport rapport.json --baseline baseline.json \
  --skriv-baseline
```

Exit 0 = højst 1,0 procentpoint forværring; exit 1 = **mere end** 1,0 pp;
exit 2 = ugyldige/usammenlignelige data, filfejl eller manglende baseline.
Ved manglende baseline siger gaten eksplicit, at `--skriv-baseline` skal
køres på referencemaskinen. Sammenligningen bruger heltalskrydsprodukter,
så præcis 1,0 pp består uden flydende-komma-grænsefejl. Test dækker også
1,1 pp, forbedring, manglende og nye klip samt metadata- og referenceændringer.

Motor/model, normaliseringsversion, testsættype, klip-id'er og referencetekst
skal være identiske. Totalsummer og WER kontrolleres mod tællingerne; ugyldige,
negative og ikke-endelige tal afvises. Markdown viser kun id'er og tal.
Gaten genberegner ikke selve Levenshtein-alignments fra JSON; rapporter skal
komme fra den betroede runner. Lydindhold/modelvægte hashes ikke i rapporten:
fastlås testsættets revision, modelversion, konfiguration og maskine i baseline-
commitbeskrivelsen, og ændr ikke lyd under samme id. Én baseline pr. motor/model.
Der findes endnu ingen lyd-/modelbaseline; fixture-baselinen må ikke bruges til det.
Branch protection skal særskilt kræve WER-jobbet, hvis det skal blokere merge.

## Windows med installerede modeller (manuel)

1. Indsaml og registrér klip efter KILDER.md. Brug samme klip og modelvægte
   ved alle sammenligninger. Start fra en ren checkout på referencemaskinen.
2. Installér modeller gennem appens eksisterende modelhåndtering. Brug dens
   præcise modelnavn og modelmappe, ikke et opfundet navn. Runneren downloader
   intet. Parakeet kræver den ONNX Runtime-DLL som appen leverer; sæt
   `ORT_DYLIB_PATH` til den, før processen startes.
3. Kør i PowerShell fra repoets rod, med egne eksisterende stier/navne:

   ```powershell
   $env:PATH = "$HOME\.cargo\bin;$env:PATH"
   $env:CARGO_TARGET_DIR = "$HOME\dev\cargo-target\tingskriver"
   $env:ORT_DYLIB_PATH = Read-Host 'Fuld sti til appens onnxruntime.dll'
   $models = Read-Host 'Eksisterende modelmappe'
   $model = Read-Host 'Installeret Parakeet-modelnavn'
   cargo run --locked -p tingskriver-core --example wer_run -- --manifest testsaet/dansk-wer/manifest.json --engine parakeet --models-dir "$models" --model "$model" --out parakeet.json
   python scripts/wer-gate.py --rapport parakeet.json --baseline testsaet/dansk-wer/baseline-parakeet.json --skriv-baseline
   $model = Read-Host 'Installeret Whisper-modelnavn'
   cargo run --locked -p tingskriver-core --example wer_run -- --manifest testsaet/dansk-wer/manifest.json --engine whisper --models-dir "$models" --model "$model" --out whisper.json
   python scripts/wer-gate.py --rapport whisper.json --baseline testsaet/dansk-wer/baseline-whisper.json --skriv-baseline
   ```

4. Efter en ændring: kør samme kommando igen og sammenlign med den gemte
   baseline **uden** `--skriv-baseline`. Gem JSON og markdown som bevis.
   Opdatér aldrig automatisk baseline efter en regression.

Adapterne bruger `ParakeetEngine::transcribe_audio` og
`WhisperEngine::transcribe_audio(..., Some("da"))`. De kompileres også på Linux
og ved `cargo xwin check ... -p tingskriver-core --example wer_run`. De går
**direkte til de eksisterende model-API'er**, ikke gennem mikrofon, VAD,
bidder, mødeorkestrering eller valgfri polering.

## Bevis for den fulde transskriptionskæde (mangler)

Der er ingen stabil offentlig fil-til-fuld-kæde-API med en VAD-kontakt i denne
opgaves tilladte filer. Runneren foregiver derfor ikke at teste den. På Windows:

1. Afspil/importér samme rettighedsklare klip gennem appens normale optagelses-
   og transskriptionskæde på en fast model. Gem det endelige output pr. klip.
2. Beregn hvert klip med `wer::score(reference, output)` og byg en `Report`
   med `Report::new`/`push`, samme klip-id'er og et særskilt motornavn, fx
   `parakeet:<model>:full-chain:<settings-revision>`. Gem den som baseline.
3. Lav en lokal, kontrolleret VAD-ændring i en separat gren eller brug en
   eksisterende diagnostisk kontakt. Denne gren tilføjer ingen VAD-kontakt.
   Gentag den faktiske optagelses-/kædekørsel og generér samme rapportformat.
4. Kør gaten og kræv exit 1. At slå VAD fra garanterer ikke højere WER; hvis
   den ikke forværres over 1 pp, er den kontrol **ikke bevist**. Brug en
   dokumenteret kædefejl, som klipper tale væk, og gem indstillinger og resultater.
5. Gendan de oprindelige indstillinger og kør igen; bekræft positivt, at
   output og WER svarer til baseline. Fjern ikke fejlbeviset.

Tekstfixture-kontrollen simulerer manglende ord. Rigtige modeller, det endelige
lydtestsæt og en reel VAD-/kædeforringelse er fortsat manuelle, ubeviste dele af #30.
