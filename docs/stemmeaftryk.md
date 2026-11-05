# Stemmeaftryk på tværs af møder (issue #198)

Tingskriver kan huske, hvem der er hvem fra møde til møde, så en deltager, der går igen, kan foreslås med navn. Et stemmeaftryk er biometri efter GDPR artikel 9, så funktionen er bygget efter Mads' beslutning af 5/11-2026:

1. **Kun lokalt.** Ingen netværkskode, ingen synkronisering. Aftryk indgår ikke i eksport, aflevering, MCP, logfil, diagnoserapport eller sikkerhedskopi.
2. **Samtykke pr. person**, gemt og kan trækkes tilbage.
3. **Slet-knap og automatisk sletning.**
4. **Forklaring på dansk og engelsk** på samme side.
5. **Slået fra som standard.**

Koden ligger i `crates/tingskriver-core/src/voiceprint/`.

## Hvad der gemmes hvor

| Hvad | Hvor | Indhold |
|---|---|---|
| Aftrykket (embedding) | Windows Legitimationsstyring, mål `Tingskriver/voiceprint/<id>` | tallene, som base64 |
| Indeks over aftryk | tabellen `voiceprint_exemplars` | id, samtykke-id, datoer. Ingen tal |
| Samtykke | `voiceprint_consents` + `voiceprint_consent_log` | visningsnavn, tidspunkt, metode (`confirmed_in_app`), valgfri note, tekstversion, status, tilbagetrækningstidspunkt |
| Indstillinger | `voiceprint_settings` | til/fra (standard fra), 30/90/180 dage (standard 90), seneste sletning |

Afinstallation med "slet data" fjerner allerede alle Credential Manager-poster under `Tingskriver/` (`install/user_data.rs`), og aftrykkene ligger under det præfiks.

## Regler i koden

- Intet aftryk gemmes uden aktivt samtykke (`enroll` kontrollerer det). Slået fra gemmer og matcher intet.
- Tilbagetrækning markerer samtykket som trukket først, sletter derefter aftrykkene og skriver en post i samtykkeloggen. Fejler en sletning, bliver indeksrækken stående, og et nyt kald prøver igen.
- "Glem denne person" sletter personens aftryk (samtykket består). "Glem alle" fjerner alle poster under præfikset i Credential Manager, også dem databasen ikke kender, og tømmer indekset.
- Automatisk sletning: aftryk, der ikke er bekræftet som match inden for indstillingen, slettes. Et aftryk, der aldrig er matchet, tæller fra den dag, det blev lavet. Kører ved opstart (`start_maintenance`) og mindst hver 24. time, også mens funktionen er slået fra. Loggen har kun antal.
- Højst tre aftryk pr. person; det ældste erstattes.
- **Navngivning sker aldrig automatisk.** `suggest` giver forslag; først `confirm_suggestion` (brugerens ét klik) sætter navnet og tæller som "matchet".
- Tærsklerne (score 0,62, margin 0,08, gensidigt bedste match) er anarlogs startværdier, målt på engelsk. De er markeret som **ikke kalibreret til dansk** (`THRESHOLDS_CALIBRATED_ON_DANISH = false`, og forklaringen siger det).

## Vagter (`tests/voiceprint_guards.rs`)

Testene læser kildeteksten og fejler, hvis: modulet får netværks- eller procesadgang, en aftrykstype kan serialiseres, en logsætning kan skrive navn, score eller tal, en fil uden for en kort tilladelsesliste nævner `voiceprint` (så ny eksport-, aflevering-, MCP- eller diagnosesti skal gennemgås), migreringen får en blob- eller embedding-kolonne eller ændrer standarderne, eller indstillingssiden mister den tosprogede forklaring. Hver scanner har en positiv kontrol.

## Hvad der mangler for en rigtig embedder

Repoet har ingen talerembeddingmodel. `SpeakerEmbedder` er traitten, og `StubEmbedder` er en tydeligt markeret teststub (otte energibins, genkender ingen). Den må ikke tilbydes brugere. For en rigtig embedder mangler:

1. En model (typisk ONNX, fx en ECAPA- eller WeSpeaker-model) med licens, der tillader udgivelse, og et pakketjek af modellen og dens runtime. `ort` findes allerede i kernen på Windows.
2. Rene enkelttalersekvenser at fodre den med: det kommer med lokal diarisering (#196, efter målingen i #184). anarlog bruger sammenlægningshul 400 ms, 1,5 til 10 s og højst tre pr. taler.
3. En måling på dansk lyd (fixtures med kendte talere) til at genkalibrere tærsklerne. Indtil da er genkalibrering **åben**, og forslag kræver altid et klik.

## Hvor brugerfladen kobles på

Indstillingssiden i GPUI-appen viser statuslinjen "slået fra" og forklaringen på dansk og engelsk. Der er endnu ingen kontakt til at slå funktionen til, ingen samtykkedialog og ingen liste over samtykker: privatlivssiden (#84) og talernavneforslag (#87) findes ikke på `main`. Når de kommer, kalder de `VoiceprintService` (`set_enabled`, `set_retention_days`, `grant_consent`, `consents`, `withdraw_consent`, `forget_person`, `forget_all`, `suggest`, `confirm_suggestion`) og viser `voiceprint::texts::explanation()`.

Kilde til idéen og startværdierne: anarlog (MIT, Fastrepl, Inc.). Der er ikke kopieret kode.
