# Enhedsskift og mute

Mute ændrer aldrig mikrofonens enhed. Stille frames og frames med kun nuller
viser stadig en levende strøm. Manglende buffere i mindst 2 sekunder eller en
strømfejl kan give failover. En fjernet enhed, Windows' ændrede standardenhed
og en tilbagevendt valgt enhed kan også give skift. Lydindhold afgør aldrig skift.

## Systemlyd følger den enhed, mødeprogrammet afspiller til

Teams og Zoom kan afspille til et headset, mens Windows' standardudgang er
højttalerne. En loopback på standardenheden er så stille, og fjernstemmerne
går tabt. Derfor spørger følgeren hvert 2. sekund Windows, hvilke udgange der
har en aktiv lydsession fra en anden proces end Tingskriver selv
(`windows_render_sessions.rs`). Beslutningen ligger i `render_follow.rs` og
er ren og testet:

- Er standardenheden aktiv, sker der intet skift. Standardenheden vinder
  også ved lighed (begge afspiller).
- Er standardenheden inaktiv og en anden enhed aktiv i to polls i træk, flytter
  systemlyden til den enhed. Et enkelt udsving giver intet skift.
- Tilbage til standardenheden kræver samme to bekræftelser, så en kort
  systemlyd på højttalerne ikke får loopbacken til at hoppe.
- En poll, hvor intet afspiller nogen steder, ændrer ingenting: en pause i
  mødet flytter ikke loopbacken.
- Har brugeren selv valgt en udgang til systemlyd, følges afspilningen ikke.
- Mikrofonen berøres aldrig af dette.

Den nye loopback åbnes, før den gamle lukkes. Kan den nye ikke åbnes, er den
gamle urørt, og enheden springes over, til Windows melder en ændring eller der
er gået 2 minutter. Hullet er normalt 0 ms; stilhedsfylderen (højst 1 s) bruges
ved de øvrige skift, fx når en enhed forsvinder. Hvert skift logges som `[DEVICE_FOLLOW]`:

- `[DEVICE_FOLLOW] Playback: <default> -> '…' (Windows default: '…', playing: …)`
- `[DEVICE_FOLLOW] Switching system audio: '…' -> '…' (meeting app plays on another device)`
- `[DEVICE_FOLLOW] system audio moved '…' -> '…' (…): new loopback opened before the old one was stopped, gap … ms, took … ms`

## Transporttype fra bus-enumeratoren, ikke fra navnet

Bufferens timeouts afhænger af, om en enhed er trådløs. Navnet er ikke en
pålidelig kilde: et Jabra-headset med USB-dongle (Link 390) hedder "Jabra", men
er USB. På Windows læses derfor enhedens adapter-sti via
`IDeviceTopology::GetConnector(0).GetDeviceIdConnectedTo()`
(`windows_device_topology.rs`), og første segment, bus-enumeratoren, afgør
typen (`device_transport.rs`):

| Enumerator | Type | Timeouts |
| --- | --- | --- |
| `bth…` (BTHENUM, BTHHFENUM, BTHLEENUM) | Bluetooth | 80–200 ms |
| `usb…` | USB | 20–50 ms |
| `hdaudio`, `intelaudio`, `pci`, `acpi` | indbygget | 20–50 ms |
| `swd`, `root` | virtuel | 20–50 ms |

Kan topologien ikke læses (enheden er væk, COM fejler, ukendt bus), bruges
navnet og buffer-heuristikken som før, og loggen siger det:
`Cannot read the bus of '…' from the device topology (…); falling back to the
device name`. `pipeline.rs` logger, hvilken bus der blev fundet, og hvilke
timeouts der følger af den.

Tingskriver bytter aldrig brugerens valgte eller Windows' standardmikrofon ud
for at undgå Bluetooth-hands-free: reglen er at følge Windows' standardenhed.

## Automatiske beslutningstests

`cargo test -p tingskriver-core --test device_decisions -- --nocapture` kører
14 datadrevne tidsforløb gennem `MicWatchdog` og `DeviceFollower`, herunder et
midlertidigt fejlet enhedsopslag under mute og efterfølgende genopretning. Samme tabel
køres mod en bevidst forkert variant, der erklærer strømmen fejlet efter fem
sekunders stilhed. Kontrollen kræver, at tabellen afslører denne regression.
Produktionslogikken er uændret.

`tingskriver_core::fake_device::FakeDevice` er offentlig og implementerer både
`DeviceSource` og `MicSignal`. `TimedEvent` angiver virtuel tid som `Duration`;
`advance_to` anvender hændelser i rækkefølge uden at vente. `Buffer::Silence`
og `Buffer::Zeros` er levende callbacks, mens fravær af bufferhændelser betyder,
at strømmen står stille. `Devices` modellerer fjernelse, genkomst og ændret
default. `StreamError` returneres én gang til kalderens fejlhåndtering.
`SnapshotError` varer indtil næste snapshot. Se modulets kørbare API-eksempel.
Fake'en ændrer ikke selv followerens tilstand eller genåbner strømme.

## Manuel Windows-protokol: 10 minutter

Denne prøve skal udføres på Mads' Windows-maskine med rigtigt headset.
Automatiske tests dokumenterer ikke optagelsens kontinuitet eller donglens
hardwareadfærd. Brug syntetisk tale og afspilning uden personoplysninger.

Dato: __________  Tester: __________
Build/commit: __________  Windows-version: __________
Headset og dongle/forbindelse: __________
Windows standardmikrofon: __________  Standardhøjttaler: __________
Fallback-mikrofon og højttaler: __________
Logfil/optagelsens lokale placering: __________

1. Åbn loggen med info-niveau for `tingskriver_core::audio`. Vælg at følge
   Windows' standardenheder. Sørg for, at en indbygget mikrofon og højttaler
   er tilgængelige som fallback. Kontrollér, at headsettet er standard.
2. Start en optagelse med både mikrofonspor og systemspor. Start samtidig
   et stopur på 10 minutter. Afspil en vedvarende testlyd på PC'en, og sig
   minut og sekund højt mindst hvert sekund uden for muteintervallet.
3. Ved 01:00: sluk eller afbryd headsettet, så endpointet faktisk fjernes.
   Bekræft skift til fallback. Ved 01:30: tænd/tilslut det igen, og bekræft
   at Windows vælger det som standard, og at optagelsen følger efter.
4. Ved 03:00: mute headsettets mikrofon med hardwareknappen. Lad det være
   tilsluttet. Rør ikke Windows' standardenhed under dette interval.
   Ved 04:00: unmute og tal igen. Notér samtlige skiftehændelser mellem
   03:00 og 04:00; det krævede antal er **0**.
5. Ved 06:00: sluk/afbryd headsettet anden gang. Ved 06:30: tilslut igen,
   bekræft Windows-default og tilbagevenden til headsettet. Fortsæt tale
   og systemlyd til 10:00, og stop så optagelsen.
6. Vent på færdigbehandling. Kontrollér både mikrofon- og systemsporet
   separat fra start til slut og ved begge afbrydelser/genforbindelser.
   Sammenhold varighed og tidsmarkører med stopuret og loggen. Kontrollér
   de færdige filer, ikke kun den blandede afspilning.

### Logbeskeder og bedømmelse

Søg efter disse faktiske tekstdele (enhedsnavne og tider varierer):

- `[DEVICE_FOLLOW] Microphone '…' delivers only silent frames for … ms (muted or quiet); silent frames ignored, no switch`
- `[DEVICE_FOLLOW] Microphone '…' delivers sound again`
- `[DEVICE_FOLLOW] Switching …: '…' -> '…' (…)`
- `microphone switched '…' -> '…' (…), gap filled with silence, took … ms`
- `system audio switched '…' -> '…' (…), gap filled with silence, took … ms`
- `delivered no frames for … ms -> failover` og `FAILED after` er fejltegn,
  som altid noteres med tidspunkt. Under mute må de ikke give skift.

Fravær af en skiftelinje er ikke alene tilstrækkeligt. Kontrollér, at strømmen
fortsat leverer, at loggen dækker hele prøven, og at tale vender tilbage efter
unmute. Loggens `gap filled with silence` beviser ikke, at der ikke mangler lyd.
Mål huller ved hjælp af testlyd/tidsmarkører i hvert spor.

| Kontrol | Måling/resultat | Bestået? |
| --- | --- | --- |
| Samlet optaget tid, cirka 10 minutter | | |
| Første afbrydelse og genforbindelse, begge spor | | |
| Mute 03:00–04:00: antal skift = 0 | | |
| Tale vender tilbage efter unmute | | |
| Anden afbrydelse og genforbindelse, begge spor | | |
| Største hul i mikrofonspor ≤ 2 s, planlagt mute undtaget | | |
| Største hul i systemspor ≤ 2 s | | |
| Mikrofonspor komplet fra start til slut | | |
| Systemspor komplet fra start til slut | | |
| Fejl/failover i loggen, med tidspunkt og årsag | | |

Prøven består kun med **0 skift under mute**, **ingen uventede huller over
2 sekunder** og **begge spor komplette**. Muteintervallets tilsigtede stilhed
er ikke et hul. Hvis donglen fortsætter med at levere nuller efter slukning,
notér det som hardwarebegrænsning og prøvens resultat; det må ikke løses ved
at bruge lydindhold som fejlsignal. Issue #21 kan først lukkes, når prøven
også er dokumenteret som bestået her.

Bemærkninger og dokumentation: __________

## Manuel testplan: afspilningsenhed og transporttype (#199 og #182)

Kræver Windows og rigtige enheder. Syntetisk tale og testlyd uden
personoplysninger. Notér build/commit og Windows-version.

### A. Teams på headset, standard = højttalere (#199)

1. Sæt Windows' standardudgang til højttalerne. Åbn Teams (eller Zoom), og vælg
   headsettet som højttaler i mødeprogrammets egne indstillinger.
2. Åbn loggen på info-niveau for `tingskriver_core::audio`. Start en optagelse
   med mikrofon og systemlyd.
3. Afspil testlyd til Teams-headsettet (fx et testopkald eller en lyd i et
   møde). Vent mindst 6 sekunder.
4. Bekræft i loggen `[DEVICE_FOLLOW] Playback: <default> -> 'Headset …'` og en
   `moved … gap … ms`-linje. Gap skal være under 1000 ms.
5. Stop optagelsen. Systemsporet skal indeholde testlyden fra headsettet,
   også tiden før skiftet, hvis lyden allerede spillede (første ca. 4 s kan
   mangle: to polls á 2 s).
6. Afspil lyd til højttalerne (standard) samtidig med headsettet. Efter to
   polls skal loggen vise skift tilbage til standard (standard vinder).
7. Vælg en bestemt udgang til systemlyd i optagelsen, og gentag trin 3: der
   må ikke komme `Playback:`-skift.

| Kontrol | Resultat | Bestået? |
| --- | --- | --- |
| Fjerntale fra Teams-headsettet i systemsporet | | |
| Loggen viser `Playback:` og `moved … gap … ms` | | |
| Største hul i systemsporet ved skiftet ≤ 1 s | | |
| Standard aktiv giver intet skift | | |
| Valgt udgang følges ikke | | |

### B. Transporttype (#182)

For hver enhed: start en optagelse, og find linjen `bus …` fra `pipeline.rs`
og `Bus of '…' is …` fra enhedsdetektionen.

| Enhed | Forventet bus | Forventede timeouts | Fundet | Bestået? |
| --- | --- | --- | --- | --- |
| Bluetooth-headset (A2DP og hands-free) | Bluetooth | 80–200 ms | | |
| USB-headset | USB | 20–50 ms | | |
| Headset med USB-dongle, navn med "Jabra" | USB, ikke Bluetooth | 20–50 ms | | |
| Indbygget mikrofon/højttaler | built-in | 20–50 ms | | |
| Virtuel kabel (fx VB-Cable) | virtual | 20–50 ms | | |

Fald-tilbage: afbryd en enhed lige efter optagelsens start, så topologien ikke
kan læses; loggen skal sige `falling back to the device name`.
Brugerens valgte eller standardmikrofon må aldrig blive byttet ud.

### C. Hot-swap

Gentag protokollen "Manuel Windows-protokol: 10 minutter" ovenfor med et
Bluetooth-headset og bagefter et USB-headset. Under afbrydelsen skal systemsporets
hul være højst 1 s, og følgeren må ikke melde `FAILED`.
