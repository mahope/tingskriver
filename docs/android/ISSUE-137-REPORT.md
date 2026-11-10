# Issue #137: sikker tekstindsættelse

Implementeret oven på main 157f6f5, som endnu ikke havde InsertionSession.
Et eksisterende ukommitteret udkast blev bevaret og tilpasset optagelsesflowet
fra #135; ingen ændringer af Rust-workspacet.

Sessionen fastholder generation og den oprindelige commit-funktion, som lukker
over InputConnection ved optagelsens start. Alle eksisterende afbrydelser
(feltskift, appskift via input-lifecycle, finish, skjult IME, skærmlås,
annuller og destruktion) invaliderer forbindelsen. Sessionen forbruges før
commitText, også ved afvisning og genindtræden. Tomme og fejlede resultater
ændrer ikke feltet. Manglende/afvist forbindelse giver dansk status.
Password-politikken er bevaret. Ingen clipboard, surrounding-text-læsning,
Accessibility eller Enter. Den aktiverede STT-stub producerer fortsat ingen tekst.
Rigtig STT skal senere køre på worker med completion på IME-hovedtråden.

## Verifikation

- Rød fase: de nye indsættelsestests kompileret uden den nye controller
  fejlede med unresolved reference InsertionSession/InsertionOutcome, som
  heller ikke findes i origin/main.
- Grøn fase: alle Androids rene Kotlin/JUnit-tests kompileret med eksisterende
  kotlinc og kørt med eksisterende JUnit 4.13.2/Hamcrest: OK (23 tests).
  Heraf ni indsættelsestests, inklusive forskellige forbindelser, generationer,
  annuller/ny session, gentagne og genindtrædende callbacks, manglende/afvist/
  kastende forbindelse, blanke resultater og password-/ikke-tekstfelter.
- Lifecycle-scenarier testes som controller-invalidering, ikke som Android-
  framework-integration. Service-wiring er gennemlæst, ikke enhedstestet.
- Forsøgt Gradle som Android-workflowet: ./gradlew --no-daemon
  :app:assembleDebug :app:testDebugUnitTest :app:lintDebug.
  Blokeret før build: Gradle requires JVM 17 or later; available JVM is 11.
  APK, Gradle-tests og lint er derfor ikke verificeret.
- adb devices: ingen tilsluttede enheder. Fysisk EditText/webformular, OEM-
  lifecycle og service-stop mangler; prøveplanen står i apps/android/README.md.
- git diff --check passerede. Ingen nye pakker installeret.

## Foreslået PR

Titel: Bind Android-indsættelse til oprindeligt felt og session

Beskrivelse: Fasthold oprindelig InputConnection ved sessionsstart og afvis sene
eller gentagne resultater efter input- og service-lifecycle. Bevar optagelsesflow
og password-blokering, og vis danske indsættelsesfejl. Tilføj regressionstests og
en fysisk prøveplan. 23 rene JVM-tests består; Gradle kræver JDK 17, og fysisk
EditText/webformular er endnu ikke prøvet. Fixes #137.

Ingen PR oprettet. Ikke klar til at erklære alle acceptkriterier opfyldt før
Gradle-gaten og den fysiske prøve er gennemført.

## Opfølgning på review 2026-11-03

Begge fund er vurderet som korrekte. Ingen P3-fund var angivet.

- P2: Tilføjet otte tests i InputMethodLifecycleTest, der kalder den faktiske
  services onStartInput (samme app, anden app og restarting), onFinishInput,
  onFinishInputView, onWindowHidden og onDestroy. Et gammelt syntetisk resultat
  afleveres derefter til servicens egen InsertionSession. En særskilt test
  kontrollerer gammel callback og vellykket ny session efter annullering.
  Testene supplerer de eksisterende controller-tests; produktionskode er uændret.
  Android-framework-metoder bruger Gradles default-returnerende JVM-stubs.
  Det er tests af servicekoblingen, ikke af platformens InputConnection.
- Målrettet Gradle-kørsel af InputMethodLifecycleTest blev forsøgt først, men
  stoppede før kompilering: "Gradle requires JVM 17 or later" (installeret: 11).
  Derfor er hverken rød/grøn-kørsel eller mutation af servicekoblingen udført;
  de otte nye tests er endnu ikke verificeret.
- Fuld Android-gate forsøgt til sidst med assembleDebug, testDebugUnitTest og
  lintDebug; samme JVM-blokering før build. Alle 23 eksisterende rene Kotlin/
  JUnit-tests kørt separat med kotlinc: OK (23 tests). Dette inkluderer ikke
  de nye service-tests. Ingen nye pakker installeret.
- P1 er ikke løst: adb devices viser fortsat ingen enheder. Fysisk EditText,
  webformular og service-stop kan ikke verificeres her. Den eksisterende
  fysiske prøveplan gælder fortsat, og grenen er ikke klar til merge på dette
  grundlag. Der påstås ingen fysisk eller fuld Gradle-verifikation.

Ingen PR oprettet. Commit og push omfatter kun testdækning og dokumentation.

## Fysisk tjekliste efter review af PR #152

- Prøv både EditText og browser/WebView med syntetiske slutresultater.
  Indsættelsen skal ske ved den aktuelle markør i forbindelsen fra optagestarten.
- Prøv apps og WebViews, der ofte kalder `restartInput`: det invaliderer den
  aktive session og kan afbryde diktering. Et efterfølgende resultat må ikke
  indsættes, heller ikke hvis forbindelsen genbruges.
- Kontrollér både automatisk stop ved 30 sekunder og fuld buffer samt manuelt
  stop, annuller, feltskift, appskift, skjult IME, skærmlås og service-stop.
- Fysiske prøver er fortsat ikke udført på worker-miljøet. STT-stubben indsætter
  fortsat ingen tekst; rigtig STT er ikke tilsluttet.

## Rettelser og verifikation af PR #152, 2026-11-10

- Automatisk STOPPED ved tidsgrænse eller fuld buffer går nu gennem samme
  afslutningssti som manuelt stop. ERROR og CANCELLED invaliderer indsættelsen.
  Livscyklus- og tilladelseskontroller kan fortsat annullere en optagelse.
- Den lille DictationController bruges af knap-handleren og fastholder
  forbindelsen fra optagestarten. Fem nye tests dækker forbindelsesskift,
  oprydning efter startfejl og vellykket genstart, manglende forbindelse,
  blokerede felter, tilladelsesafslag, begge automatiske stop og fejl/annullering.
- README beskriver nu den aktuelle markør i den oprindelige forbindelse.
  insertionStatus læses via en lokal val. Den gentagne controller-test bruger
  nu faktisk samme commit-funktion, mens servicens felt-/apptests allerede
  bruger forskellige fieldId/packageName.
- Mutationstest: STOPPED blev midlertidigt ændret tilbage til invalidering.
  Regressionstesten fejlede med "expected:<1> but was:<0>". Rettelsen blev
  derefter gendannet før den fulde kørsel.
- Kørt fra apps/android med lokal JAVA_HOME og ANDROID_HOME:
  `./gradlew --no-daemon testDebugUnitTest lint`.
  Resultat: 36 tests, 0 fejl, 0 udeladte. Gradle: `BUILD SUCCESSFUL in 28s`;
  opgavelinje: `34 actionable tasks: 13 executed, 21 up-to-date`.
  Sidste outputlinje: `Consider enabling configuration cache to speed up this build: https://docs.gradle.org/9.6.0/userguide/configuration_cache_enabling.html`.
  Lint: ingen fejl, seks advarsler i uændret opsætning/kode: OldTargetApi,
  InflateParams, AndroidGradlePluginVersion, GradleDependency,
  DataExtractionRules og MissingApplicationIcon.
- Den tidligere JDK-blokering er løst i jobbet med Eclipse Adoptium Temurin
  21.0.13+9 og Googles officielle Android-SDK. Arkiv-checksummer er verificeret.
  Temurin er et etableret Eclipse-projekt; SDK-værktøjerne vedligeholdes af Google.
  De valgte JDK-/CLI-udgivelser er ældre end tre døgn. Arkiverne pakkes ud uden
  installationsscripts. SDK-manageren læser SDK/JDK-stier og henter officielle
  SDK-komponenter via netværket. Ingen nye projektafhængigheder er tilføjet.
  Konklusion: installér lokalt til jobbet.
- Ingen fund afvist. Fysiske prøver mangler fortsat; JVM-tests certificerer
  hverken OEM-livscyklus eller rigtig STT.

PR-titel: Complete automatic Android dictation stops safely

PR-beskrivelse: Route elapsed-time and full-buffer stops through completion,
capture the original input connection in a tested controller, and clear failed
starts. Correct cursor documentation and document restartInput cancellation.
Android JVM tests (36) and lint pass. Physical-device checks remain pending.
Fixes #137.
