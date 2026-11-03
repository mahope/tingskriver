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
