# Kalender fra privat ICS-adresse

`tingskriver_core::calendar` er en UI-fri kalenderklient. Den har ingen
afhængighed af mødedetektoren. Parser, gentagelser og matchning er rene
funktioner, som kan testes på Linux med syntetiske data.

## Brug

Opret `IcsAddress::new(address)`, kald `save()` én gang ved brugerens valg,
og brug derefter `IcsAddress::load()`. `delete()` fjerner legitimationsposten.
Opret klienten med `Calendar::open(address, config).await`, og kald
`refresh_if_due(now).await` fra værtens runtime. Kaldet tager falsk tid i tests;
der oprettes ingen skjult baggrundsløkke. Værten kan bruge `runtime::spawn`
til en lille løkke med ventetid og eksplicit nedlukning.

`CalendarConfig` indeholder kun `timezone` (standard `Europe/Copenhagen`),
`own_addresses` (standard tom) og `refresh_minutes` (standard 10, begrænses
til 5–15). Ukendte JSON-felter afvises. Der er intet felt til ICS-adressen.

`events_around(now)` returnerer begivenheder, som overlapper intervallet
fra et døgn før til et døgn efter `now`. `parse(ics, config, from, to)` kan
bruges direkte til andre tidsvinduer. Sluttiden er eksklusiv; en tidsbestemt
begivenhed uden DTEND/DURATION har nul varighed.

`match_meeting(now, started_at, &candidates)` returnerer en kopi af den
bedst matchende begivenhed med titel, deltagernes navn/mail, start og forventet
slut. Kandidaten skal fortsat være aktiv, eller starte inden for ti minutter.
Startafstanden afgør rangeringen; heldagsaftaler får et tillæg på ét døgn.
Tom deltagerliste er gyldig. Start, UID og titel bryder pointlighed.
Et møde, hvis observerede start ligger efter `now`, giver intet match.

## ICS-understøttelse

- VEVENT: UID, SUMMARY, ATTENDEE med CN/PARTSTAT, DTSTART, DTEND, DURATION,
  STATUS, RECURRENCE-ID, EXDATE og RDATE. Linjefoldning med mellemrum/tab,
  CRLF/LF, citerede parametre og escapede teksttegn understøttes.
- UTC (`Z`), IANA TZID, flydende tid i konfigurationens zone og
  `VALUE=DATE`. Heldagsaftaler uden slut varer én lokal dag. DTEND for
  heldagsaftaler er eksklusiv. Dagbaserede varigheder bevarer lokal tid
  hen over sommertid.
- DURATION skelner mellem nominelle dage/uger og forløbne timer/minutter/
  sekunder: P1D over forårsskiftet kan være 23 timer, PT24H er altid 24 timer.
  Gentagne tidsbestemte aftaler med DTEND bevarer den oprindelige, præcise
  varighed. DTEND og DURATION må ikke optræde sammen.
- RRULE: DAILY, WEEKLY, MONTHLY og YEARLY; INTERVAL, COUNT, inklusiv UNTIL,
  BYDAY (og positive/negative ordinaler for måned/år), BYMONTHDAY (og negative
  værdier), BYMONTH og WKST. Ugyldige datoer og ikke-eksisterende lokale
  gentagelsestider springes over uden at bruge COUNT. Ved efterårets dobbelte
  klokkeslæt vælges den første forekomst. Gentagelser beregnes i lokal tid,
  så 09:00 forbliver 09:00. DTSTART er første forekomst i sættet.
- EXDATE trækkes fra sættet; RDATE tilføjes. Begge kan have en anden zone
  end DTSTART. RECURRENCE-ID erstatter én forekomst, også når den flyttes
  ind eller ud af tidsvinduet. Aflyste overrides kan udelade DTSTART.
- Kendte VTIMEZONE/TZID bruger `chrono-tz`'s indbyggede IANA-database.
  Windows-navnene W. Europe, Romance, Central Europe og Central European
  Standard Time bruger Europe/Copenhagen; GMT Standard Time bruger London,
  Eastern Standard Time New York og Pacific Standard Time Los Angeles.
  UTC understøttes også. Det er en dokumenteret navnemapping, ikke hele
  Windows-zonelisten. For ukendte TZID læses VTIMEZONE med STANDARD/DAYLIGHT,
  DTSTART, TZOFFSETFROM/TO, RDATE og ovenstående RRULE-delmængde. Kendte navne
  følger IANA, selv hvis en medsendt definition er forældet.

Eksplicitte UTC-tidspunkter i RDATE bevares også ved efterårets dobbelte
klokkeslæt; EXDATE og RECURRENCE-ID sammenlignes som UTC-tidspunkter.

STATUS:CANCELLED filtreres altid fra. PARTSTAT=DECLINED filtreres kun fra,
når den tilhørende mailadresse findes i `own_addresses` (uden forskel på
store/små bogstaver). Andres afvisning skjuler ikke mødet. Uden egen
identitet kan afvisning ikke afgøres; mødet beholdes. Cachen bevarer
afvisningsmetadata, så et ændret `own_addresses` også gælder offline.

Ukendte almindelige felter, VTODO, VALARM, vedhæftninger og andre felter
ignoreres (URL, LOCATION, DESCRIPTION og ORGANIZER læses, se næste afsnit). Ukendt tidszone, ugyldige data, EXRULE,
flere RRULE per event, RECURRENCE-ID med RANGE, BYSETPOS, BYHOUR/BYMINUTE,
ugenumre og frekvenser under én dag giver en generisk parserfejl.
Dublerede master-UID'er og dublerede overrides afvises; klienten vælger ikke
selv mellem forskellige SEQUENCE-versioner. Ikke-eksisterende eksplicitte
start/sluttider giver fejl. Parseren accepterer højst 8 MiB og højst 200.000
undersøgte kalenderdage per gentagelsesregel (cirka 547 år).

## Start af planlagte møder (#201)

Parseren udleder pr. begivenhed `meeting_link` (første https-link til en kendt
platform i URL, X-GOOGLE-CONFERENCE, X-MICROSOFT-SKYPETEAMSMEETINGURL, LOCATION,
DESCRIPTION; Teams, Zoom, Google Meet, Webex, GoToMeeting, Jitsi, Whereby; et
vilkårligt link, http og link med brugerinfo tæller ikke), `own_status`
(`Organizer` fra ORGANIZER, ellers PARTSTAT for en ATTENDEE i `own_addresses`;
uden PARTSTAT `NeedsAction`; ingen træf eller ingen `own_addresses` giver
`Unknown`), `Attendee::is_self`, `recurring` og `original_start`
(RECURRENCE-ID, så en flyttet forekomst bevarer sin identitet). Felterne er
nye i cachen uden versionsbump: en gammel cache læses med `Unknown`/intet link,
så intet starter, før næste hentning har fyldt dem ud.

`should_auto_start(event, now, mic_holds, ignored)` er en ren funktion og
returnerer `Start { early }` eller `Skip(årsag)`. Reglerne, i rækkefølge:
ignoreret; heldags; intet møde-link; egen status ikke arrangør/accepteret
(afvist, måske, ubesvaret og ukendt starter ikke); ingen andre deltagere eller
alle andre har afvist; `now` skal ligge fra starten til 5 minutter efter
(`GRACE`) og før slut. Før starten tæller kun en tidlig deltagelse: en
mikrofon, som en anden proces har holdt siden højst 10 minutter før starten
(`EARLY_JOIN_WINDOW`). Er mikrofonen holdt længere, er det forrige opkald, og
et back-to-back-møde starter ikke tidligt.

`auto_start_action(mode, decision, recording_active)` oversætter til
`Record`, `Ask` eller `Nothing` efter indstillingen `AutoRecordMode`
(`record`/`ask`/`off`, standard `ask`). Intet starter eller spørges, mens en
anden optagelse kører, og `off` giver aldrig noget. `should_remind` følger
samme begivenhedsregler i de 5 minutter før starten (`REMINDER_LEAD`) og er
falsk ved `off`.

`IgnoredEvents` gemmer "Ignorér denne begivenhed" (UID + oprindelig start) og
"Ignorér serien" (UID) i `calendar/ignored.json` (0600 på Unix, atomisk
skrivning). Begge dele stopper påmindelse og start. Enkelte forekomster
glemmes efter 400 dage (`prune`), serier først ved `unignore_series`. En fil,
der ikke kan læses eller tolkes, giver fejl og ikke en tom liste; værten må
da ikke starte automatisk. Et skiftet UID er en ny begivenhed og er ikke
ignoreret.

Bevidst ikke med: anarlogs krav om et snapshot på højst 3 minutter, fordi
opdateringsintervallet er 5 til 15 minutter; værten bør hente kalenderen lige
før en beslutning. `AutoRecordMode` er defineret her, fordi #59 ikke ligger på
denne gren; den skal samles med #59's indstilling ved merge. Der er endnu
ingen UI, ingen timer og ingen kobling til promptvinduet og
`recording_commands`; se PR-beskrivelsen.

## Hemmelighed og hentning

Adressen er en adgangsnøgle. Den gemmes kun via de eksisterende offentlige
`dictation::secrets::{read, write, delete}` med målet
`Tingskriver/calendar-ics`. Windows bruger Credential Manager. Andre
platforme følger den eksisterende secrets-implementering: skrivning afvises,
læsning giver ingen nøgle. `IcsAddress` implementerer ikke Serialize;
Debug og Display viser kun `[hidden]`. Begivenheder og deltagere har heller
ingen automatisk Debug-udskrift.

HTTP er asynkron reqwest med højst ti sekunders samlet request-timeout og
begrænset body-størrelse. Der bruges ingen blocking-klient. Kun HTTP/HTTPS
uden brugerinfo accepteres. Brug HTTPS i produktion; HTTP findes til lokale
tests. Redirects følges ikke, så en privat adresse ikke videresendes til
en anden vært. Fejl indeholder aldrig reqwest-fejlen, response-body eller
adressen, kun en statisk fejlvariant. Der logges kun, at opdatering fejlede
og den lokale cache bruges, aldrig begivenhedstitler eller deltagere.

ETag sendes som If-None-Match. 304 genbruger cachen og opdaterer tidspunktet.
Når cachevinduets slut er under 30 dage væk, hentes hele feedet igen, selv
om det er uændret. Fejl, timeout, ugyldig ICS og utilgængelig server giver
`RefreshOutcome::Cached`, ikke en brugerfejl. Også mislykkede forsøg er
underlagt opdateringsintervallet. Ved ur, der går baglæns, forsøges igen.

## Cacheformat og privatliv

`paths::app_data_dir()/calendar/events.json` er JSON version 2 med parserens `timezone`, `events`,
`etag`, `fetched_at`, `range_start` og `range_end`. Begivenheder indeholder
UID, titel, deltagere (navn/mail/declined), UTC-start/slut og all_day.
Der gemmes hverken rå ICS eller ICS-adresse i filen. Ved ændring af
konfigurationens tidszone kasseres cachen, og feedet hentes uden ETag.
Version 1 kasseres også, da den ikke gemte parserens tidszone.
Den indeholder kalenderens personlige mødeoplysninger og bør behandles som
andre lokale mødedata. På Unix oprettes filen med mode 0600.

Opdatering skriver til en unik fil i samme mappe, synkroniserer filen og
omdøber atomisk over den tidligere fil. Ved fejl beholdes den tidligere
diskcache; nye begivenheder kan fortsat bruges fra hukommelsen. Cacheindlæsning
og skrivning bruger tokio-filsystemet. Ukendt cacheversion eller beskadiget
JSON giver en tom hukommelsescache og et nyt henteforsøg.

Hver fuld hentning udvider perioden fra 30 dage før til 366 dage efter
hentetidspunktet. Offline kan kun dette gemte vindue vises; efter længere
end cirka et år offline kræves en ny hentning. Værten skal fjerne den gamle
cache ved skift til en anden kalender eller ved brugerens valg om at slette
kalenderdata; sletning af legitimationsposten alene sletter ikke mødedata.

## Verifikation

`cargo test -p tingskriver-core --test calendar --test calendar_fetch --test calendar_matching --test calendar_autostart`
bruger kun håndskrevne fixtures med fiktive identiteter og TCP på localhost.
Der testes Google-/Outlook-formater, tidszoner og begge danske sommertidsskift,
gentagelser og undtagelser, afvisning, matchning, timeout, 304, genåbning
offline, hemmelighedsredaktion og indfanget logoutput.

På Windows: gem en privat testkalender via `IcsAddress::save`, luk processen,
og bekræft at `load` læser den efter genstart. Hent, ændr testkalenderen,
vent ti minutter og hent igen, så eksisterende cache overskrives. Afbryd
netværket, genstart, og bekræft at de samme begivenheder vises. Kald `delete`,
og bekræft at `load` giver None. Kontrollér Credential Manager-målet og at
konfiguration/log ikke indeholder den private adresse. Ingen UI til dette
er del af issue #32; brug en lokal core-harness uden at udskrive adressen.
