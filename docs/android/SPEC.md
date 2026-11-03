# Tingskriver Android v0

Status: låst MVP-scope, 2. november 2026. [Research og begrundelser](RESEARCH.md).
Milepæl 1 (skelet) og 4 (PCM-optagelse) er implementeret. Fysisk kontrol af
mikrofonindikatoren udestår; talegenkendelse er ikke tilsluttet.

## Resultat og betjening

Android 8.0+ (minSdk 26) får et separat dansk dikteringstastatur. Brugeren vælger
Tingskriver i Androids tastaturliste, trykker mikrofonen, taler og trykker igen
for at afslutte. Hold-tal-slip kan tilføjes efter v0; tryk-start/tryk-stop er
tilgængeligt med TalkBack og kræver ingen langvarig berøring. Teksten indsættes
én gang ved markøren i det oprindelige felt. Der sendes aldrig Enter automatisk.

Mikrofon, læsbar status, annuller og skift tastatur er v0-betjeningen. Tastaturet
har mikrofonknap, status, annuller og tastaturskift. PCM optages kun i RAM og
kasseres ved stop; STT er endnu ikke tilsluttet, og ingen tekst indsættes.
Indstillingsaktiviteten åbner systemets IME-indstillinger og tastaturvælger.

## Låste valg

- Native Kotlin `InputMethodService` og almindelige Android Views, ikke Tauri,
  webview eller en fork af et fuldt tastatur. MIT, ingen GPL/source-first-kode.
- minSdk 26 giver en rimelig supportgrænse og moderne lifecycle-API'er. API 31
  er kun nødvendig for systemets on-device recognizer, som vi ikke vælger.
  compileSdk/targetSdk 36, JDK 17, Gradle 9.6.0, AGP 9.5.0, Kotlin 2.4.21.
- Lokal whisper.cpp med multilingual base som udgangspunkt, dansk `da`,
  arm64-v8a først. Konkret native-version, kvantisering og modelhash fastlåses
  i benchmarkmilepælen. Ingen STT eller NDK-afhængighed i skelettet.
- Deterministisk dansk tekstbehandling fra eksisterende Rust-logic gennem en
  senere smal UniFFI-bro. Ingen LLM-polering i v0. Før broen er verificeret,
  foregives ingen genbrug eller dansk oprydning i Kotlin-stubben.
- Ingen cloud-fallback. Ved manglende model vises downloadvejledning; ved fejl
  vises en fejl og optagelsen kasseres. Eksisterende desktopudbydere ændres ikke.

## Arkitektur og sessioner

```text
SettingsActivity -> aktivering, modelvalg og mikrofontilladelse
InputMethodService -> session-controller -> SpeechToText-interface
                                           -> lokal whisper.cpp-adapter (senere)
                   -> tekstbehandling -> current session InputConnection.commitText
                                        ^
                              lille UniFFI-tekstbro (senere)
```

`SpeechToText` returnerer et eksplicit resultat: tekst, tom tale, annulleret,
ikke implementeret eller fejl. Stubben returnerer kun ikke implementeret.
Interfacet har synkront `transcribe` med in-memory PCM; optagelsen kalder
det endnu ikke. En virkelig adapter skal kaldes på en worker,
aldrig på UI-tråden. PCM bliver mono 16 kHz, 16-bit med højst 30 s pr. session
(ca. 960 kB rå lyd). AudioRecord-ejerskab, worker og cancellation er
implementeret; adapteren er endnu ikke aktiveret. Ingen midlertidig WAV-fil.

MVP-tilstande: inaktiv, optager, behandler, fejl. Hver session får generation og
det oprindelige felt/forbindelse. `onStartInput`, `onFinishInput`, skjult IME,
skærmlås og service-destruktion annullerer og invaliderer sessionen. Et sent
callback må aldrig bruge en ny `currentInputConnection`. Højst én optagelse og
ét commit pr. session; blank tekst indsættes ikke. Manglende eller afvist
InputConnection giver en brugerfejl uden clipboard-fallback.

Password-varianter (tekst, web, synligt og numerisk), `TYPE_NULL` og talfelter
afviser diktering. Tekstfelter med `IME_FLAG_NO_PERSONALIZED_LEARNING` må gerne
modtage lokal diktering, men må ikke bruges til læring/historik. Understøttelse
af almindelige tekstfelter er målet, ikke en garanti for specialbyggede editors.

## Privatliv

Appen har RECORD_AUDIO og runtime-forespørgsel fra aktiviteten med dansk
forklaring. Den håndterer afslag og senere tilbagekaldelse. Ingen INTERNET,
netværksklient, analyse, tekstlogning eller persistens. Backup er deaktiveret.
Optagelse starter kun ved brugerens handling mens IME er synlig og ulåst.
Ingen baggrundsmikrofon eller Accessibility,
overlays, kontaktadgang, clipboard eller læsning af omgivende felttekst.
PCM og transskription lever kun i RAM og frigives ved stop/annuller/fejl.
Vi lover ikke kryptografisk sletning af JVM-strenge. Modelfiler og brugerens
egen ordbog kan gemmes privat lokalt; lyd og transskripthistorik gemmes aldrig.
Modeldownload er den eneste planlagte netadgang, opt-in og med checksum.

## Ikke-mål

Fuld QWERTY, swipe, emoji, cloud/BYOK, konti/synkronisering, mødeoptagelser,
diarisering, oversættelse, kommandoer til andre apps, snippets, stil pr. app,
LLM-omskrivning, systemomfattende hands-free, baggrundsservice og Play-udgivelse.
Ingen ændring af root Cargo.toml/Cargo.lock eller desktopflows i denne fase.

## Milepæle og accept

1. **Skelet (denne ændring):** registrerbar IME, to knapper, settings, ærlig
   STT-stub og rene JVM-tests. Separat path-filtreret Android-CI bygger debug-APK,
   kører tests og lint. Ingen eksisterende workflow redigeres.
2. **Baseline:** optag et samtykket/offentligt dansk testmateriale med støj,
   egennavne og stilhed. Mål base/small på to arm64-telefoner; offentliggør kun
   ikke-personlige resultater. Mål: WER <= 15 % på fastlåst stille talesæt,
   ingen tekst ved stilhed, warm stop-til-tekst p95 <= 3 s for 15 s lyd,
   peak PSS <= 700 MB for base. Dette er acceptmål, ikke observerede resultater.
3. **Modelhåndtering:** lås native-version/modellicens/hash, download med
   brugerens valg, checksum og atomisk aktivering; håndter afbrud og lav disk.
   Ingen model i git/APK og ingen hemmelige URL'er.
4. **Optagelse:** tilladelsesflow, AudioRecord til begrænset PCM-buffer,
   stop/annuller og lifecycle-afbrydelse. Bekræft mikrofonindikator slukker.
5. **Lokal STT:** arm64-adapter på worker, dansk model, tidsgrænse og annullering;
   tests af fejl, blankt resultat og dobbelte tryk. Ingen UI-blokering.
6. **Sikker indsættelse:** generationsbundet commit, nul input til passwords;
   test feltskift under behandling, samme app med nyt felt, afvist forbindelse
   og præcis én indsættelse. Ingen sendehandling eller clipboard.
7. **Dansk core:** lille platformuafhængig crate og UniFFI, samme Rust-fixtures
   for talte tegn og ordbog på Android. Ingen desktopafhængigheder over grænsen.
8. **Enhedsgate:** Settings, TalkBack, mørkt tema, rotation, landscape, lås,
   batteri/termik og IME-skift testes på to fysiske telefoner og API 26-emulator.
   Gem før/efter-skærmbilleder og måleresultater uden persondata.

## Verifikation og risici

En ren JVM kan teste resultatkontrakt og feltpolitik. Gradle `assembleDebug`,
`testDebugUnitTest` og `lintDebug` er Android-gaten. CI uploader debug-APK og
testrapporter. Fysisk mikrofon, OEM-lifecycle, TalkBack og tastaturskift kræver
enhed og må ikke kaldes verificeret af enhedstests alene.

Største risici: små Whisper-modellers danske fejl, hallucinationer ved stilhed,
model-RAM i en IME-proces, OEM-procesdrab, termisk throttling og at resultatet
ellers kunne ende i et nyt felt. Derfor benchmark før native-integration,
in-memory grænser og sessionskontrol før rigtig indsættelse. UniFFI kræver
udtrækning af en lille del af core; hele desktopcraten må ikke krydsbygges
ukritisk. AGP/Kotlin er nye stabile versioner med begrænset offentlig
kompatibilitetsmatrix, så CI-build er et nødvendigt bevis før merge.
