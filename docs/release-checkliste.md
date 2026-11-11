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
