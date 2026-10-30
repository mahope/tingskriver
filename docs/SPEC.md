# Tingskriver: samlet plan for møder, diktering og assistent

> Afløser scope-afsnittene i `2026-10-29-meetily-fork.md`. Projektformen derfra gælder stadig: gratis open source (MIT), Windows først, intet Mahope-specifikt hardkodet, ingen Pro-kode, ingen betalte tjenester ud over dem brugeren selv slår til.

## Formål
Mads vil have ét program, der bliver hans foretrukne til alt tale på PC'en:
1. **Møder** som Granola: optag, skriv egne noter undervejs, få et dansk referat, der bygger videre på noterne.
2. **Diktering** som Wispr Flow: hold en tast, tal, og teksten står renskrevet i det program, markøren står i.
3. **Assistent** som HeyEmma: spørg med stemmen, få svar med kendskab til det, der står på skærmen, og til dine egne møder.

Det skal kunne bruges af andre danskere uden Mahopes servere: alt eksternt (Syv.ai, Ollama, aflevering) er valgfrit og konfigureres i indstillinger. Uden konfiguration kører alt lokalt.

## Status pr. 30/10-26
- v0.5.0-dev.2 er ude: fuld omdøbning, nyt design, logfil, hurtigere enhedsskift.
- I gang: `fix/mute-ingen-skift`. Stilhed (mute) må aldrig udløse skift; kun reel fejl på enheden.

## Bærende principper
- **Optagelsen må aldrig gå tabt.** Lyden skrives løbende til disk (to spor), så et nedbrud koster højst få sekunder. Alt efterbehandling kan køres igen fra lydfilerne.
- **Mute er ikke en fejl.** Enhedsskift sker kun ved fjernet/ugyldig enhed, Windows-default-skift eller manglende buffere. Aldrig på grund af lydindhold.
- **Data bliver hos brugeren.** Lokal SQLite + filer. Eksterne kald kun til tjenester brugeren selv har sat op. Ingen telemetri. For Mads: mødedata og diktering går kun til Syv.ai og den lokale model, aldrig Claude/OpenAI.
- **Dansk først.** UI, standardsprog `da`, skabeloner, datoformater, talte kommandoer.
- **Rolig UI.** Noterne er i fokus; status vises diskret; aldrig modale dialoger midt i et møde.

## Scope (hele planen)

### A. Møder (Granola-niveau)
A1. **To lydspor** (mikrofon og system) gemmes hver for sig plus mixet spor til afspilning. Løbende skrivning (segmenterede filer eller fragmenteret mp4), genopretning efter nedbrud ved næste start.
A2. **Egne noter under mødet:** en notesblok ved siden af/oven på optagelsen (BlockNote findes). Tidsstempler gemmes pr. afsnit, så noterne kan kobles til transskriptet.
A3. **Transskription efter mødet:**
   - Syv.ai (valgfri nøgle i Windows Credential Manager): mikrofonspor som "Mig" (navn fra indstillinger, Mads), systemspor med `diarize`. Segmenter flettes efter tid.
   - Uden nøgle: lokal Parakeet/Whisper som i dag, med sprog `da` (aldrig `auto-translate` som standard).
   - Kø og genforsøg: fejler et kald, prøves igen med backoff; brugeren kan altid køre transskriptionen igen.
A4. **Talernavne:** LLM foreslår navne ud fra samtalen og kalender-deltagere (A8). Brugeren bekræfter eller retter med ét klik pr. taler; navne huskes pr. stemme-id i mødet.
A5. **Referat ("forbedrede noter"):** LLM fletter brugerens egne noter med transskriptet til et dansk referat: beslutninger, opgaver (hvem/hvad/frist), åbne spørgsmål, kort resumé. Egne noter vises i sort, AI-tilføjelser i dæmpet farve (Granola-mønstret). Skabeloner: standardmøde, kundemøde, 1:1, intern statusmøde; brugeren kan lave egne.
A6. **Spørg mødet:** chatfelt på mødesiden ("hvad sagde hun om prisen?") med svar og tidskoder, der kan afspilles.
A7. **Søgning på tværs af møder:** fuldtekst (SQLite FTS5) over titler, noter, referater og transskripter; resultater med uddrag og spring til tidskode.
A8. **Kalender (valgfri):** privat ICS-adresse (Google/Outlook, ingen OAuth). Giver mødetitel, deltagere og forventet slut; bruges til titel, talernavne og automatisk stop.
A9. **Automatisk mødegenkendelse:** Teams-app, Teams i browser, Google Meet i browser og som app, Zoom i browser og app. Signaler: hvilken proces bruger mikrofonen (`CapabilityAccessManager\ConsentStore`), vinduestitler, kendte procesnavne og kalender. Indstilling pr. kilde: "optag automatisk" / "spørg med notifikation" / "fra". Standard for Mads: optag automatisk. Stop når mikrofonbrug og mødevindue er væk i 30 s, eller kalenderens slut + 5 min uden tale.
A10. **Indikator:** lille flydende pille (altid øverst, kan flyttes) med optagestatus, varighed, aktiv mikrofon, lydniveau og knapper til pause og stop. Tray-ikon skifter farve under optagelse.
A11. **Aflevering (valgfri):** efter godkendt referat sendes transskript, referat, noter og sidecar (`kilde: "tingskriver"`, titel, tid, talere, opgaver) til et mål: mappe, webhook eller SSH-mål. Mads' opsætning: SSH med låst nøgle til ingest på netcup (wiki får referatet, HQ får opgaverne). Ingest i mahope-automation udvides til at tage færdigt transskript og springe Syv.ai over.
A12. **Eksport og deling:** kopiér referat som markdown/HTML til mail, gem som .md/.docx, kopiér link til lydfil.

### B. Diktering (Wispr-niveau)
B1. **Global genvej:** hold `Ctrl+Win` for at tale, slip for at indsætte. Dobbelttryk låser (håndfri) indtil næste tryk. Genvejen kan ændres. Virker i alle programmer, også når Tingskriver er skjult. En genvej kun af modifiers kan ikke registreres med `RegisterHotKey`, så der bruges én `WH_KEYBOARD_LL`-hook (i egen tråd, kun aktiv for de konfigurerede genveje), som sluger Win-tastens key-up, så startmenuen ikke åbner. Hooken ser kun modifier-tilstand og genvejstaster; der logges aldrig tastetryk.
B2. **Pille-overlay** nederst på skærmen med lydniveau, mens der optages; viser "Skriver..." under behandling. Ingen fokus-tyveri.
B3. **Transskription:** Syv.ai (hurtigt endpoint, `da`) hvis nøgle; ellers lokal Parakeet v3 (understøtter dansk). Mål: under 1,5 s fra slip til tekst for 15 s tale med Syv.ai.
B4. **Renskrivning i to lag:**
   - Altid (deterministisk, < 50 ms): fjern "øh/øhm/altså" ved start, talte tegn ("punktum", "komma", "spørgsmålstegn", "ny linje", "nyt afsnit", "kolon"), stort begyndelsesbogstav, ordbog og erstatninger.
   - Valgfrit (LLM-polering): grammatik, selvrettelser ("nej, jeg mener torsdag"), lister. Kun hvis en hurtig model er sat op; aldrig blokerende ud over 2 s, ellers indsættes lag-1-teksten.
B5. **Indsættelse:** via udklipsholder + `Ctrl+V` med gendannelse af den oprindelige udklipsholder bagefter; fallback til `SendInput`-tastning for felter, der blokerer indsæt.
B6. **Ordbog:** egne ord og navne (fx "Mahoje", "Syv.ai", kundenavne) sendes som prompt/hotwords til transskriptionen og bruges til erstatning. Retter brugeren et indsat ord inden for 30 s i samme felt, foreslås det tilføjet (kun forslag, ingen tastelogning ud over feltet).
B7. **Snippets:** talt udløser → tekstblok ("min signatur", "mit CVR", "booking-link").
B8. **Stil pr. program:** formel i Outlook/Spark, kort i Slack/Discord/Teams-chat, rå i kodeeditorer og terminal (ingen polering, ingen stort bogstav).
B9. **Kommandotilstand:** markér tekst, hold `Ctrl+Win+Alt`, sig en instruks ("gør den kortere", "oversæt til engelsk", "lav det til punktform") → teksten erstattes. Kræver LLM.
B10. **Historik:** de seneste 500 dikteringer med original lyd 24 t, tekst altid; kan kopieres igen. Kan slås fra.

### C. Assistent (HeyEmma-niveau)
C1. **Genvej:** `Ctrl+Win+Space` åbner et lille spørgevindue og lytter; slip/stilhed sender. Skrevet spørgsmål virker også.
C2. **Skærmkontekst:** valgfri "se skærmen": aktivt vindue fanges og læses med Windows' indbyggede OCR (`Windows.Media.Ocr`, dansk, lokalt, gratis). Teksten sendes til LLM, ikke billedet. Synligt øje-ikon når skærmen er med.
C3. **Kendskab til møder:** svar kan bruge egne møder via FTS-søgning (A7) som kontekst ("hvad lovede jeg Anna i går?"), med kilde-links til mødet.
C4. **Svar** streames i vinduet; knapper: kopiér, indsæt i aktivt program, læs højt (Windows' danske stemme via SAPI/`Windows.Media.SpeechSynthesis`, valgfrit).
C5. **Handlinger (begrænset):** "skriv et svar på denne mail" (skærm + diktering → udkast indsættes), "lav en opgave" (via aflevering A11), "start optagelse". Ingen autonome klik i andre programmer.
C6. **Wake word "Hej Tingskriver":** ikke i denne plan (se Ikke i scope).

### D. Fælles
D1. **AI-udbydere** (Mads 30/10: "gør det muligt at bruge OpenAI eller fx odincore.dk som andre modeller end Syv.ai"; tjenesten hedder OdinCore og ligger på odincore.ai, base `https://api.odincore.ai/v1`, med OpenAI-kompatibel transskription `ordbogen/whisper` uden diarisering og chat `odin-2`). Én fælles udbyder-model: navn, base-URL, API-nøgle (Credential Manager), model-id og type (Syv.ai, OpenAI, OdinCore, generisk OpenAI-kompatibel, lokal). Færdige forvalg for Syv.ai, OpenAI og OdinCore; alle andre via "brugerdefineret OpenAI-kompatibel". Hver funktion vælger udbyder og model: transskription (Syv.ai / lokal), referat og assistent (Ollama / OpenAI-kompatibelt endpoint / indbygget lokal model), diktering-polering (hurtig udbyder / fra). Test-knap pr. udbyder med målt svartid.
D2. **Mads' opsætning (besluttet 30/10):** referat, talernavne, spørg-mødet og assistent bruger den **indbyggede lokale model** (llama-helper) på PC'en. Ollama på netcup åbnes ikke for PC'en; Ollama/OpenAI-kompatibel udbyder findes kun som valgfri indstilling for andre brugere. Diktering bruger Syv.ai som standard med lokal Parakeet som reserve (automatisk, hvis Syv.ai fejler eller er over 2 s). Syv.ai-nøgle fra Bitwarden, aflevering via SSH.
D3. **Hurtigtaster** samlet under Indstillinger → Genveje, med konfliktvarsel.
D4. **Start med Windows** (minimeret til tray), et klik til "Ny optagelse", "Diktér", "Spørg".
D5. **Onboarding** i 3 trin: mikrofon og systemlyd, transskription (Syv.ai-nøgle eller lokal model), genveje. Alt andet har gode standarder.
D6. **Opdatering og installer** via Velopack med GitHub Releases som kilde (erstatter Tauri-updater og NSIS). Den sidste Tauri-release flytter eksisterende installationer over; installeren rydder rester efter tidligere binærnavne (`meetily.exe`).
D7. **Projektside** på `tingskriver.mahoje.dk`: én statisk side med skærmbilleder, download-link til seneste release, privatliv i klartekst.
D8. **Upstream:** enhedsskift-rettelsen sendes som PR til Zackriya-Solutions/meetily, når Mads har bekræftet den på rigtigt udstyr.

## Ikke i scope
- macOS og Linux (koden må ikke ødelægge upstreams stier, men vi tester dem ikke).
- Wake word / altid-lyttende mikrofon. Kommer måske senere med lokal model (openWakeWord) og tydelig indikator.
- Mødebot, der deltager i mødet; automatisk besked i mødechatten.
- Live-transskript under mødet (efter mødet er nok; noter skrives selv undervejs).
- Billedforståelse (vision-model); vi bruger OCR-tekst.
- Mobil-app, synkronisering mellem maskiner, deling med andre brugere.
- Betalte tjenester som standard; kodesignering ud over evt. SignPath Foundation.

## Data og integrationer
| System | Bruges til | Bemærkning |
|---|---|---|
| Syv.ai (`platform.syv.ai/v1`) | Transskription af møder og diktering | OpenAI-kompatibel, `diarize`, `da`. Nøgle i Credential Manager; Mads' nøgle i Bitwarden "Syv.ai - Api key". |
| Ollama (valgfri udbyder) | Referat, talernavne, spørg-mødet, assistent for brugere, der selv har Ollama | Ikke i Mads' opsætning (besluttet 30/10); han bruger den lokale model. |
| Indbygget lokal model (llama-helper, Qwen3.5-4B) | Fallback | Ligger allerede i appen. |
| Windows-API'er | Enhedsskift, mikrofonbrug, vinduer, OCR, TTS, globale genveje, SendInput | Alt i Rust (`windows`-crate). |
| Kalender-ICS | Titler, deltagere | Privat URL, gemt i Credential Manager. |
| Ingest på netcup (mahope-automation) | Wiki + HQ | Ny kilde `tingskriver` med færdigt transskript. Separat PR i mahope-automation. |
| GitHub (repo, Actions, Releases) | Kode, issues, CI, opdatering | Findes. |

## Faser (hver fase = egen gren, review, release)
| Fase | Indhold | Afhænger af | Kan køre parallelt med |
|---|---|---|---|
| 3 | A1 to spor + løbende skrivning + genopretning, A3 transskription (Syv.ai + lokal `da`), D1 udbyder-indstillinger | mute-fix | 7 |
| 4 | A2 noter under mødet, A4 talernavne, A5 referat med skabeloner, A12 eksport | 3 | 7 |
| 5 | A9 mødegenkendelse, A10 indikator-pille, A8 kalender | 3 | 4, 7 |
| 6 | A6 spørg mødet, A7 søgning (FTS5), A11 aflevering + ingest-PR i mahope-automation | 4 | 8 |
| 7 | B1-B8, B10 diktering | mute-fix | 3, 4, 5 |
| 8 | B9 kommandotilstand, C1-C5 assistent | 6, 7 | |
| 9 | D3-D8 finpudsning, genveje, onboarding, projektside, Velopack-overgang, upstream-PR, ydeevne og fejltilstande | alle | |

**Repo og arkitektur (Mads 30/10):** ét offentligt repo, `github.com/mahope/tingskriver`, som monorepo. Forgejo-kopien arkiveres. Brugerfladen bygges i GPUI, hele vejen. Cargo-workspace med `crates/tingskriver-core` (lyd, transskription, database, udbydere og diktering uden UI), `crates/tingskriver-app` (GPUI med gpui-component; GPUI-versionen låses til gpui-components), `crates/tingskriver-overlay` (pille og indikator), samt `site/` (projektsiden) og `installer/` (Velopack). Tauri-appen bliver liggende, indtil GPUI-appen kan det samme, og slettes derefter. Den sidste Tauri-release flytter brugerne over på Velopack-installeren.

**Byggerækkefølge (Mads 30/10): byg det hele på én gang, uden udgivelser imellem.** Faserne bygges parallelt på egne grene med klart filejerskab, flettes i en samlegren `integration/samlet`, og der udgives én version, når helheden er grøn og testet. Prioritet: diktering (B) er vigtigst, så møder (A), så stabilitet og hastighed; assistenten (C) er en "fed feature", men lavest prioritet. UI-teknologi (Tauri eller GPUI) afgøres af research 30/10, før der bygges mere UI.

Releases (besluttet 30/10): vi er i starten, ikke på 1.0-niveau. Versionerne fortsætter i små PATCH-skridt fra den udgivne 0.5.0-dev.2: `0.5.1`, `0.5.2` osv., én pr. færdig og testet delleverance (dev-suffiks kun til prøvebyg). Auto-opdateringen kræver altid et højere nummer. `1.0.0` kommer først, når Mads selv siger, at programmet er klar.

## Færdig-kriterier
- [ ] 10-minutters testmøde: headsettet af og på to gange, mute i 1 minut. Ingen huller over 2 s, ingen skift under mute, begge spor komplette.
- [ ] Nedbrud midt i optagelse (dræb processen): ved næste start tilbydes mødet genoprettet med højst 5 s tab.
- [ ] Dansk møde med to andre: Mads står som "Mads", de andre adskilt og navngivet efter én bekræftelse.
- [ ] Referatet indeholder Mads' egne noter + beslutninger og opgaver, på dansk, lavet af den lokale model, og er i wikien med opgaver i HQ inden for 10 min.
- [ ] Automatisk start og stop for Teams-app, Teams i browser, Meet i browser, Meet-app og Zoom; ingen start ved Discord-talebesked eller diktering.
- [ ] Diktering i Spark, Teams-chat, VS Code og en browser-tekstboks: tekst inde under 1,5 s (Syv.ai, 15 s tale), udklipsholderen uændret bagefter.
- [ ] Snippets, ordbog og talte tegn virker; kommandotilstand omskriver markeret tekst.
- [ ] Assistent: "hvad står der her?" med skærmkontekst og "hvad aftalte jeg med X?" med mødekilde svarer korrekt.
- [ ] Ingen ord "Meetily" synligt for brugeren; al tekst dansk.
- [ ] CI grøn, unit-tests for alle beslutningsfunktioner (enhedsskift, mødedetektor, fletning, renskrivning, snippets).

## Risici og åbne spørgsmål
- **Lokal model på PC'en (besluttet):** referat og assistent kører på den indbyggede model (Qwen3.5-4B via llama-helper). Hastighed og dansk kvalitet på Mads' PC er ikke målt; måles i fase 4 på et rigtigt møde. Referatet laves i baggrunden efter mødet, så minutter er acceptabelt. For lange møder: opdel transskriptet i bidder og flet (map-reduce). Kommandotilstand og LLM-polering ved diktering bliver nok for langsomme lokalt; derfor er lag 1 deterministisk, og polering er valgfri. Plan B, hvis kvaliteten er for svag: større lokal model som valgfri download, eller Mads beslutter senere at åbne Ollama via Tailscale.
- **Syv.ai-latens for diktering** er ikke målt. Måles i fase 7 som det første (prototype). Er den over 2 s, bliver lokal Parakeet standard for diktering.
- **Mødegenkendelse i browseren** (fane-titel kun når fanen er aktiv). Mikrofonbrug pr. browserproces + titel + kalender skal kombineres. Plan B: lille browserudvidelse.
- **Globale genveje og indsættelse** kan blokeres af programmer kørt som administrator (UIPI). Dokumentér; kør ikke Tingskriver som admin.
- **Antivirus/SmartScreen** kan reagere på en global tastaturhook i en usigneret app. Der bruges præcis én `WH_KEYBOARD_LL`-hook (B1), som returnerer hurtigt (< 1 ms) og ikke gemmer tastetryk; søg SignPath.
- **Ingen Windows-compile-løkke for agenter:** `cargo check` stopper på agent-worker (mangler gdk-3.0), og agenter kan ikke se CI. Før fase 5, 7 og 8 (næsten ren Windows-Rust) skal agenterne have en løkke: Linux-Tauri-afhængigheder + `cargo xwin check --target x86_64-pc-windows-msvc` på agent-worker, eller læseadgang til branch-build. "Branch-build grøn" er en del af hver fases færdig-kriterium.
- **Mute-rettelsen kan bringe dongle-hullet tilbage:** når stilheds-skift fjernes, kan Jabra-donglen igen levere nul-frames i 14-21 s, når headsettet slukkes. Løsningen må ikke være indholdsbaseret; der skal findes et signal fra endpoint/jack-tilstand (fx `IAudioEndpointVolume`/`KSJACK_DESCRIPTION`, Jabra-dongle-status) eller accepteres som kendt begrænsning.
- **Fletning med upstream** bliver sværere for hver fase. Accepteret: Tingskriver er nu sit eget produkt; upstream-rettelser plukkes manuelt.
- **Samtykke:** automatisk optagelse optager andre. Indikator + Mads' egen oplysning dækker indtil videre.

## Test-seams
- **Enhedsbeslutninger:** ren funktion (hændelser + buffertilstand → skift/ikke skift). Fanger mute-regressionen.
- **Mødedetektor:** fixtures (proces, titel, mikrofonbrug, kalender) → møde/ikke møde.
- **Transskript-fletning:** to segmentlister → ét transskript med talere.
- **Renskrivning lag 1, snippets, ordbog:** tekst ind → tekst ud, dansk testkorpus.
- **Udbydere:** kontrakt-tests mod en fake OpenAI-kompatibel server.
- **Ingest-kontrakt:** sidecar-skema + `--test` på netcup.
- **UI:** Playwright-skærmbilleder med mock-data (findes) for hver ny skærm, lys og mørk.
- **Manuelt hos Mads:** headset/mute-test, ét rigtigt møde pr. mødetype, diktering i fire programmer.

## Estimat
Cirka 80-120 timers agentarbejde over fase 3-9, plus Mads' tests efter hver release. Største usikkerheder: mødegenkendelse i browser (+10 t ved plan B), diktering-latens (+5 t), den lokale models hastighed og danske kvalitet, GPUI-migreringen (250-450 t ifølge research 30/10; tallet ovenfor dækker ikke migreringen).

## Ordliste
- **Mikrofonspor / systemspor:** Mads' egen stemme / det PC'en afspiller.
- **Referat:** LLM-flettet version af egne noter + transskript (ikke "summary").
- **Diktering:** tal → tekst i et andet program. **Kommandotilstand:** tal en instruks til markeret tekst.
- **Assistent:** spørg med stemmen, svar i Tingskriver.
- **Aflevering:** referat og opgaver sendt videre (ikke "sync").
