# Revision af funktionernes synlighed

Revision for #169, 4. november 2026. Kun dokumentation og issue-udkast.

Regel: »Alle nye features skal vises i interface, indstillinger, autoopdateringer og alt andet.« Det betyder synlig adgang, relevante valg, tilstande og hjælp. En intern algoritme skal forklares gennem dens brugerresultat; den behøver ikke en kunstig indstilling.

## Kilder og afgrænsning

Main er undersøgt ved `afcf73649c7792979d93db88fddd5184f158401f`. Kilder: `CLAUDE.md`, `CONTRIBUTING.md`, `README.md`, `Cargo.toml`, `docs/` inklusive `docs/SPEC.md`, `docs/plans/70-aflevering.md`, `docs/android/`, `installer/README.md`, alle workspace-craters moduloversigter, GPUI-skallen og dens måleside, core-API'er, Tauri-komponenter, Android-appen, `llama-helper` og `site/`. Filhenvisninger er fra repoets rod. `core/` nedenfor betyder `crates/tingskriver-core/src/`; `app/` betyder `crates/tingskriver-app/src/`.

`git ls-remote --heads origin` og GitHub REST API for åbne issues, lukkede issues og åbne PR'er er læst. Der var fire åbne PR'er. Alle fire er undersøgt med filoversigt, relevant kode og dokumentation fra deres egne git-referencer, uden checkout eller merge. Andre remote-grene, herunder `agent/ui-*` og `issue/49-skaermbilledtest`, er ikke åbne PR'er og tæller ikke som leveret UI. Ingen eksisterende auditgren blev fundet ved opslaget. Der er ingen `graphify-out/` på denne base; en fremtidig `/graphify .` kan supplere filkortet, men der er ikke genereret eller installeret værktøjer i revisionen.

| Åben PR | Gren og undersøgt tip | Status og betydning |
|---|---|---|
| #168 | `issue/105-mcp-server-v2`, `c319a36808a9c88e63a42faabd2852797acf564b` | Kun på denne gren (umerget): MCP-crate, adgangspolitik, migration, IPC, delvise GPUI-indstillinger, GPUI- og Tauri-deling pr. møde samt vejledning. Konflikt med main skal løses før integration. `docs/plans/105-mcp-server.md` findes her, ikke på main. |
| #151 | `issue/36-async-io`, `e523d285691b1e0237f2b4dee24dbff04910dcec` | Umerget PR, men async HTTP, tidsgrænser og blocking workers findes allerede i main. #36 er lukket; PR #167 har flyttet resterende fil-I/O. Den gamle PR's diff er ikke en liste over manglende main-funktioner. |
| #152 | `issue/137-android-indsaettelse`, `498e738068037c3e1b5d8c8486d86e02d43168ea` | Kun på denne gren (umerget): felt-/sessionsbundet Android-indsættelse og livscyklustests. Ingen GPUI-funktion. |
| #130 | `issue/33-moededetektor`, `45c5c0119cc0b45b36bc2a23c288d67758ecbecd` | Kun på denne gren (umerget): ren detektor med fixtures; ingen Windows-signalindsamler eller færdig GPUI-styring. |

Vigtig skelnen: README og `CLAUDE.md` beskriver stadig meget Tauri-adfærd. GPUI `ui.rs` har siderne Møder, Diktering, Måling og Indstillinger. Møder er en pladsholder, også med en åben database; Diktering er et tekstprøvefelt. Indstillinger viser tema, databasestatus og logsti samt åbn log/afslut. Versionsnummer findes i sidefod; en Om-side findes ikke. Måling er en reel funktion. En React-skærm eller et website-skærmbillede er ikke bevis for GPUI-paritet.

## Matrix

En række er en brugerfunktion eller et selvstændigt kontrolpunkt. »Planlagt« betyder, at der ikke er fundet en færdig implementering på main eller de fire PR-grene. Det gør ikke et åbent issue til eksisterende produktkode.

I Om-kolonnen betyder **V**: kun appversion i sidefoden; samlet Om-visning mangler (#171). **M**: model-/motoroplysninger bør indgå i den kopierbare diagnose fra #171. **–**: ingen særlig Om-visning nødvendig ud over V. I opdateringskolonnen betyder **R**: funktionen mangler en verificeret, fælles kobling fra leveret ændring til release notes og brugerhjælp; se udkast 03. **U**: selve opdateringsfunktionen mangler UI, dækket af #170. En grenfunktion må først stå som leveret i ændringsloggen, når den er merget og udgivet. Prioritet vedrører synlighedshullet, ikke hele det oprindelige issue.

| Funktion | Hvor den bor (crate/fil og status) | I interfacet? (skærm) | Indstilling? | Om-siden/versionen | Opdateringsflowet/ændringslog | Dokumentation/hjælp | Hul | Eksisterende issue | Prioritet |
|---|---|---|---|---|---|---|---|---|---|
| Navigation, tema og vinduesplacering | `app/ui.rs`, `theme.rs`, `window_state.rs`; i main | Ja, skal og automatisk tema | Tema vises; ikke fuld indstillingsside | V | R | `docs/gpui.md` | Skallen må ikke præsenteres som færdige funktionsskærme | #85, #97 | mellem |
| Databaseopstart, ventetilstand og genforsøg | `app/startup.rs`, `ui.rs`; i main | Ja, loader og fejlside med prøv igen/log | Databasestatus vises | V | R | `docs/arkitektur.md` | Dette er implementeret; øvrige jobfejl er ikke dermed dækket | #5 (lukket), #85 | lav |
| Mødeliste, omdøbning og sletning | `core/database/`; i main | Nej, Møder er pladsholder | Ikke relevant | – | R | README viser Tauri | Port liste, status og handlinger | #56 | høj |
| Optagelse af mikrofon/systemlyd | `core/audio/recording_commands.rs`, `pipeline.rs`; i main | Ingen reel GPUI-optagekontrol | Enhedsvalg mangler | M | R | `docs/arkitektur.md`, `enheder.md` | Start/stop/pause, niveau og valgt enhed | #56, #82, #85 | høj |
| To spor, diskcheckpoints, slutkodning og verifikation | `core/audio/recording_writer.rs`, `track_writer.rs`, `finalize.rs`; i main | Ingen samlet GPUI-gemt/koder/fejlet-status | Ingen teknisk tuning nødvendig | – | R | `docs/audio-issue-6-report.md` | Skeln sikkert gemt rålyd fra færdig kodning | #6, #7, #51, #82 | høj |
| Lagringsfejl og degraderet optagelse | `core/audio/`; i main, #7 stadig åben | Mangler i GPUI og end-to-end overlay-bevis | Genforsøg er systemadfærd | – | R | #7 og lydrapporter | Vis fuld disk, utilgængelig encoder og bevarede råspor; ikke blot log | #7 | høj |
| Genopret efterladte optagelser | `core/recover/`; i main | Ingen GPUI-genopretningsliste | Bruger vælger genopretning | – | R | `docs/arkitektur.md` | Tilbyd fund, fremdrift og fejl fra scan/restore | #56; #54 lukket for core | høj |
| Enhedsskift og mute-genopretning | `core/audio/devices/`, capture og monitor; i main | GPUI-beskeder mangler | Mikrofon/systemlyd under Lyd mangler | M | R | `docs/enheder.md` | Vis valgt enhed, tab og automatisk genopretning | #82, #85; #21/#35 lukket | høj |
| Transskriptionskø, backoff og kør igen | `core/transcription_queue/`; i main | Ingen GPUI-kø/procesbånd | Udbyder/sprog ved genkørsel mangler | M | R | `docs/transcription-queue.md` | Vis ventetid, forsøg, fejl og bevaret tidligere version | #52, #51, #56 | høj |
| Lange optagelser og genoptagelige bidder | `core/audio/`, `transcription_queue/`; i main | Nej, indgår i manglende køvisning | Ingen chunk-tuning nødvendig | – | R | `docs/transcription-chunks.md` | Samlet status frem for interne bidder; genoptagelse skal forklares | #48, #52, #51 | mellem |
| VAD og tidsmapping | `core/vad_trim/`; i main | Ikke direkte, ingen færdig transskriptside | Kun relevante bruger-/modelvalg | M | R | `docs/vad.md` | Bevar originale tidskoder i synligt resultat; ingen særskilt VAD-side | #83, #52; #23 lukket | mellem |
| Flettet transskript, talere og tidskoder | `core/transcript_merge/`; i main | Nej | Brugerens navn under Generelt mangler | – | R | `docs/transskriptfletning.md` | Talere, redigering og klik til tid | #83, #85, #87; #18 lukket | høj |
| Lydafspilning, seek og hastighed | `core/playback/`; i main | Nej | Afspilningskontrol mangler | – | R | `docs/afspilning.md` | Transport, fejl og afspilbar tidskode | #51, #83; #17 lukket | høj |
| Tidsstemplede noter og autosave | `core/notes/`; i main | Nej | Ikke relevant | – | R | `docs/noter.md` | Editor, gemmestatus og fejl uden datatab | #47, #51; #15 lukket | høj |
| Referatgenerering og afbrudte jobs | `core/summary/`, `database/`; i main, ny referatmotor planlagt | Nej | Udbyder/model og skabelon mangler | M | R | `docs/arkitektur.md`, README | Procesbånd, fejl/genforsøg og tidligere resultat; skeln gammel motor fra #44 | #44, #51 | høj |
| Referatskabeloner | `core/summary/templates/`, `template_commands.rs`; i main | Ingen GPUI-editor/vælger | Mangler | – | R | Core/API, #66 | Port standard/egne skabeloner og valg | #66, #51 | mellem |
| Fuldtekstsøgning og paginering | `core/database/repositories/search.rs`, transcript; i main | Nej | Ikke relevant | – | R | `docs/soegning.md` | Søgefelt, uddrag og tidskode-navigation | #68; #31/#38 lukket | høj |
| Import af lyd/video | `core/audio/import.rs`; i main | Ikke i GPUI; Måling importerer kun prøve-WAV | Udbydervalg mangler | M | R | Tauri og #89 | Filvælger, kø, fejl, formatbegrænsninger | #89, #52 | mellem |
| Manuel eksport og kopiér referat | Tauri-komponenter; i main, fuldt eksportformat planlagt | Nej | Filnavn/format mangler | – | R | README, #67 | GPUI-eksport; docx er ikke bevist implementeret | #67, #51 | mellem |
| Aflevering til mappe/webhook/SSH | `core/delivery/`; i main; Tauri har UI | Nej | Ingen GPUI-mål, test eller politik | – | R | `docs/delivery.md`, schema | Godkendelse, mål, sikker test og køstatus mangler i GPUI | #70 lukket; #51/#85 generelle. Nyt udkast 02 | høj |
| Versioneret afleveringskontrakt og validering | `docs/schemas/delivery-v1.schema.json`, schema-tests; i main | Ingen validerings-CLI i appens args | Ikke relevant | – | R | `docs/delivery.md` | Skema er leveret; samlet CLI og versionsgate er ikke hele #99 | #99 | mellem |
| Fælles udbydermodel og reservekæder | `core/providers/model.rs`, `chain.rs`, `service.rs`; i main | Kun Måling kan læse opsætning | Ingen GPUI-redigering/test-knap | M | R | `docs/arkitektur.md` | Model-id, nøgler, kæde pr. funktion, svartid og dansk fejl | #53; #19 lukket | høj |
| Eksterne udbydere, Ollama og lokal AI | `core/{openai,anthropic,groq,openrouter,ollama}/`; i main | Ikke almindelig GPUI-opsætning | Mangler; OdinCore planlagt | M | R | README og core | Afstem faktisk udbyderliste; eksterne kald kræver valg | #53, #60 | høj |
| Desktop-modeldownload, annullering og sletning | `core/whisper_engine/commands.rs`, `parakeet_engine/commands.rs`, `summary/summary_engine/model_manager.rs`; i main | Kun Tauri-modelmanagere | Ingen GPUI-modelmanager | M | R | `docs/lokal-model.md`, `GPU_ACCELERATION.md` | Installeret/mangler/henter/fejl, størrelse, kilde og eksplicit download | #53 dækker valg, #134 kun Android. Nyt udkast 01 | høj |
| Modelvalg pr. funktion og sprog | `core/providers/`, `dictation/settings.rs`, `lib.rs`; i main | Måling vælger forsøg, ikke appens aktive modeller | Mangler i GPUI | M | R | `docs/measurement.md`, `dictation-text.md` | Skeln forsøg fra aktivt valg; dansk uden utilsigtet oversættelse | #53, #50 | høj |
| Varm model, idle-unload og GPU/CPU | `core/dictation/warm.rs`, `logic/residency.rs`, motorer; i main | Ingen synlig aktiv/reserve/indlæser-status | `model_idle_minutes`, `keep_warm_models` uden GPUI | M | R | `docs/arkitektur.md`, `lokal-model.md` | Eksponér relevante valg i diktering og modelmanager | #50, #53; udkast 01 | mellem |
| Separat STT-proces | `core/stt/`, `bin/tingskriver-stt.rs`; i main | Ingen normal brugerflade til motorfejl | Ingen processwitch nødvendig | M | R | `docs/stt-issue-71-report.md` | Vis utilgængelig model/proces og genforsøg uden protokolstøj | #52, #53; #71 lukket | høj |
| Lokal llama-helper og genstart efter fejl | `llama-helper/src/main.rs`, `core/summary/summary_engine/sidecar*`; i main | Ingen GPUI-referatfejlvisning | Lokal model via udbydersiden mangler | M | R | `docs/llama-helper.md` | Timeout, afbrudt generation og motorstatus til brugeren | #51, #53; #20 lukket | mellem |
| Diktering end-to-end og tekstindsættelse | `core/dictation/service.rs`, `target_win.rs`, `win.rs`; i main | Diktering er prøvefelt; native overlay findes | Ingen GPUI-aktivering/mikrofon/engine | M | R | README, `docs/host.md` | Forbind core, vært og GPUI; tekstprøve er ikke diktering | #46, #50, #79, #80 | høj |
| Genvejsgestus, Esc og genkør sidste | `core/dictation/logic/gesture.rs`, hotkey; i main, udvidelser planlagt | Ikke i GPUI | `shortcut` uden editor | – | R | `docs/host.md`, #75/#76 | Vis den faktisk gemte værdi; konflikt og hjælp; nye gestus kræver core | #9, #64, #75, #76 | høj |
| Global vært, tray og enkelt instans | `crates/tingskriver-host/src/`, `app/single_instance.rs`; i main | Tray/skal delvist; ikke færdige produktkommandoer | Autostart/genveje mangler | V | R | `docs/host.md` | Bind menupunkter til rigtige handlinger og tilstande | #9, #64, #85 | høj |
| Native pille og optageindikator | `crates/tingskriver-overlay/src/`; i main | Native overlay findes; komplet GPUI-binding mangler | Ikke relevant | – | R | `docs/host.md`, `gpui.md` | Vis capture/arbejder/fejl uden fokus; ikke bevis for fuld optagelses-UI | #46, #82; #10 lukket | høj |
| Danske tal, adresser, tegn og rå tilstand | `core/dictation/logic/`, `format_da.rs`; i main | Ingen GPUI-regel-/prøvevisning | `format_numbers`, `format_addresses`, `app_styles`, spoken-fil | – | R | `docs/dictation-text.md` | Valg, eksempler og redigering af talte tegn | #50, #14, #28, #29, #93 | mellem |
| Programtilstande og import/eksport | `core/dictation/modes.rs`, `settings.rs`; i main | Nej | Ingen GPUI-editor | – | R | `docs/dictation-text.md` | Tilstande, matchregler og aktiv tilstand skal fremgå | #50, #85; #13 lukket | høj |
| Personlig desktop-ordbog og snippets | `core/dictation/logic/`, tilstandsregler og Tauri; i main, GPUI-editor mangler | Nej | Ingen GPUI CRUD/import/eksport | – | R | `docs/dictation-text.md`, #50 | Skeln egne regler fra automatisk læring og Android | #50, #81 | mellem |
| Personlig Android-ordbog | Planlagt; `docs/android/SPEC.md` | Nej, ikke GPUI-scope | Android-indstillinger mangler | – | R | #139, Android-spec | Egne ord/aliaser, ingen snippets eller historik; ikke desktop-port | #139, #138 | mellem |
| Dikteringshistorik og lydretention | `core/dictation/store.rs`, `settings.rs`; i main | Ingen GPUI-historik | `history_enabled`, `keep_audio` uden GPUI | – | R | README, #74/#63 | Kopiér/kør igen, samtykke og sletning | #74, #50, #63, #84 | høj |
| Valgfri polering, kommando og oversættelse | Polering i `core/dictation/`; øvrige tilstande planlagt | Nej | `polish_*` uden GPUI | M | R | `docs/dictation-text.md` | Opt-in, tidsbudget og tydelig tilstand; undgå at love planlagt funktion | #50, #77, #78, #102, #103 | mellem |
| Dikteringstider og udbydersammenligning | `app/measurement.rs`, `core/measurement/`, dictation timings; i main | Måling virker: import, valg, resultater, gemmefejl | Testmodeller vælges eksplicit; timing-dashboard mangler | M | R | `docs/measurement.md`, `ydeevne.md` | Bevar eksisterende Måling; median/p95 til diktering mangler | #50; #12/#69 lukket | mellem |
| Kalender og hotwords | `core/calendar/`; i main, hotword-integration planlagt | Nej | Ingen GPUI ICS/test/næste møder | – | R | `docs/kalender.md` | Sikker adresse, cachefejl, test; deltagere må ikke skjult sendes | #85, #57; #32 lukket | mellem |
| Diagnose, log og crash-oplysninger | `core/diagnose/`, `crash_dumps.rs`, `app/crash.rs`; i main | Logmappe/DB-fejl ja; diagnosefund nej | Diagnose og eksport mangler | M | R | `docs/diagnose.md`, CLI | Vis rettelser og redigeret eksport; ingen rå indholdskopi | #85, #171; #34/#37 lukket | høj |
| CLI-start, diagnose og eksterne triggere | `app/args.rs`; i main | `--hidden`, `--minimized`, `--page`, `--diagnose`, `--json` | Ikke relevant | V | R | `docs/diagnose.md`, `gpui.md` | Disse findes; start/stop-CLI og `tingskriver://` er planlagt | #95, #97, #99 | mellem |
| Asynkron HTTP og fil-I/O | `core/http.rs`, `runtime.rs` og callers; i main; gammel #151 umerget | DB og Måling har vente/fejl; andre sider mangler | Ingen »async«-kontakt | – | R | `docs/arkitektur.md`; rapport på #151-grenen | Loading, timeout, retry og afsluttede jobs skal nå UI uden blokering | #53, #51, #52, #85, #7; #36 lukket | høj |
| Privat tilstand, kun lokal og datastrømme | Delpolitik i `core/delivery/`, dictation settings; samlet funktion planlagt | Ingen GPUI-privatlivsside | Samlede kontakter mangler | – | R | README, #63/#84 | Delivery-politik er ikke global blokering af alle udbydere | #63, #84 | høj |
| Autostart og installation/migration | `core/install/`, `installer/`, `app/update.rs`; i main | Ingen samlet GPUI-styring | Autostart mangler | V | U | `installer/README.md` | Gør migrations-/installationsstatus forståelig; signering/winget udestår | #22, #45, #62, #85 | høj |
| Om, build, licenser og diagnosekopi | Sidefods-version i `app/ui.rs`; i main | Om findes ikke | Link fra indstillinger/tray mangler | V og M mangler samlet | U | `NOTICE.md`, `THIRD_PARTY_LICENSES.md` | Lokal Om-side og sikker diagnosekopi | #171 | høj |
| Opdateringstjek og automatisk valg | `app/update.rs`; i main | Nej; status har danske labels men ingen UI | Kun feed via miljø; ingen gemt brugerkontakt | V | U | `installer/README.md`, README | Baggrundstjek er automatisk for installerede kopier; krævet opt-in mangler | #170, #63 | høj |
| Download, release notes og genstart | `app/update.rs`; i main | Nej | Kanal følger build-version; ikke brugerens valg | V | U | `installer/README.md` | Størrelse/fremdrift, ændringslog, hent/installér/genstart senere og fejl | #170 | høj |
| Blokér opdatering/afslut under arbejde | Startup single-instance-beskyttelse i `app/update.rs`; i main | Afslut-knap kalder `cx.quit()` | Ingen fælles guard | – | U | #55; updater tests | Opstartsbeskyttelse er ikke runtime-guard for optagelse/diktering | #55, #170 | høj |
| Stabil/beta, portable og signaturstatus | `app/update.rs`, installer workflows; i main, signering udestår | Nej | Feed override er ikke understøttet kanalvælger | V | U | `installer/README.md`, #62 | Skeln portable/deaktiveret/fejl/ingen release; vis faktisk støtte | #170, #171, #62 | høj |
| MCP stdio-server og værktøjer | `crates/tingskriver-mcp/src/main.rs`; kun gren `issue/105-mcp-server-v2` (umerget) | Ikke main; værktøjsliste mangler på grenen | Adgangsvalg findes på grenen | V | R | Grenens `docs/mcp.md`, README | Vis kapabiliteter og begrænsninger, ikke blot serverfil | #105, PR #168 | høj |
| MCP aktivér/fra og adgangspolitik | Grenens `core/mcp.rs`, migration og `app/ui.rs`; kun samme gren (umerget) | Ja delvist på grenen, Indstillinger | Marked/Metadata/All/Off; gemmebesked | – | R | Grenens MCP-vejledning | Loading/gemmefejl, konsistent valg og sikker standard; Off er politik, ikke fjernelse af klient | #105, PR #168 | høj |
| MCP deling pr. møde | Grenens `core/mcp.rs`, `app/ui.rs::meeting_sharing`, Tauri `MeetingHeader.tsx`; kun samme gren (umerget) | Ikke main; GPUI Møder og Tauri ja på grenen | Del/fjern-deling pr. møde samt Opdater i GPUI på grenen | – | R | Grenens `docs/mcp.md` beskriver begge apps | Bevar og verificér eksisterende mødeliste, loading/tom/fejl, asynkron `set_shared()` og tidligere værdi ved gemmefejl under integration | #105, PR #168; #56/#51 er senere samlet møde-UI, ikke blokering | høj |
| MCP Claude Code/Codex og forbindelsestest | Grenens `app/ui.rs::mcp_settings`; kun samme gren (umerget) | Kopiér-knapper findes; test mangler | Ingen synlig server-/klientstatus | M | R | Grenens README og `docs/mcp.md` | Kontrollér sti, stdio handshake/tools/list; ikke et skjult AI-kald | #105, PR #168 | høj |
| MCP app-handlinger og samtykke | Grenens `core/mcp_ipc.rs`, host/app; kun samme gren (umerget) | Handlingsnotifikationer findes på grenen; GPUI-mødeside mangler | Politikker og afleveringsmål mangler samlet | – | R | Grenens MCP-vejledning | Fem læse- og fem handlingsværktøjer; vis handling/fejl, read-only når app lukket | #105, #51; udkast 02 for GPUI-aflevering | høj |
| Mødedetektor og per-kilde automatik | `core/meeting_detect/` på `issue/33-moededetektor` alene (umerget) | Nej | Automatic/Ask/Off i model, ingen UI | – | R | Grenens `docs/moededetektor.md` | Signalindsamler, forslag og kildevalg mangler; ren funktion optager ikke | #33, PR #130, #59, #58 | mellem |
| Android IME, tilladelse og PCM | `apps/android/`; i main | Android UI ja; ingen genkendelse/tekst endnu | Native indstillinger, ikke GPUI | Separat Android-version | R | `apps/android/README.md`, Android-spec | Stub skal være tydelig; STT, tekstbro og enhedstest udestår | #136, #138, #140 | mellem |
| Android sessionsbundet indsættelse | `InsertionSession.kt` på `issue/137-android-indsaettelse` alene (umerget) | Native IME på grenen | Ikke GPUI-indstilling | Separat Android-version | R | Grenens `docs/android/ISSUE-137-REPORT.md` | Vis afbrydelse ved felt-/sessionændring; kun egne sessionsresultater | #137, PR #152, #140 | mellem |
| Android modeldownload/benchmark | Benchmarkværktøj i main; download planlagt | Ingen download-UI | Samtykke, størrelse og modelrevision mangler | Android-modelversion | R | `docs/android/BENCHMARK.md`, SPEC | Checksum, atomisk aktivering, annullering/diskfejl; ikke desktop | #133, #134, #136 | mellem |
| Assistent, mødechat, OCR og handlinger | Planlagt i `docs/SPEC.md` | Nej | Ikke implementeret | – | R | #88, #98–#104 | Ingen færdig core fundet; egne eksisterende issues, ikke nye auditdubletter | #88, #98, #100, #101, #104 | lav |
| Backend-arkiv, Tauri og website-løfter | `backend/`, `frontend/`, `site/`; i main | Tauri har mere UI end GPUI | Gamle indstillinger er ikke GPUI | V | R | `CLAUDE.md` siger stadig Tauri understøttet | Markér arkiv, migration og faktisk appstatus; behold features indtil paritet | #92, #97; #43 lukket | høj |
| Synlighed som fast leverancekrav | `.github/pull_request_template.md`, `docs/release-checkliste.md`; i main | Ikke produktfunktion | Ikke relevant | Kræv #171-data ved ændringer | R | Kravet er ikke samlet i leverancekontrol | Forebyg gentagelse: interface/valg/hjælp/release notes med begrundet ikke relevant | Nyt udkast 03 | mellem |

## Rækkefølge og afhængigheder

| Trin | Leverance | Afhænger af / kan køre samtidig |
|---|---|---|
| 1 | #171: Om-side og fælles build-/versionsdata | Kan bygges på main nu; aftal opdateringsfeltets kontrakt med #170. |
| 2 | #170 og #55: synlige, valgbare opdateringer og fælles aktivitetsguard | Bruger Om-værten fra #171; selve tilstandsmodellen kan bygges parallelt. Ingen runtime-genstart før guard er tilsluttet både møde og diktering. |
| 3 | #105 på PR #168: afslut MCP-indstillinger og bevar deling | Afklar konflikter med async-main; genbrug #171/#170-navigation. Bevar grenens GPUI-deling på Møder; den kræver ikke #56/#51, som senere kan integrere kontrollen i det samlede møde-UI. |
| 4 | #53, udkast 01, #50, #64 og #85 | Udbyder-/modelvalg før onboarding (#86). Modeldownload og polering må ikke aktiveres ved blot at åbne indstillinger. |
| 5 | #56, #47, #51, #83, #52, #7 og #82 | Mødeliste og dokument først; bind de eksisterende core-events, fejl og køer; genopretning skal være tilgængelig. |
| 6 | Udkast 02: GPUI-aflevering | Bygger på core #70 (landet), #51/#85 samt #84-politik. #105 handoff deler service, men erstatter ikke brugerens kontroller. |
| 7 | #59 efter #33/PR #130; Android #136/#138/#134/#139 | Separate produktspor; ingen desktop-issue må lukkes med Android-kode eller omvendt. |
| Løbende | #49, #73, #97 og udkast 03 | Lys/mørk, fokus/tastatur, AccessKit/UIA og tom/loading/fejl i hvert job. Ingen nye featureløfter i site før funktionen er leveret. |

## Konkret overdragelse til de tre næste jobs

### Job 1: Om-/versionsside (#171)

Berør `crates/tingskriver-app/src/ui.rs` (Page, navigation, settings-link og rendering), `crates/tingskriver-app/src/args.rs` ved behov for `--page about`, `crates/tingskriver-host/src/{lib,win}.rs` for tray-routing og `crates/tingskriver-core/src/strings/mod.rs`. Læs version fra workspace/package-versionen; suppler builddato/commit/platform gennem deterministiske builddata, eventuelt et nyt app-buildmodul. Ingen gættet dato eller netværksopslag. Afstem `installer/` og `.github/workflows/app-release.yml`, og indlæs `NOTICE.md`/`THIRD_PARTY_LICENSES.md` lokalt.

Byg navn, version, build, kanal, platform, licenser, privatliv/kontakt og en diagnosekopi med allowlist (version/OS/GPU/modelversioner, ingen datasti, logindhold, nøgle, mødetitel eller brugeridentitet). Del en opdateringsstatus-komponent/kontrakt med #170; en installeret kopi må ikke fejlagtigt kaldes inaktiv. Portable/ikke-understøttet build har en præcis lokal forklaring. Tilføj danske strenge, tastatur/fokus, AccessKit-navne og mock-skærmbilleder i lys/mørk via #49, hvis infrastrukturen er tilgængelig. Windows/tray og UIA kræver faktisk Windows-verifikation.

### Job 2: Synlig opdatering (#170)

Primære filer: `crates/tingskriver-app/src/update.rs`, `lib.rs` (opstart), `ui.rs` (Om/indstillinger), `crates/tingskriver-host/src/` (tray), core-settings/persistens og strengkataloget; `installer/README.md`, README og relevante release workflows. Genbrug Velopack, ikke Tauri-updateren. `UpdateStatus` og `status()` findes allerede, men UI observerer dem ikke; polling/notification skal opdatere uden at blokere GPUI.

Split kontrollér/hent/anvend, så brugerens valg håndhæves før netværk, også ved opstart og planlagte gentagelser. Gem til/fra og interval; eksisterende kode starter efter 60 sekunder og downloader automatisk. Vis ny version, størrelse, release notes, fremskridt og klar til genstart. Adskil offline/fejl, ingen release, opdateret og deaktiveret/portable. Kanalvalg vises kun med reel installerunderstøttelse; build-afledt prerelease er ikke i sig selv en kanalvælger. #55 leverer fælles optager/dikterer-guard for afslut/installér/genstart; `should_apply_pending()` ved opstart er ikke nok. Test ren tilstandsmodel, gemt fravalg uden netværk, aktivitet under hentning og klar/genstart samt retry/signaturfejl. Mock alle syv #170-tilstande i lys/mørk; verificér rigtig installer på Windows separat.

### Job 3: MCP-indstillinger på PR #168 (#105)

Arbejd på `issue/105-mcp-server-v2`, ikke ved at antage MCP findes i main. Læs grenens `docs/plans/105-mcp-server.md` og `docs/mcp.md`. Løs konflikten med main uden at miste nye async delivery-workers. Primære filer: `crates/tingskriver-app/src/ui.rs` (`mcp_settings` og modeltilstand), `core/mcp.rs`, `core/mcp_ipc.rs`, migrationen, `crates/tingskriver-mcp/src/main.rs`, `crates/tingskriver-host/src/`, strings samt installer-workflows. React `McpSettings.tsx` og `MeetingHeader.tsx` er reference, ikke den færdige GPUI-løsning.

Bevar fire politikker og sikker standard Kun markerede møder. Off deaktiverer adgang, ikke klientprocessens eksistens; ingen særlig ejerinstallation i kode eller migrationsdata. Færdiggør gemmer/loading/fejl, kopiér og vis den præcise klientopsætning med korrekt citeret runtime-sti. Tilføj eksplicit forbindelsestest af lokal server/stdio handshake og tools/list med timeout, uden at hente mødeindhold eller kontakte en AI-udbyder. Vis de faktiske ti værktøjer: `list_meetings`, `get_meeting`, `search`, `list_tasks`, `recent_dictations`, `start_recording`, `stop_recording`, `regenerate_summary`, `handoff`, `add_note`. Forklar adgangskrav, app-lukket og privacy-konsekvensen ved eksterne AI-klienter.

Bevar grenens eksisterende GPUI-deling i `ui.rs::meeting_sharing()` på Møder. På det undersøgte commit `c319a36808a9c88e63a42faabd2852797acf564b` findes allerede mødeliste, Opdater, del/fjern-deling, loading/tom/fejl og asynkron `core::mcp::set_shared()`; visningen ændrer først delingsværdien efter vellykket lagring og bevarer den ved gemmefejl. Grenens `docs/mcp.md` beskriver også GPUI-kontrollerne. Verificér begge delingsretninger, opdatering af listen og gemmefejl efter konfliktløsningen. Funktionen mangler på main, men skal ikke genbygges eller vente på #56/#51; de kan senere indarbejde den i det samlede møde-UI. Handlinger skal give notifikation og sikker log uden indhold. Test politikændring under brug, alle fire niveauer, udelukkede møder, lukket app, manglende binær, timeout og kopieret Claude/Codex-opsætning. Grenens PNG'er er af React-kontroller og beviser ikke GPUI-rendering; tilføj native mock-tests efter #49 og verificér klienterne på Windows.

## Nye issue-udkast og dubletkontrol

Udkastnumrene er lokale filnumre, **ikke GitHub-issue-numre**. `docs/audit/issues.json` er en liste med kun `title`, `body`, `labels`, klar til senere oprettelse. Der oprettes ingen issues her.

| Udkast | Afgrænsning mod eksisterende issues |
|---|---|
| [01: Desktop-modelmanager](../audit/issues/01-desktop-modelmanager.md) | #53 vælger udbyder/model-id; #134 er Android-download. Ingen af dem specificerer desktop-modellernes samlede download/annullér/slet/fejl-UI. #50 beholder dikteringsvalg. |
| [02: GPUI-aflevering](../audit/issues/02-gpui-aflevering.md) | #70 er lukket efter core/Tauri; #51 har dokument/eksportmenu, #67 manuel fileksport, #85 generelle indstillinger. Ingen af dem specificerer mål, godkendelse og vedvarende afleveringskø i GPUI. #99 beholder skema/validerings-CLI. |
| [03: Fast synlighedskontrol](../audit/issues/03-synlighedskontrol.md) | #97 opdaterer bygge-/bidragsvejledning; #169 er denne engangsrevision. Udkastet indfører fremtidig PR-/releasekontrol, ikke en ny Om-/opdateringsside. |

Alle øvrige fund sendes til de nævnte eksisterende issues, også hvis et ekstra acceptkriterium bør tilføjes dér. Lukkede core-issues er baggrund, ikke påstand om færdig GPUI. Der foreslås ingen nye MCP-, async-, Om- eller updater-issues.

## Verifikation og begrænsninger

Revisionen er statisk: ingen GUI, optagelse, modeldownload, AI-klient, netværkslevering eller installer er kørt. Eksisterende rapporter er kilder til tidligere arbejde, ikke nye testresultater. Produktkode er uændret, og Cargo/GPUI-build/tests er derfor ikke brugt som bevis for dokumentændringen. Dokumenterne kontrolleres for tabelstruktur, filreferencer, eksisterende issue-numre, gyldig JSON, match mellem JSON og Markdown, kendte labels, UTF-8 og offentligt indhold. Ingen pakker installeres. Review og PR-oprettelse er efter opgavens instruktion reserveret til den efterfølgende gennemgang.

Nedenfor står det komplette undersøgte issue-/PR-indeks fra API-snapshottene. Åbne og lukkede issues er begge medtaget for at undgå at oprette en dublet af allerede leveret core-arbejde. API-snapshot: tre kald til issues/pulls; efterfølgende CI-opslag tæller separat mod den anonyme grænse på 60/time. Kontrollér nytilkomne issues igen før udkastene oprettes.


<!-- issue-index -->

| Nummer | Status | Undersøgt titel |
|---|---|---|
| #6 | Åben issue | Skriv to lydspor løbende til disk med højst 2 s tab |
| #7 | Åben issue | Gør lagringsfejl synlige og stop gentagne kodeforsøg |
| #9 | Åben issue | Byg vært til genvejshooken og tray uden for GPUI-løkken |
| #14 | Åben issue | Udvid talte tegn og forstå Windows' danske stemmekommandoer |
| #22 | Åben issue | Byg Velopack-installer og -opdatering i installer/ |
| #28 | Åben issue | Formatér tal, datoer, klokkeslæt og beløb efter dansk skik |
| #29 | Åben issue | Bevar engelske ord i danske sætninger ved diktering |
| #33 | Åben issue | Byg mødedetektor som ren funktion med fixtures |
| #40 | Åben issue | Lav mixet afspilningsspor uden nul-huller ud fra de to spor |
| #41 | Åben issue | Gør lyd-callbacken låsefri og fejlgrænser tidsbaserede |
| #42 | Åben issue | Ryd op i core: fjern analytics og nulstil hængende status |
| #44 | Åben issue | Byg referatmotoren: flet noter og transskript til dansk referat |
| #45 | Åben issue | Flyt Tauri-brugere til Velopack med en sidste Tauri-release |
| #46 | Åben issue | Kobl diktering end-to-end i core uden hovedvindue |
| #47 | Åben issue | Byg notesblokken i GPUI: markdown-lite med dansk tastaturstøtte |
| #48 | Åben issue | Del lange optagelser i bidder ved pauser og sæt dem sammen igen |
| #49 | Åben issue | Byg skærmbilledtest for GPUI-skærme med mock-data i lys og mørk |
| #50 | Åben issue | Byg dikteringsindstillinger i GPUI: tilstande, ordbog og snippets |
| #51 | Åben issue | Byg mødedokumentet i GPUI: noter, referat og status |
| #52 | Åben issue | Byg transskriptionskø med genforsøg og mulighed for at køre igen |
| #53 | Åben issue | Byg udbyderindstillinger i GPUI med test-knap og målt svartid |
| #55 | Åben issue | Bloker Afslut, logoff og opdatering under optagelse |
| #56 | Åben issue | Byg mødelisten i GPUI med genoprettelse og tom-tilstand |
| #57 | Åben issue | Send kalenderdeltagere og ordbog som hotwords til transskriptionen |
| #58 | Åben issue | Find endpoint-signal for slukket headset uden indholdstjek |
| #59 | Åben issue | Byg automatisk mødegenkendelse med optag, spørg eller fra pr. kilde |
| #60 | Åben issue | Tilføj OdinCore som udbyder med korrekt domæne og modeller |
| #62 | Åben issue | Signér installer og opdateringer, og udgiv i winget |
| #63 | Åben issue | Byg privatlivsfunktioner: auto-sletning, privat tilstand, kun lokal |
| #64 | Åben issue | Byg genvejsindstillinger i GPUI med konfliktvarsel |
| #66 | Åben issue | Tilføj referatskabeloner med egne skabeloner og automatisk valg |
| #67 | Åben issue | Eksportér referat som markdown, docx og kopiér til mail |
| #68 | Åben issue | Byg søgning på tværs af møder i GPUI med uddrag og tidskoder |
| #72 | Åben issue | Tilføj kopierbar besked til mødedeltagerne om optagelse |
| #73 | Åben issue | Opnå tilgængelighedsbaseline med AccessKit og UIA |
| #74 | Åben issue | Byg dikteringshistorik i GPUI med kopiér og kør igen |
| #75 | Åben issue | Giv genvejen fire tilstande og lad Esc afbryde en diktering |
| #76 | Åben issue | Indsæt eller kør den sidste diktering igen med en genvej |
| #77 | Åben issue | Tilføj valgfri LLM-polering med hård tidsgrænse på 2 s |
| #78 | Åben issue | Byg kommandotilstand: tal en instruks til markeret tekst |
| #79 | Åben issue | Byg testmatrix for indsættelse i de programmer, danskerne bruger |
| #80 | Åben issue | Gem teksten i en kladde, når intet tekstfelt har fokus |
| #81 | Åben issue | Foreslå ordbogsord ud fra brugerens rettelser, uden tastelogning |
| #82 | Åben issue | Byg optageindikator med pause, stop og lydniveau |
| #83 | Åben issue | Byg transskriptvisningen i GPUI med afspilning og talerfarver |
| #84 | Åben issue | Byg privatlivssiden i GPUI med datastrømsoversigt og kontakter |
| #85 | Åben issue | Byg generelle indstillinger i GPUI: lyd, kalender, start og diagnose |
| #86 | Åben issue | Byg onboarding i GPUI med prøve-diktering som sidste trin |
| #87 | Åben issue | Foreslå talernavne fra samtale og kalender, bekræft med ét klik |
| #88 | Åben issue | Byg chatfelt til at spørge mødet med afspilbare tidskoder |
| #89 | Åben issue | Importér lyd- og videofiler og få samme referat som en optagelse |
| #90 | Åben issue | Understøt møder i lokalet: del mikrofonsporet i talere |
| #91 | Åben issue | Kør en 3-timers optagelse som fast ydelsestest |
| #92 | Åben issue | Kør paritetscheck og fjern Tauri-appen i frontend/ |
| #93 | Åben issue | Tilføj udviklertilstand til variabelnavne, filnavne og kommandoer |
| #94 | Åben issue | Prøv streaming af dikteringen, så teksten er klar ved slip |
| #95 | Åben issue | Tilføj kommandolinje og tingskriver://-links til eksterne triggere |
| #96 | Åben issue | Send enhedsskift-rettelsen som PR til upstream Meetily |
| #97 | Åben issue | Skriv README, bidragsguide og byggevejledning for GPUI på Windows |
| #98 | Åben issue | Byg assistentvinduet: Ctrl+Win+Space, spørg og få streamet svar |
| #99 | Åben issue | Udgiv ingest-kontrakt for sidecar-skemaet med testværktøj |
| #100 | Åben issue | Lad assistenten svare ud fra egne møder med kildelinks |
| #101 | Åben issue | Tilføj begrænsede assistenthandlinger og webhook-udløserord |
| #102 | Åben issue | Tilføj AI-prompt- og oversættelsestilstand til diktering |
| #103 | Åben issue | Sæt komma efter danske regler i poleringen og vælg kommastil |
| #104 | Åben issue | Giv assistenten skærmkontekst: OCR, markeret tekst, udklipsholder |
| #105 | Åben issue | Byg lokal MCP-server så Claude Code og Codex kan bruge Tingskriver |
| #130 | Åben PR | Add a pure meeting detector with fixtures |
| #133 | Åben issue | Mål dansk Whisper-kvalitet og ressourceforbrug på Android |
| #134 | Åben issue | Tilføj valgfrit modeldownload med checksum og atomisk aktivering |
| #136 | Åben issue | Forbind lokal Whisper til Androids STT-interface |
| #137 | Åben issue | Bind tekstindsættelse til det oprindelige felt og session |
| #138 | Åben issue | Udtræk dansk tekstoprydning til en lille UniFFI-bro |
| #139 | Åben issue | Tilføj en lokal personlig ordbog uden transskripthistorik |
| #140 | Åben issue | Verificér Android-IME på enheder og dokumentér tilgængelighed |
| #151 | Åben PR | Make core HTTP asynchronous and bound blocking I/O |
| #152 | Åben PR | Bind text insertion to the original field and session on Android |
| #168 | Åben PR | Add MCP server for meetings with access policy |
| #169 | Åben issue | Revider funktioner uden synlighed i interface, indstillinger, opdatering og dokumentation |
| #170 | Åben issue | Vis automatisk opdatering i appen: tjek, ændringslog, installér og genstart (Velopack) |
| #171 | Åben issue | Byg Om-siden i GPUI: version, build, licenser og opdateringsstatus |
| #1 | Lukket issue | Opdel repoet i Cargo-workspace og udtræk tingskriver-core |
| #2 | Lukket issue | Sæt CI op for workspacet: Windows-build, tests og xwin-tjek |
| #3 | Lukket issue | Mål baseline for opstart, RAM og dikteringstid på Windows |
| #4 | Lukket issue | Gør optagelsen uafhængig af transskription og VAD |
| #5 | Lukket issue | Returnér DB-fejl ved opstart i stedet for at crashe |
| #8 | Lukket issue | Bevar slutningen af optagelsen ved stop |
| #10 | Lukket issue | Byg pille-overlayet som native Win32-lagvindue |
| #11 | Lukket issue | Indsæt pålideligt: vent på udklipsholderen, fald tilbage til SendInput |
| #12 | Lukket issue | Mål og log dikteringstid pr. trin, og vis medianen |
| #13 | Lukket issue | Saml dikteringsindstillinger i tilstande, der skifter pr. program |
| #15 | Lukket issue | Gem egne noter med tidsstempel pr. afsnit og migrér gamle noter |
| #16 | Lukket issue | Mål den indbyggede lokale models hastighed og danske kvalitet |
| #17 | Lukket issue | Byg lydafspilning i core med søgning til tidskode |
| #18 | Lukket issue | Flet mikrofon- og systemtransskript til ét transskript med talere |
| #19 | Lukket issue | Byg fælles udbydermodel med nøgler, valg pr. funktion og reservekæde |
| #20 | Lukket issue | Gør llama-helper-protokollen robust |
| #21 | Lukket issue | Dæk mute-regressionen med beslutningstests og headset-protokol |
| #23 | Lukket issue | Klip stilhed væk med VAD før transskription |
| #24 | Lukket issue | Hold dikteringsmodellen varm og kør inferens uden global lås |
| #25 | Lukket issue | Byg GPUI-appskallen: vindue, navigation, tema og danske fonte |
| #26 | Lukket issue | Verificér lydfilen, før rå-sporene slettes |
| #27 | Lukket issue | Skriv e-mailadresser, webadresser og CVR-numre fra tale |
| #30 | Lukket issue | Byg dansk testsæt og WER-regression til transskriptionskæden |
| #31 | Lukket issue | Byg fuldtekstsøgning på tværs af møder med FTS5 |
| #32 | Lukket issue | Hent kalender fra privat ICS-adresse til titler og deltagere |
| #34 | Lukket issue | Tilføj panic-hook, crash-dumps og opstartsmarkører i loggen |
| #35 | Lukket issue | Begræns ventetid ved stop af enhedsovervågning |
| #36 | Lukket issue | Gør netværks- og fil-IO i core asynkron med tidsgrænser |
| #37 | Lukket issue | Byg selvdiagnose for typiske Windows-fejl i core |
| #38 | Lukket issue | Tilføj indeks på transcripts og eksplicitte SQLite-indstillinger |
| #39 | Lukket issue | Fjern mutex-poisoning i optage- og transskriptionsstien |
| #43 | Lukket issue | Byg projektsiden i site/ med skærmbilleder og privatliv i klartekst |
| #54 | Lukket issue | Find efterladte optagelser fra Rust ved opstart |
| #61 | Lukket issue | Stop releases ved forværret ydeevne i forhold til baseline |
| #65 | Lukket issue | Saml al brugertekst i et dansk strengkatalog og formatér dansk |
| #69 | Lukket issue | Byg måleværktøj til dansk nøjagtighed, svartid og pris pr. udbyder |
| #70 | Lukket issue | Byg aflevering af referat til mappe, webhook og SSH-mål |
| #71 | Lukket issue | Kør lokal transskription i en separat proces |
| #106 | Lukket PR | Add performance baseline script for Windows |
| #107 | Lukket PR | Split into Cargo workspace with tingskriver-core, GPUI app crate and CI |
| #108 | Lukket PR | Add native Win32 pill overlay |
| #109 | Lukket PR | Add GPUI app shell with navigation, theme and Danish fonts |
| #110 | Lukket PR | Make dictation insert reliably, measure each step and keep the model warm |
| #111 | Lukket PR | Wait for the status listener and show crash dumps only on Windows |
| #112 | Lukket PR | Keep old provider keys and choices intact through the migration |
| #113 | Lukket PR | Describe the track writer instead of the removed live mixer in CLAUDE.md |
| #114 | Lukket PR | Keep "en anden maj" as words instead of a date |
| #115 | Lukket PR | Remove the keyboard hook when the host stops |
| #116 | Lukket PR | Apply downloaded updates only in the sole instance |
| #117 | Lukket PR | Store notes with per-paragraph timestamps and migrate old notes |
| #118 | Lukket PR | Measure local model speed and Danish quality |
| #119 | Lukket PR | Add audio playback with seek to timecode in core |
| #120 | Lukket PR | Merge microphone and system transcripts into one with speakers |
| #121 | Lukket PR | Make the llama helper protocol robust to timeouts and restarts |
| #122 | Lukket PR | Cover the mute regression with decision tests and a headset protocol |
| #123 | Lukket PR | Trim silence with VAD before transcription |
| #124 | Lukket PR | Add a Danish test set and WER regression gate for transcription |
| #125 | Lukket PR | Add full-text search across meetings with FTS5 |
| #126 | Lukket PR | Read meeting titles and attendees from a private ICS calendar |
| #127 | Lukket PR | Add self-diagnosis for common Windows failures in core |
| #128 | Lukket PR | Block releases when performance regresses against baseline |
| #129 | Lukket PR | Collect all user-facing text in a Danish string catalog |
| #131 | Lukket PR | Remove duplicate proptest and quick-error entries after parallel merges |
| #132 | Lukket PR | Group dictation settings into per-application modes |
| #135 | Lukket issue | Optag begrænset PCM-lyd med tilladelse og annullering |
| #141 | Lukket PR | Add Android dictation keyboard research, spec and skeleton |
| #142 | Lukket PR | Document Hex and what Tingskriver can reuse |
| #143 | Lukket PR | Harden audio recovery and checkpoint cleanup |
| #144 | Lukket PR | Test enumeration recovery during microphone mute |
| #145 | Lukket PR | Implement bounded Android PCM capture and permission flow |
| #146 | Lukket PR | Recover orphaned recordings from disk at startup |
| #147 | Lukket PR | Add nightly synthetic recording soak and crash recovery checks |
| #148 | Lukket PR | Compare transcription providers with local measurement reports |
| #149 | Lukket PR | Revert "Recover orphaned recordings from disk at startup" |
| #150 | Lukket PR | Bound device monitor shutdown and drop Windows polling |
| #153 | Lukket PR | Add Android Whisper benchmark tooling and device protocol |
| #154 | Lukket PR | Recover recording and transcription locks after worker panics |
| #155 | Lukket PR | Add persistent transcription queue with bounded retries |
| #156 | Lukket PR | Recover orphaned recordings without losing truncated audio |
| #157 | Lukket PR | Index transcripts and paginate transcript loading |
| #158 | Lukket PR | Run local transcription in a separate process |
| #159 | Lukket PR | Sync both recording tracks to disk every second |
| #160 | Lukket PR | Remove analytics and fail stale pending summaries on startup |
| #161 | Lukket PR | Split long recordings with overlap and resumable checkpoints |
| #162 | Lukket PR | Add signed Android APK releases |
| #163 | Lukket PR | Release 0.5.0-dev.3 and move Tauri users to the GPUI app |
| #164 | Lukket PR | Move audio DSP off callbacks and use rolling error limits |
| #165 | Lukket PR | Skip published Velopack packages for non-release builds |
| #166 | Lukket PR | Deliver summaries to folders, webhooks and SSH targets |
| #167 | Lukket PR | Move remaining core file I/O onto blocking workers |
