# Ydeevne

Baseline og budgetter for Tingskriver på Windows. Tallene måles med `scripts/maal-ydeevne.ps1`, så den nuværende Tauri-app og den senere GPUI-app kan sammenlignes på samme måde (issue #3).

## Kør målingen

Krav: Windows 11 med PowerShell 5.1 (ingen andre værktøjer). Appen skal være installeret i `%LOCALAPPDATA%\Tingskriver\tingskriver.exe` og være lukket (også i tray). Ingen optagelse må være i gang.

```powershell
powershell -ExecutionPolicy Bypass -File scripts\maal-ydeevne.ps1
```

Nyttige parametre:

| Parameter | Betydning | Standard |
|---|---|---|
| `-Repetitioner N` | antal gentagelser, medianen bruges | 5 |
| `-EfterMinutter N` | mål hukommelsen igen N minutter efter tomgangsmålingen (fx 60 for en time). Scriptet skjuler vinduet i tray før ventetiden, så målingen er en tray-måling | 0 (fra) |
| `-Dikteringer N` | antal seneste dikteringer i p50/p95 | 50 |
| `-TomgangSekunder N` | ventetid før hukommelsesmåling | 60 |
| `-ExeSti` | anden sti til appen | `%LOCALAPPDATA%\Tingskriver\tingskriver.exe` |
| `-KunDiktering` | læs kun dikteringstid, start ikke appen | |
| `-KunHukommelse` | mål hukommelse og CPU på en app der allerede kører (fx efter et møde); rører ikke appen | |
| `-KunKoldstart -KoldLog fil.txt` | én koldstart pr. kørsel; medianen over filens linjer vises | |

`efter_ws_total_mb` er kun med, når `-EfterMinutter 60` er brugt.

Med `-EfterMinutter 60` og 5 gentagelser tager en kørsel over 5 timer. Brug i stedet `-Repetitioner 1 -EfterMinutter 60` til tray-målingen og de normale fem gentagelser til resten.

Scriptet skriver en markdown-tabel og en JSON-blok. Sæt tabellen ind under "Resultater" nedenfor. Scriptet ændrer intet: det starter og lukker kun appen (`CloseMainWindow`, derefter `Stop-Process` efter 10 s), læser logfilerne og læser en midlertidig kopi af `dictation.sqlite`. Eneste fil det kan skrive er `-KoldLog`, hvis du angiver den.

Appen skjuler sig i tray ved `CloseMainWindow` og afslutter ikke selv, så lukningen ender normalt i `Stop-Process` på appen og dens underprocesser. Varmstart måles derfor efter et kill, ikke efter et pænt afslut.

Scriptet stopper uden at røre appen, hvis:

- `tingskriver.exe` allerede kører (der kan være en optagelse i gang)
- filer i optagelsesmappen er ændret de seneste 2 minutter: standardmappen `%USERPROFILE%\Music\tingskriver-recordings` og den mappe brugeren har valgt (`save_folder` i `%APPDATA%\dk.mahoje.tingskriver\recording_preferences.json`, læses kun)

## Metode

| Måling | Sådan måles den |
|---|---|
| Koldstart | Tid fra `Start-Process` til hovedvinduet har et `MainWindowHandle` og er synligt (`IsWindowVisible`), poll hver 10. ms. |
| Varmstart | Samme måling ved start nr. 2 og frem i kørslen, efter at appen er lukket. |
| Hukommelse | Working set og private bytes for `tingskriver.exe` og alle efterkommere i procestræet (`msedgewebview2.exe`, `llama-helper.exe`, `ffmpeg.exe` m.fl.), målt 60 s efter vinduet blev synligt. Gruppesummer: app, webview2, øvrige. |
| CPU i tomgang | Samlet processortid for procestræet over de sidste 10 s af ventetiden, delt med antal logiske kerner. |
| Dikteringstid | Fra linjen `Dictation: N ms from release to text` i `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\tingskriver.log*` (tiden fra slip af genvejen til teksten er indsat). Fejlede indsættelser (`method none`) tælles ikke med. Samme tal læses fra `latency_ms` i `dictation_history` (`dictation.sqlite` i `%APPDATA%\dk.mahoje.tingskriver`), hvis `sqlite3.exe` findes på PATH. p50/p95 er nearest-rank over de seneste N. Der rapporteres også kun for optagelser på 10-20 s, som er målet "15 s tale". Der printes aldrig tekstindhold. |
| Dikteringstid pr. trin | Fra tabellen `dictation_timings` i `dictation.sqlite` (kræver `sqlite3.exe`): pr. udbyder (`syv`, `local`) p50/p95 for hele tiden fra slip til indsat og for hvert trin: tastetryk til optagelse (`start_ms`), slip til lyd klar (`capture_ms`), transskription inkl. fallback (`transcribe_ms`), polering (`polish_ms`) og indsættelse (`insert_ms`). Tabellen har kun tal og udbydernavn, ingen tekst, og fyldes også, når historikken er slået fra. Appen viser de samme tal (median og p95 over de 50 seneste pr. udbyder) og udsender `dictation-latency-warning`, når en udbyders median er over 1.500 ms (mindst 5 dikteringer). |
| Gentagelser | Hver måling gentages `-Repetitioner` gange; medianen rapporteres. |

### Hvad scriptet ikke kan måle

- **Ægte koldstart.** Windows' filcache er varm efter første start. Første start i en kørsel er en koldstart-proxy. Et rigtigt koldt tal kræver `-KunKoldstart -KoldLog` lige efter en genstart, gentaget fem gange.
- **Hukommelse efter et 30 min møde.** Kræver et rigtigt møde. Optag selv, og kør `scripts\maal-ydeevne.ps1 -KunHukommelse` lige efter, mens appen stadig kører. Det starter, lukker og ændrer ikke noget.
- **Optagelse af to spor** (budget 400 MB): kræver instrumentering i appen, som ikke findes endnu. **Tid fra tastetryk til optagelse** (budget 150 ms) står i `dictation_timings.start_ms` fra 0.5.1.
- **Dikteringstid kræver dikteringer.** Diktér mindst 20 gange, helst 10 af dem på omkring 15 s, før scriptet køres.
- **Vindue der starter skjult i tray** bliver aldrig synligt. Scriptet melder det og tæller tiden ikke med.

## Budgetter (foreslået)

Ikke kildebelagte; ejeren godkender efter første måling. Maskinlæsbart, til en senere automatisk sammenligning med JSON-blokken fra scriptet (nøglerne under `budgetter` er de samme som under `resultater` i scriptets JSON; `ikke_maalt` har ingen nøgle i scriptet endnu):

```json
{
  "status": "foreslaaet",
  "budgetter": {
    "efter_ws_total_mb": 200,
    "tomgang_cpu_pct": 1,
    "diktering_log_p50_15s_ms": 1500
  },
  "ikke_maalt": {
    "optagelse_to_spor_ws_total_mb": 400,
    "tastetryk_til_optagelse_ms": 150
  }
}
```

## Resultater

Medianer af fem kørsler. Udfyldes af Mads på referencemaskinen (kræver 0.5.0-dev.2 installeret).

### Maskine

| Felt | Værdi |
|---|---|
| Dato | |
| CPU | |
| Kerner | |
| RAM | |
| Windows | |
| WebView2 | |
| Appversion | |

### Målinger

| Måling | Median | Enhed | Budget | Note |
|---|---|---|---|---|
| Koldstart | | ms | | |
| Varmstart | | ms | | |
| Tomgang, working set i alt | | MB | | |
| Tomgang, private bytes i alt | | MB | | |
| - tingskriver.exe, working set | | MB | | |
| - msedgewebview2.exe, working set | | MB | | |
| - øvrige underprocesser, working set | | MB | | |
| Tomgang, CPU | | % | 1 | |
| Efter 60 min i tray, working set i alt | | MB | 200 | |
| Efter 30 min møde, working set i alt | | MB | | |
| Optagelse af to spor, working set i alt | | MB | 400 | |
| Tastetryk til optagelse | | ms | 150 | |
| Diktering p50, 10-20 s tale | | ms | 1500 | |
| Diktering p95, 10-20 s tale | | ms | | |

### Historik

Én blok pr. måling, nyeste øverst, med dato og appversion i overskriften.
