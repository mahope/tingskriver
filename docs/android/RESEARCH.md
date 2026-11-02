# Android-diktering: research

Undersøgt 2. november 2026. Repoets udgangspunkt: `d760060`. Dette er en
designundersøgelse, ikke en måling på Android. Ingen telefon, cloudtjeneste eller
betalingskonto blev brugt. Ubekendte tal er angivet som ubekendte.

## Kerneoplevelsen i Wispr Flow

Man fokuserer et tekstfelt, starter tale, taler naturligt og får redigerbar tekst
ved markøren. Lav friktion, hurtig afslutning og brugbare danske sætninger er
vigtigere end en fuld tastaturlayoutmotor.

| Platform | Start og indsættelse | Relevans for Tingskriver |
| --- | --- | --- |
| Desktop | Genvej holdes nede eller hands-free; Flow Bar viser status | Hold-tal-slip og synlig optagestatus er forbilledet |
| iOS | Flow Keyboard med trykbetjening | Et separat dikteringstastatur er et brugbart produkt |
| Android | Flow Bubble over det eksisterende tastatur; Accessibility bruges til indsættelse | Vores IME bruger i stedet Androids `InputConnection`; ingen overlay eller Accessibility |

Flow renser fyldord og retter formulering, bruger personlig ordbog og kan tilpasse
stil til anvendelsen. Desktop har kommandoer til tekstredigering. Funktioner og
snippets er ikke ens på alle platforme; vi lover ikke desktopparitet på Android.
Flow kræver internet til transskription. Tingskrivers v0 vælger lokal behandling.

Primære kilder: [produktbeskrivelse](https://docs.wisprflow.ai/articles/2772472373-what-is-flow),
[platformenes betjening](https://docs.wisprflow.ai/articles/5096240724-navigating-the-wispr-flow-app-desktop-ios-and-android),
[hands-free og platformbegrænsninger](https://docs.wisprflow.ai/articles/6391241694-use-flow-hands-free),
[Android-opsætning og tilladelser](https://docs.wisprflow.ai/articles/8858845757-setup-wispr-flow-on-android-android-settings),
[Android-produkt](https://wisprflow.ai/android).

## Eksisterende IME'er og licenser

Tingskriver har [MIT-licens](../../LICENSE.md). En offentlig kildekode er ikke
nødvendigvis open source. Tabellen er en teknisk integrationsbeslutning: ingen
tredjeparts-IME-kode er kopieret i denne ændring. Ved faktisk genbrug skal den
konkrete fils licens, transitive biblioteker, modellicens og NOTICE kontrolleres.

| Projekt og primær kilde | Arkitektur og modenhed | Licens og mulighed for genbrug |
| --- | --- | --- |
| [FUTO Voice Input](https://github.com/futo-org/voice-input), [licens](https://github.com/futo-org/voice-input/blob/master/LICENSE.md) | Lokal Whisper, voice input som separat IME; etableret brugerprodukt og kildekodespejl fra FUTO GitLab | FUTO Source First 1.0 begrænser bl.a. kommerciel brug og sublicensering. Ikke OSI open source, ikke MIT-genbrug; læs som inspiration |
| [FUTO Keyboard](https://github.com/futo-org/android-keyboard), [voice input](https://docs.keyboard.futo.tech/settings/voiceinput) | Fuldt tastatur med integreret lokal tale og mulighed for ekstern taletjeneste; langt større vedligeholdelsesflade end vores behov | Source-first produkt; ikke grundlag for at kopiere kode ind under MIT. Kontroller også den særskilte keyboard-licens før eventuelt genbrug |
| [Sayboard](https://github.com/ElishaAz/Sayboard), [licens](https://github.com/ElishaAz/Sayboard/blob/master/LICENSE.txt) | Vosk-baseret lokal voice IME, modeldownload; mindre projekt, seneste registrerede push juli 2025 | GPL-3.0: kan bruges i et GPL-kompatibelt samlet produkt med GPL-forpligtelser, men den kopierede kode kan ikke bare relicenseres MIT. Ingen kodegenbrug i v0 |
| [WhisperInput](https://github.com/alex-vt/WhisperInput), [licens](https://github.com/alex-vt/WhisperInput/blob/master/LICENSE) | Eksperimentel lokal Whisper/whisper.cpp med Kõnele-komponenter; IME, panel og assistent. Standardmodel er engelsk, skal erstattes til dansk | Egen kode MIT, Kõnele Apache-2.0. Muligt med bevarede notices og kontrol af alle komponenter; ikke valgt som base |
| [whisper-to-input](https://github.com/j3soon/whisper-to-input), [licens](https://github.com/j3soon/whisper-to-input/blob/master/LICENSE) | Android IME med fjernbackend, bl.a. OpenAI og selvhostet Whisper; ikke det samme som WhisperInput. Mindre projekt, registreret push december 2025 | GPLv3 ifølge README og LICENSE; ikke kopiering ind som ren MIT. Arkitekturreference |
| [HeliBoard](https://github.com/HeliBorg/HeliBoard), [licens](https://github.com/HeliBorg/HeliBoard/blob/main/LICENSE) | Fuldt privacy-orienteret tastatur fra OpenBoard/AOSP; ekstern voice input frem for egen dansk STT. Etableret tastaturbase | Samlet GPLv3, enkelte AOSP-filer Apache-2.0. Fork af hele tastaturet medfører GPL-forpligtelser. Ikke valgt |
| [FlorisBoard](https://github.com/florisboard/florisboard), [licens](https://github.com/florisboard/florisboard/blob/main/LICENSE) | Kotlin-tastatur med layout-, tema- og indstillingssystem; upstream betegner det beta. Talemotor kræver særskilt integration | Apache-2.0; kan kombineres med MIT-kode, men oprindelig licens og notices skal bevares. For stor base til en mikrofon-IME |

**Valg:** skriv en lille IME fra bunden. Androids platform-API løser indsættelsen;
et komplet bogstavtastatur giver os unødvendig layout-, gestus- og ordbogsdrift.

## Lokal STT

| Motor | Dansk og integration | Størrelse, RAM, svartid og strøm |
| --- | --- | --- |
| [whisper.cpp / ggml](https://github.com/ggml-org/whisper.cpp) | MIT; Android-eksempel og C/JNI. Multilingual modeller har dansk (`da`); `.en` er uegnede. Kvantisering understøttes | Upstreams reference: tiny 75 MiB/~273 MB RAM, base 142 MiB/~388 MB, small 466 MiB/~852 MB, medium 1,5 GiB/~2,1 GB. Ikke Android-peakmålinger. CPU-inferens kan varme og belaste batteriet; cold/warm latency skal måles |
| [sherpa-onnx](https://k2-fsa.github.io/sherpa/onnx/android/index.html), [Whisper-modeller](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/whisper/index.html) | Apache-2.0 runtime, Kotlin/JNI og Android-eksempler. Dansk afhænger af den valgte model, ikke runtime. Multilingual Whisper er en kandidat; mange andre modeller er sprogbegrænsede | ONNX-encoder/decoder og evt. kvantisering giver modelspecifik disk/RAM. Ikke direkte sammenligneligt med ggml-tal. Ingen verificeret dansk Android-latency eller batterimåling her; QNN-understøttelse er ikke automatisk acceleration af alle modeller |
| [Vosk](https://github.com/alphacep/vosk-api), [modelkatalog](https://alphacephei.com/vosk/models) | Apache-2.0 API, Android streaming og justerbart ordforråd. Der blev ikke fundet en dansk model i det officielle katalog; derfor fravalgt til dansk MVP | Små modeller generelt omkring 50 MB/~300 MB runtime ifølge upstream; det er ikke tal for dansk. Typisk streaming-egnet, men kvalitet/strøm kan ikke vurderes uden en dansk model |
| [Android SpeechRecognizer](https://developer.android.com/reference/android/speech/SpeechRecognizer) | `createOnDeviceSpeechRecognizer` fra API 31; kontroller `isOnDeviceRecognitionAvailable`. Dansk og modeltilgængelighed varierer med telefon og installeret service. Support-/download-API'er kommer i nyere API-niveauer | Ingen egen model i APK. Modelstørrelse, RAM, svartid og energiforbrug bestemmes af systemtjenesten og er ubekendte. Ikke egnet til kontinuerlig lytning. `EXTRA_PREFER_OFFLINE` på almindelig recognizer er ikke en garanti for offline |

Dansk understøttelse er ikke bevis for god dansk kvalitet. Ingen af ovenstående
kilder giver en sammenlignelig måling på vores talesæt og telefoner. Sammenlign
WER/CER, egennavne, tegnsætning og hallucination ved stilhed på samme optagelser.
Mål cold start separat fra stop-til-indsættelse, p50/p95, peak PSS, modeldownload,
termisk påvirkning og energi ved gentagne 15-sekunders ytringer. Mindst en
mellemklassetelefon og en nyere arm64-telefon, både stille og støjfyldt tale.

**Valg til v0:** whisper.cpp med multilingual `base`, kvantiseret kandidat efter
måling; `small` er sammenligningskandidat. Download sker kun efter brugerens
valg, ikke med i APK eller repo. Det matcher projektets eksisterende Whisper-
erfaring og gør dansk uafhængigt af OEM-tjenester. Det er et låst produktvalg,
men den konkrete model/hash og native-version låses først efter benchmark og
licenskontrol. Hvis kvalitets-/ressourcekrav ikke nås, kræves en SPEC-ændring;
vi skifter ikke lyd til cloud automatisk.

## Cloud og Tingskrivers faktiske udbydere

Kilde i dette repo: [arkitektur](../arkitektur.md#ai-providers-one-model-a-chain-per-function),
[model](../../crates/tingskriver-core/src/providers/model.rs),
[client](../../crates/tingskriver-core/src/providers/client.rs),
[chain](../../crates/tingskriver-core/src/providers/chain.rs).

| Udbyder | Hvad der faktisk findes | Android-vurdering |
| --- | --- | --- |
| Syv.ai | Preset, STT og chat, standardkæde for diktering med 2 s remote-timeout før lokal fallback | Dansk-fokuseret kandidat, men 2 s er en timeoutpolitik, ikke målt svartid. Kræver brugerens nøgle, samtykke og netværk |
| OpenAI | Preset med OpenAI-kompatible kald | Multilingual STT-kandidat. Mobil WER, netværkslatency, retention og pris skal undersøges før aktivering; ingen kald udført |
| OdinCore | Preset, `ordbogen/whisper` og `odin-2` | Dansk-fokuseret alternativ; modellen siger ikke noget om målt Android-kvalitet. [Officiel produktbeskrivelse](https://www.ordbogen.ai/) |
| Custom OpenAI-compatible | Bl.a. Groq, OpenRouter eller selvhostet server som konfiguration | STT kræver at endpoint/modellen faktisk har audio-transcription capability. Chat-kompatibilitet er ikke STT-understøttelse |
| Ollama / Local | Ollama er tekstmodelserver; Local vælger indbyggede lyd-/tekstmodeller | Ollama er ikke i sig selv en STT-udbyder. Desktop Local kan ikke flyttes direkte til Android |

Claude, Deepgram og ElevenLabs er ikke længere aktive presets i den nuværende
udbydermodel; migreringen bevarer deres gamle nøgler. Ældre README-omtaler må
ikke bruges som featureliste. Cloud mindsker lokal model-RAM og beregning, men
radio/upload bruger stadig strøm og giver netafhængighed. Intet sammenligneligt
dansk mobilbenchmark blev fundet eller gennemført. Cloud er uden for v0.

## Genbrug fra Rust

| Kilde | Genbrug og grænse |
| --- | --- |
| `dictation/logic/cleanup.rs` | Ren `clean(input, style, rules)`; talte tegn, fyldord, mellemrum, store begyndelsesbogstaver, replacements/snippets. `prompt_words` giver modelhint. God kandidat til en lille delt crate |
| `dictation/logic/app_style.rs` | Regelvalg kan genbruges som idé/pure logic; Windows-processer skal erstattes af Android package-id/EditorInfo. Ingen læsning af omgivende tekst nødvendig |
| `dictation/logic/engine.rs` | Udbyder-/fallbackvalg og poleringsbudget er rene beslutninger, men Androids lokale engine har anden livscyklus |
| `dictation/store.rs` | Ordbogsformat og fixtures kan inspirere; SQLx-store gemmer også historik, hvilket strider mod Android-v0 uden transskripthistorik |
| `providers/{model,chain,client,secrets}.rs` | Udbydertyper, capabilities, fejlkategorier og kædepolitik kan deles senere. HTTP/runtime, persistence og Windows Credential Manager skal ikke bindes direkte til Kotlin |
| `dictation/service.rs`, `capture.rs`, `win.rs` | Desktoporkestrering, cpal, clipboard/SendInput og historie genbruges ikke. Android ejer mikrofon, lifecycle og `InputConnection` |

UniFFI kan generere Kotlin/JNI omkring en lille, platformuafhængig tekst-crate
med `clean_text` og records til regler. Direkte håndskrevet JNI er muligt, men
øger ansvaret for strenge, ejerskab, tråde og fejl. Hele `tingskriver-core` er
ikke Android-klar: dens manifest omfatter cpal, ffmpeg-sidecar, SQLx og ONNX,
og Whisper er kun deklareret til desktop-OS'er. Ingen JNI/UniFFI-afhængighed
tilføjes nu. Native STT og en tekstbro er to særskilte fremtidige integrationer.

## Byggeværktøjer og afhængighedsvurdering

Kontrolleret mod [Google Maven metadata](https://dl.google.com/android/maven2/com/android/tools/build/gradle/maven-metadata.xml),
[AGP 9.5-noter](https://developer.android.com/build/releases/agp-9-5-0-release-notes),
[Kotlin releases](https://kotlinlang.org/docs/releases.html),
[Kotlin kompatibilitet](https://kotlinlang.org/docs/gradle-configure-project.html)
og [AGP built-in Kotlin](https://developer.android.com/build/migrate-to-built-in-kotlin).

AGP 9.5.0 er seneste stabile (9.6-alpha fravalgt); Kotlin 2.4.21 er seneste
stabile, udgivet 8. oktober. AGP kræver mindst Gradle 9.6.0 og JDK 17.
Projektet bruger Gradle 9.6.0 og AGP's built-in Kotlin med eksplicit KGP 2.4.21
på build-classpath; `org.jetbrains.kotlin.android` anvendes ikke. Kotlin-tabellens
fuldt testede AGP-interval slutter ved 9.3.1; kombinationen med nyere stabile
AGP skal derfor bevises af Android-CI, ikke antages testet af JetBrains.

Google/JetBrains/Gradle er mangeårige upstream-vedligeholdere. Artefakterne er
ældre end tre døgn på undersøgelsesdatoen. Ingen fremmed repo er klonet.
Wrapperens distribution og JAR kontrolleres med Gradles officielle SHA-256;
`--version` kan hente distributionen uden Android SDK. Gradle-plugins er eksekverbar buildkode,
kan læse buildmiljøet og henter transitive afhængigheder over netværket; deres
fulde antal er ikke verificeret uden resolution. Runtime har kun Kotlin stdlib;
JVM-tests bruger JUnit 4.13.2 (JUnit-teamet, udgivet 2021) og Hamcrest 1.3
transitivt. Ingen AndroidX/Compose, SDK'er til analyse eller STT-pakker endnu.
Konklusion: **installér** de fastlåste officielle buildartefakter i CI uden
projektsecrets; ingen automatisk installation af Android SDK eller JDK lokalt.

## Resterende evidens

Der findes ikke `graphify-out/` i dette checkout; analysen bygger på de navngivne
kildefiler. En graf kan tilføjes ved en senere bred core-udtrækning. Wiki-værktøj
var ikke tilgængeligt; ingen intern projektviden er nødvendig eller publiceret.
Android SDK findes ikke i standardinstallationerne her, og lokal Java er 11.
APK, lint og Gradle-enhedstests kræver derfor den nye CI eller en udviklermaskine
med værktøjerne fra SPEC. Ingen fysisk IME-/mikrofonprøve er udført.
