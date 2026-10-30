# Tingskriver 1.0: møder, diktering og assistent i ét program

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
- **Data bliver hos brugeren.** Lokal SQLite + filer. Eksterne kald kun til tjenester brugeren selv har sat op. Ingen telemetri. For Mads: mødedata og diktering går kun til Syv.ai og egen Ollama, aldrig Claude/OpenAI.
- **Dansk først.** UI, standardsprog `da`, skabeloner, datoformater, talte kommandoer.
- **Rolig UI.** Noterne er i fokus; status vises diskret; aldrig modale dialoger midt i et møde.

## Scope 1.0

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
C5. **Handlinger (begrænset 1.0):** "skriv et svar på denne mail" (skærm + diktering → udkast indsættes), "lav en opgave" (via aflevering A11), "start optagelse". Ingen autonome klik i andre programmer.
C6. **Wake word "Hej Tingskriver":** ikke i 1.0 (se Ikke i scope).

### D. Fælles
D1. **AI-udbydere** i indstillinger med rolle pr. funktion: transskription (Syv.ai / lokal), referat og assistent (Ollama / OpenAI-kompatibelt endpoint / indbygget lokal model), diktering-polering (hurtig udbyder / fra). Test-knap pr. udbyder med målt svartid.
D2. **Mads' opsætning:** Ollama på netcup via Tailscale (kræver at Ollama også lytter på tailnet-IP; se Risici), Syv.ai-nøgle fra Bitwarden, aflevering via SSH.
D3. **Hurtigtaster** samlet under Indstillinger → Genveje, med konfliktvarsel.
D4. **Start med Windows** (minimeret til tray), et klik til "Ny optagelse", "Diktér", "Spørg".
D5. **Onboarding** i 3 trin: mikrofon og systemlyd, transskription (Syv.ai-nøgle eller lokal model), genveje. Alt andet har gode standarder.
D6. **Opdatering** via GitHub Releases (findes). NSIS-hook der rydder rester efter tidligere binærnavne (`meetily.exe`).
D7. **Projektside** på `tingskriver.mahoje.dk`: én statisk side med skærmbilleder, download-link til seneste release, privatliv i klartekst.
D8. **Upstream:** enhedsskift-rettelsen sendes som PR til Zackriya-Solutions/meetily, når Mads har bekræftet den på rigtigt udstyr.

## Ikke i scope (1.0)
- macOS og Linux (koden må ikke ødelægge upstreams stier, men vi tester dem ikke).
- Wake word / altid-lyttende mikrofon. Kommer måske i 1.x med lokal model (openWakeWord) og tydelig indikator.
- Mødebot, der deltager i mødet; automatisk besked i mødechatten.
- Live-transskript under mødet (efter mødet er nok; noter skrives selv undervejs).
- Billedforståelse (vision-model); vi bruger OCR-tekst.
- Mobil-app, synkronisering mellem maskiner, deling med andre brugere.
- Betalte tjenester som standard; kodesignering ud over evt. SignPath Foundation.

## Data og integrationer
| System | Bruges til | Bemærkning |
|---|---|---|
| Syv.ai (`platform.syv.ai/v1`) | Transskription af møder og diktering | OpenAI-kompatibel, `diarize`, `da`. Nøgle i Credential Manager; Mads' nøgle i Bitwarden "Syv.ai - Api key". |
| Ollama på netcup | Referat, talernavne, spørg-mødet, assistent | `qwen2.5:7b` på 8 CPU-kerner uden GPU: fint til referat, for langsom til diktering. Lytter i dag kun på 127.0.0.1. |
| Indbygget lokal model (llama-helper, Qwen3.5-4B) | Fallback | Ligger allerede i appen. |
| Windows-API'er | Enhedsskift, mikrofonbrug, vinduer, OCR, TTS, globale genveje, SendInput | Alt i Rust (`windows`-crate). |
| Kalender-ICS | Titler, deltagere | Privat URL, gemt i Credential Manager. |
| Ingest på netcup (mahope-automation) | Wiki + HQ | Ny kilde `tingskriver` med færdigt transskript. Separat PR i mahope-automation. |
| GitHub Releases / Forgejo | Kode, CI, opdatering | Findes. |

## Faser (hver fase = egen gren, review, release)
| Fase | Indhold | Afhænger af | Kan køre parallelt med |
|---|---|---|---|
| 3 | A1 to spor + løbende skrivning + genopretning, A3 transskription (Syv.ai + lokal `da`), D1 udbyder-indstillinger | mute-fix | 7 |
| 4 | A2 noter under mødet, A4 talernavne, A5 referat med skabeloner, A12 eksport | 3 | 7 |
| 5 | A9 mødegenkendelse, A10 indikator-pille, A8 kalender | 3 | 4, 7 |
| 6 | A6 spørg mødet, A7 søgning (FTS5), A11 aflevering + ingest-PR i mahope-automation | 4 | 8 |
| 7 | B1-B8, B10 diktering | mute-fix | 3, 4, 5 |
| 8 | B9 kommandotilstand, C1-C5 assistent | 6, 7 | |
| 9 | D3-D8 finpudsning, genveje, onboarding, projektside, NSIS-oprydning, upstream-PR, ydeevne og fejltilstande, release 1.0.0 | alle | |

Releases: `0.6.0-dev.N` efter fase 3-5, `0.7.0-dev.N` efter 6-7, `0.8.0-dev.N` efter 8, `1.0.0` efter Mads' accepttest.

## Færdig-kriterier
- [ ] 10-minutters testmøde: headsettet af og på to gange, mute i 1 minut. Ingen huller over 2 s, ingen skift under mute, begge spor komplette.
- [ ] Nedbrud midt i optagelse (dræb processen): ved næste start tilbydes mødet genoprettet med højst 5 s tab.
- [ ] Dansk møde med to andre: Mads står som "Mads", de andre adskilt og navngivet efter én bekræftelse.
- [ ] Referatet indeholder Mads' egne noter + beslutninger og opgaver, på dansk, lavet af Ollama, og er i wikien med opgaver i HQ inden for 10 min.
- [ ] Automatisk start og stop for Teams-app, Teams i browser, Meet i browser, Meet-app og Zoom; ingen start ved Discord-talebesked eller diktering.
- [ ] Diktering i Spark, Teams-chat, VS Code og en browser-tekstboks: tekst inde under 1,5 s (Syv.ai, 15 s tale), udklipsholderen uændret bagefter.
- [ ] Snippets, ordbog og talte tegn virker; kommandotilstand omskriver markeret tekst.
- [ ] Assistent: "hvad står der her?" med skærmkontekst og "hvad aftalte jeg med X?" med mødekilde svarer korrekt.
- [ ] Ingen ord "Meetily" synligt for brugeren; al tekst dansk.
- [ ] CI grøn, unit-tests for alle beslutningsfunktioner (enhedsskift, mødedetektor, fletning, renskrivning, snippets).

## Risici og åbne spørgsmål
- **Ollama-adgang fra PC'en:** netcup lytter kun lokalt. Plan: Ollama også på tailnet-IP med ufw-regel kun for tailscale0 (ændring på netcup = intern drift; laves additivt med rollback). Alternativ: Tailscale Serve.
- **Hastighed:** `qwen2.5:7b` på CPU giver ca. 5-10 tokens/s. Referat på 600 tokens ≈ 1-2 min, acceptabelt efter mødet. For langsomt til diktering-polering og kommandotilstand; derfor er lag 1 deterministisk og LLM-polering valgfri. Plan B: mindre model (qwen2.5:3b) til polering, eller en GPU-boks senere.
- **Syv.ai-latens for diktering** er ikke målt. Måles i fase 7 som det første (prototype). Er den over 2 s, bliver lokal Parakeet standard for diktering.
- **Mødegenkendelse i browseren** (fane-titel kun når fanen er aktiv). Mikrofonbrug pr. browserproces + titel + kalender skal kombineres. Plan B: lille browserudvidelse.
- **Globale genveje og indsættelse** kan blokeres af programmer kørt som administrator (UIPI). Dokumentér; kør ikke Tingskriver som admin.
- **Antivirus/SmartScreen** kan reagere på en global tastaturhook i en usigneret app. Der bruges præcis én `WH_KEYBOARD_LL`-hook (B1), som returnerer hurtigt (< 1 ms) og ikke gemmer tastetryk; søg SignPath.
- **Ingen Windows-compile-løkke for agenter:** `cargo check` stopper på agent-worker (mangler gdk-3.0), og agenter kan ikke se CI. Før fase 5, 7 og 8 (næsten ren Windows-Rust) skal agenterne have en løkke: Linux-Tauri-afhængigheder + `cargo xwin check --target x86_64-pc-windows-msvc` på agent-worker, eller læseadgang til branch-build. "Branch-build grøn" er en del af hver fases færdig-kriterium.
- **Mute-rettelsen kan bringe dongle-hullet tilbage:** når stilheds-skift fjernes, kan Jabra-donglen igen levere nul-frames i 14-21 s, når headsettet slukkes. Løsningen må ikke være indholdsbaseret; der skal findes et signal fra endpoint/jack-tilstand (fx `IAudioEndpointVolume`/`KSJACK_DESCRIPTION`, Jabra-dongle-status) eller accepteres som kendt begrænsning.
- **Fletning med upstream** bliver sværere for hver fase. Accepteret: Tingskriver er nu sit eget produkt; upstream-rettelser plukkes manuelt.
- **Samtykke:** automatisk optagelse optager andre. Indikator + Mads' egen oplysning dækker 1.0.

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
Cirka 80-120 timers agentarbejde over fase 3-9, plus Mads' tests efter hver release. Største usikkerheder: mødegenkendelse i browser (+10 t ved plan B), diktering-latens (+5 t), Ollama-hastighed (kan kræve ny maskine, uden for budget).

## Ordliste
- **Mikrofonspor / systemspor:** Mads' egen stemme / det PC'en afspiller.
- **Referat:** LLM-flettet version af egne noter + transskript (ikke "summary").
- **Diktering:** tal → tekst i et andet program. **Kommandotilstand:** tal en instruks til markeret tekst.
- **Assistent:** spørg med stemmen, svar i Tingskriver.
- **Aflevering:** referat og opgaver sendt videre (ikke "sync").
