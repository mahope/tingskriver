# Release-checkliste

Denne liste gælder stabile udgivelser. Forudgivelser (`x.y.z-dev.N`) udgives automatisk hver 8. time, når `main` har ændringer og grøn CI, og går uden om den manuelle ydeevnemåling; se [auto-release.md](auto-release.md). Stabile versioner udgives aldrig automatisk.

## Ydeevne før hver release

Mads ejer referencemaskinen, godkender baseline og tager den manuelle måling.
Den, der forbereder releasen, kører porten og vedhæfter rapporten. Første release
må vente, indtil referencemålingen og baseline er udfyldt. En grøn script-test
eller en grøn CI-kørsel erstatter ikke dette trin.

- [ ] Byg/installér release-kandidaten på referencemaskinen. Luk appen, også i tray,
  og kontrollér, at ingen optagelse er i gang. Følg [målemetoden](ydeevne.md),
  herunder dikteringerne og de særskilte målinger, som scriptet ikke kan automatisere.
- [ ] Gem den almindelige kørsel som UTF-8, fx i PowerShell:

  ```powershell
  powershell -ExecutionPolicy Bypass -File scripts\maal-ydeevne.ps1 | Out-File -Encoding utf8 maaling.md
  cmd /c "python scripts/ydeevne-port.py maaling.md > ydeevne-rapport.md"
  if ($LASTEXITCODE -ne 0) { throw 'Ydeevne-porten stoppede releasen' }
  ```

`cmd /c` bevarer Python-rapportens UTF-8-bytes ved omdirigering, også i
PowerShell 5.1.

- [ ] Kontrollér, at forventede målinger faktisk er med. `null` eller manglende
  nøgler står som "ikke målt" og kan ikke bevise, at en måling overholder grænsen.
  Kør eventuelle delmålinger separat og vedhæft også deres rapporter.
- [ ] Vedhæft måling og portens markdown-rapport til release-grundlaget med dato,
  appversion og maskinbeskrivelse. Porten accepterer JSON direkte eller den fulde
  markdown-fil fra målescriptet. Del kun tekniske målinger.
- [ ] Mads godkender reference-resultatet før udgivelse. Mere end 20 % forværring
  af blot én fælles måling stopper releasen. Præcis 20 % accepteres.

## Kanalisolation før hver release

Pilottone-testen (#189) finder mute- og routingfejl mellem mikrofon- og systemsporet,
som et lydniveau ikke ser. Tærsklerne står kun i
[`kanalisolation-graenser.json`](kanalisolation-graenser.json); værktøjet læser filen,
og checklisten henviser til den i stedet for at gentage tallene.

- [ ] Optag på Mads' pc med en pilottone på mikrofonen (523 Hz) og en på systemet
  (997 Hz): først begge i 10 sekunder, så kun mikrofonen i 10 sekunder, så kun
  systemet i 10 sekunder. Brug høretelefoner eller en loopback-kabel, så højttalerne
  ikke selv lækker lyd ind i mikrofonen. Kun syntetisk lyd, aldrig rigtige stemmer.
  Efter en normal optagelse findes sporene kun som `microphone.m4a` og `system.m4a`
  (de rå WAV-filer slettes, når optagelsen er verificeret), og værktøjet læser kun
  WAV. Konvertér først begge spor uden at ændre optagelsen:

  ```powershell
  ffmpeg -i microphone.m4a -ar 48000 -ac 1 microphone.wav
  ffmpeg -i system.m4a -ar 48000 -ac 1 system.wav
  ```

  Kør værktøjet på de to WAV-spor og vedhæft rapporten (exit 0 = bestået):

  ```powershell
  cargo run --release --locked -p tingskriver-core --example kanalisolation -- `
    microphone.wav system.wav docs\kanalisolation-graenser.json
  ```

  Uden fasetider finder værktøjet piloternes start og skift med Goertzel i 100 ms
  vinduer. Stilhed før og efter er tilladt, så tonerne behøver ikke starte ved
  optagelsens start. Der kræves rækkefølgen begge, mikrofon, system; ved manglende
  eller afbrudte faser fejler det. Faste tider kan stadig angives som
  `both:1:11 mic:11:21 system:21:31` (og `microphone` er alias for `mic`).

  Exit 1 betyder lækage, manglende pilot (kanal mutet eller forkert routet), klipning
  eller for stor forskel i varighed. Exit 2 betyder ubrugeligt input. Releasen må ikke
  fortsætte ved exit 1 eller 2.
- [ ] Den automatiske del kører på hver push i CI (`recording_soak isolation` på den
  syntetiske enhed) og i den natlige soak. Den erstatter ikke optagelsen fra Mads' pc.

## Privatlivspolitik før hver release

[PRIVACY_POLICY.md](../PRIVACY_POLICY.md) skal beskrive, hvad koden faktisk gør.

- [ ] `cargo test -p tingskriver-core --test privacy_policy --test log_privacy_guard`
  er grøn. Den første fejler, hvis koden indeholder en ekstern vært, som ikke står i
  tabellen; den anden, hvis et logkald udskriver en titel, et deltagernavn, en sti
  eller en adresse.
- [ ] Nye funktioner, udbydere, downloadkilder eller opdateringskilder står i
  tabellen med funktion, hvad der forlader maskinen, til hvem og hvordan det slås fra.
  Ændringer uden en adresse i koden (fx en ny type SSH-mål eller webhook) tjekkes
  ved gennemlæsning, for testen ser kun adresser i koden.
- [ ] Datoen øverst i politikken er opdateret, hvis tabellen er ændret.

## Første baseline og fejl

`docs/ydeevne-baseline.json` har indtil første måling `status: "ikke_maalt"` og
et tomt `resultater`-objekt. Mads erstatter filen med JSON-blokken fra den godkendte
referencemåling (bevar `maskine`, `repetitioner` og `resultater`; fjern status eller
sæt den til `maalt`). Indsæt også resultaterne i de eksisterende resultatafsnit i
`docs/ydeevne.md`.

Kør `python scripts/ydeevne-port.py --skriv-readme` og commit baseline,
resultater og README sammen med en begrundelse. Scriptet henter budgetterne fra
JSON-blokken i `docs/ydeevne.md`; README må ikke vedligeholdes med kopierede tal.

Ved exit 1: undersøg regressionen, ret den og mål igen. Hvis ændringen bevidst
accepteres, skal Mads godkende en ny baseline i **en egen commit med begrundelse**,
ikke blot hæve grænsen. Ved exit 2: ret input eller mål baseline; releasen må ikke
fortsætte. `--tillad-uden-baseline` er kun til valgfrie push-sammenligninger og må
aldrig bruges i release-gaten. Ugyldig JSON og ugyldige tal fejler stadig med 2.

## Tilkobling til release-workflowet

`app-release.yml` kræver nu en grøn ydeevneport før kladden publiceres.
Commit den godkendte referencemåling som
`docs/changelog/<version>.performance.json` før tagget oprettes. Jobbet kører
release-profilen mod den committede baseline uden undtagelse for manglende data.
Forudgivelser bevarer undtagelsen fra den manuelle måling.
Se [CI-dokumentationen](ci.md#gpui-release-gates-243-188) for alle porte.

CI-producenter kan kalde samme workflow med `profil: ci` og en separat,
committet baseline via inputtet `baseline`. Baseline skal måles på samme
runner-type med samme lokale model, inputlyd og konfiguration. Der er endnu ingen
måleproducent på Linux i dette workflow. Push/PR kører tests og README-tjek;
ingen faktiske app-målinger eller baseline-tal er opfundet.

## Målinger og tærskler for pilottoner

Alle fem grænser fra issue #189 bruges uden afvigende tal:

| JSON-grænse | Måling |
| --- | --- |
| `mic_isolation_db_min` (12 dB) | Begge: mikrofonpilot minus systempilot i mikrofonsporet |
| `speaker_isolation_db_min` (15 dB) | Begge: systempilot minus mikrofonpilot i systemsporet |
| `system_tone_isolation_db_min` (18 dB) | Kun system: systempilot i systemsporet minus samme pilot i mikrofonsporet |
| `mic_pilot_isolation_db_min` (6 dB) | Kun mikrofon: mikrofonpilot i mikrofonsporet minus samme pilot i systemsporet |
| `cross_channel_isolation_db_min` (12 dB) | Begge enkeltpilotfaser: samme pilot i kildesporet minus i det andet spor |

Den fælles tværkanalgrænse kontrolleres separat: den skærper mikrofonfasens
6 dB til 12 dB og er redundant med systemfasens strengere 18 dB. Det er bevidst,
så både den specifikke grænse og den fælles routingregel fremgår af rapporten.
Det tidligere `system_only_db` er fjernet, ikke en ekstra opfundet grænse.

CI bruger soak-harnessens syntetiske enhed ved produktionsstiens capture-grænse,
ikke `fake_device`, som ikke kan bære toner. Mikrofonstien har 80 Hz højpas og
EBU R128 ved -23 LUFS; RNNoise er aktuelt deaktiveret i produktionsflaget.
Harnessen kræver særskilt mikrofonpilotens niveau på -19 ± 2 dBFS peak i begge
mikrofonaktive faser. En 523 Hz sinus ved -23 LUFS ligger omkring -19 dBFS peak
efter K-vægtning; tolerancen dækker indsvingsforløb og vindueseffekt. Filterændringer,
der flytter niveauet uden for intervallet, giver en konkret niveaumeddelelse.
