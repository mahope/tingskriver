# Release-checkliste

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
    microphone.wav system.wav docs\kanalisolation-graenser.json `
    both:0:10 mic:10:20 system:20:30
  ```

  Exit 1 betyder lækage, manglende pilot (kanal mutet eller forkert routet), klipning
  eller for stor forskel i varighed. Exit 2 betyder ubrugeligt input. Releasen må ikke
  fortsætte ved exit 1 eller 2.
- [ ] Den automatiske del kører på hver push i CI (`recording_soak isolation` på den
  syntetiske enhed) og i den natlige soak. Den erstatter ikke optagelsen fra Mads' pc.

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

Det eksisterende release-workflow er ikke ændret af issue #61. Når release-arbejdet
fra #22 er klar, skal et målejob uploade målingsfilen med `actions/upload-artifact@v4`.
Et efterfølgende job kalder det genbrugelige workflow, og selve udgivelsesjobbet
skal have `needs: [performance]` uden `always()` eller `continue-on-error`:

```yaml
performance:
  needs: measurement
  uses: ./.github/workflows/ydeevne-port.yml
  with:
    artifact: reference-performance
    maaling: maaling.md
    profil: release
```

Artifactet skal komme fra samme workflow-kørsel. `maaling` er stien inde i
artifactet, ikke en fil, som forventes at overleve mellem jobs. Jobbet `gate`
henter artifactet, gemmer rapporten som artifact og i joboversigten og viderefører
exit 1 og 2 som fejl. En manglende fil fejler også. Upload kun tekniske målinger.
Et manuelt godkendelsestrin for referencemaskinen skal stadig stå før udgivelsen.

CI-producenter kan kalde samme workflow med `profil: ci` og en separat,
committet baseline via inputtet `baseline`. Baseline skal måles på samme
runner-type med samme lokale model, inputlyd og konfiguration. Der er endnu ingen
måleproducent på Linux i dette workflow. Push/PR kører tests og README-tjek;
ingen faktiske app-målinger eller baseline-tal er opfundet.
