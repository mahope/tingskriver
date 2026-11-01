# Mødedetektor

`tingskriver_core::meeting_detect::detect(&Snapshot, &DetectorState)` returnerer
`(Decision, DetectorState)` uden ur, I/O, logning, netværk eller platforms-API'er.
Alle input, output og tilstande kan serialiseres med serde. Kalenderens type er
selvstændig og afhænger ikke af ICS-modulet. Windows-indsamling hører til #59.

## Input og tid

Alle tider er UTC Unix-sekunder (`i64`). Kald med stigende eller samme `now`,
bevar den returnerede tilstand, og lever det aktuelle optagelsesflag fra værten.
Et ældre snapshot giver `clock_moved_backwards` og ændrer ikke tilstanden.
`started_at` skal være starten på det sammenhængende mikrofonbrug, ikke tidspunktet
for seneste polling. Fremtidige mikrofonstarter tæller ikke som stabil brug.
`last_speech_at` er seneste tale i den aktuelle optagelse, ikke global diktering.
Vinduer og mikrofonbrugere er komplette øjebliksbilleder, ikke ændringshændelser.

`is_browser` skelner en browserfane fra et selvstændigt app-/PWA-vindue. En Meet-PWA
kan derfor bruge `chrome.exe` eller `msedge.exe` med `is_browser = false`.
Procesnavne er filnavne uden sti og sammenlignes uden forskel på store/små bogstaver.
Ingen proces-id'er er tilgængelige: flere instanser af samme browser kan ikke
adskilles. Titler alene kan heller ikke bevise et opkald. Den konservative
browserbeslutning er derfor et forslag uden kalenderstøtte.

## Startregler

- Der kræves både et genkendt mødevindue og mindst 5 sekunders sammenhængende
  mikrofonbrug fra samme procesnavn. Zoom og CptHost accepteres som et procespar.
- Teams-app: `ms-teams.exe`, `Teams.exe`. Titlen slutter på
  ` | Microsoft Teams`; chat, aktivitet og kalender på dansk/engelsk frasorteres.
- Zoom-app: `Zoom.exe`, `CptHost.exe`. Titlen er `Zoom Meeting`, `Zoom-møde`
  eller en af disse efterfulgt af ` - ` og syntetisk/virkelig mødetekst.
- Meet-app: `Google Meet.exe`, `Meet.exe` eller et browser-ejet PWA-vindue.
  Titlen begynder med `Meet - xxx-xxxx-xxx` (bogstavkode) eller `Møde - `.
- Browsere: `chrome.exe`, `msedge.exe`, `firefox.exe`, `brave.exe`, `opera.exe`,
  `vivaldi.exe`. Samme titelmønstre bruges til de tre browserkilder.
- Native app + stabil mikrofon + mødevindue giver høj sikkerhed. En browser giver
  middel sikkerhed. En kalenderaftale med `start <= now < end` og `start < end`
  hæver til høj; kalender alene giver aldrig start. Titel og deltagerantal bruges
  ikke til match. Ved overlap vælges den tidligste slutdato deterministisk.
- Forgrundsvindue er et ekstra årsagssignal og bruges som prioritet ved flere
  kandidater. Høj sikkerhed prioriteres først, derefter forgrund, derefter den
  faste `Source`-rækkefølge, seneste mikrofonstart og procesnavn som sidste
  lighedsbryder. Inputrækkefølgen er uden betydning.
- `start_modes` vælger pr. kilde `automatic` (standard), `ask` eller `off`.
  Automatic starter ved høj sikkerhed og foreslår ved middel. Ask foreslår altid;
  off giver ingen start. En eksisterende optagelse får ingen ny startbeslutning.
  Værten udfører handlinger; den rene funktion starter ingen optagelse selv.

Discord, ukendte processer og Tingskrivers egen proces er ikke mødekilder.
`dictation_active` blokerer ikke et samtidigt ægte møde, men egen mikrofonbrug
kan aldrig opfylde et mødes procesmatch. Korte mikrofonudbrud, vinduer uden
mikrofon, YouTube/Netflix, Teams-chat og Meet/Zoom-forsider giver ingen start.
Årsager og signaler er faste enums uden vinduestitler eller mødenavne.
Snapshottene indeholder titler og bør derfor ikke logges.

## Stopregler

Kun en observeret mødetilstand med `recording_active = true` kan give stop.
En manuelt startet optagelse uden observeret mødekilde stoppes ikke automatisk.

1. Når både mødekildens mikrofonbrug og dens mødevindue er væk, starter en timer
   ved første fraværende snapshot. Ved mindst 30 sekunder gives `signals_lost`.
   Et af signalerne er nok til at nulstille timeren (fx mute med mødevindue åbent).
   Mikrofon eller vindue fra en anden kilde holder ikke det gamle møde i live.
2. Den aktive kalenderaftales slutdato fastholdes, også hvis kalenderinputtet
   senere bliver tomt. Stop tidligst ved slut + 300 sekunder, og kun hvis der
   ikke har været tale i mindst 300 sekunder. Uden tale bruges tidspunktet for
   første observation som nederste grænse. Fremtidig tale blokerer denne regel.
3. Et nyt stabilt mikrofonforløb eller en ny kilde med matchende vindue erstatter
   den gamle tilstand før stopvurderingen. Det arver ikke den gamle kalender.
   Stop rydder tilstanden; næste snapshot kan identificere et nyt møde.

Stop vurderes også ved `off`, da indstillingen styrer start og ikke en allerede
igangværende optagelse. Gentagne startforslag gentages indtil værten melder
`recording_active`; værten håndterer eventuel afvisning/notifikationsfrekvens.
Efter stop huskes mikrofonforløbets kilde, proces og starttid. Samme forløb
giver `meeting_already_stopped`, så kalenderstop ikke straks genstarter mødet.
Et nyt forløb kan starte normalt. Værten bør behandle stop før næste snapshot.

## Verifikation

`cargo test -p tingskriver-core meeting_detect --lib` indlæser alle JSON-filer i
`tests/fixtures/meeting_detect/`. Testen kræver mindst 30 filer, mindst ét trin
pr. fil, vellykket parsing og præcis beslutning pr. trin. Hvert kald gentages
med samme input for at kontrollere determinisme, og tilstanden serde-rundtures.
De 55 fixtures indeholder 85 snapshots. Alle fixturetitler er syntetiske.
Tidsforløbene bruger falsk tid, herunder
mikrofonens femsekundersgrænse, 29/30/31 sekunders fravær, kalenderfrist,
ny tale og et nyt møde umiddelbart efter.

Windows-kontrol efter integration med #59: åbn én reel samtale pr. kilde,
bekræft procesnavn, PWA-flag og mikrofonstart; afprøv derefter chat, forside,
medieafspilning og diktering. Luk opkaldet og kontrollér stop efter 30 sekunder;
behold et mutet opkaldsvindue åbent og kontrollér at det ikke udløser fraværsstop.
Dette modul alene ændrer ingen synlig adfærd i appen.
