# Enhedsskift og mute

Mute ændrer aldrig mikrofonens enhed. Stille frames og frames med kun nuller
viser stadig en levende strøm. Manglende buffere i mindst 2 sekunder eller en
strømfejl kan give failover. En fjernet enhed, Windows' ændrede standardenhed
og en tilbagevendt valgt enhed kan også give skift. Lydindhold afgør aldrig skift.

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
