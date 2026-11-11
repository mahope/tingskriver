# Tidsfrister og CPU-genforsøg for den lokale STT-proces

Whisper, Parakeet og Silero-VAD kører i `tingskriver-stt`, en egen proces
(se [arkitektur.md](arkitektur.md)). Et nedbrud i den proces bliver opdaget
med det samme. Et hang gør ikke, og før denne ændring ventede klienten
(`crates/tingskriver-core/src/stt/client.rs`) uden tidsfrist, så en diktering
kunne fryse uden en fejlbesked.

## Frister

Hvert kald til workeren får fristen `max(gulv, k × lydlængde)`. Lydlængden er
antal samples / 16.000. Når fristen er gået, dræbes workeren, kaldet fejler med
en dansk besked (`Den lokale transskription tog for lang tid og blev afbrudt.
Prøv igen.`), og næste kald starter en frisk proces.

| Enhed | Indlæsning af model | Kørsel, gulv | k |
|-------|--------------------|--------------|---|
| GPU   | 120 s              | 60 s         | 10 |
| CPU   | 120 s              | 300 s        | 20 |

Begrundelse:

- **Gulv:** en kold start af en model (indlæsning af vægte, shader-kompilering
  på Vulkan) og det første kald er meget langsommere end resten. Gulvene
  forhindrer, at korte klip giver falske timeouts.
- **k × lydlængde:** en lang optagelse må gerne tage længere tid. Et realistisk
  tal er langt under 1 × lydlængden på GPU og omkring 1–3 × på en svag CPU. k
  er med vilje 10 og 20: fristen skal fange et *hang*, ikke en langsom maskine,
  så en falsk timeout er værre end at vente lidt for længe.
- **Indlæsning på GPU er 120 s, ikke Handys 30 s.** Et timeout under
  indlæsning slår GPU'en fra resten af sessionen, og en kold Vulkan-shader-
  kompilering eller en stor model fra en langsom disk kan overstige 30 s uden
  at noget hænger. Der er ikke målt for vores modeller endnu; sænk først
  tallet, når en måling findes.
- **CPU er mere lempelig end GPU**, fordi en CPU uden AVX2 eller med få kerner
  kan være mange gange langsommere, og fordi GPU'en har et reelt alternativ
  (CPU'en) at falde tilbage til.
- Fristen er øverst begrænset til 24 timer, så en meget stor lydlængde ikke
  giver en ugyldig varighed.

Handy (MIT, CJ Pais) er inspiration til tidsfristerne. Tabellen ovenfor er
Tingskrivers egne værdier og følger ikke Handys tabel; især er GPU-indlæsningens
frist hævet til 120 s. Der er ikke kopieret kode.

## CPU-genforsøg

- Hænger eller falder en GPU-worker (ved start, indlæsning eller kørsel),
  eller afviser den initialiseringen (fx driverfejl eller utilstrækkelig VRAM),
  prøves *det samme kald* én gang i en ren CPU-worker. GPU'en markeres
  utilgængelig, og resten af sessionen starter alle nye workere på CPU
  (`stt::gpu_unavailable()`). Flaget nulstilles ved genstart af appen.
- En worker, der slet ikke kan startes (binæren mangler eller er blokeret af
  fx antivirus), er ikke GPU'ens skyld: der er intet CPU-forsøg, og GPU'en
  markeres ikke.
- En fejl i et almindeligt svar efter initialiseringen udløser ikke CPU-forsøg.
  Et forkert request-id er en protokolfejl: workeren kasseres, men GPU'en
  markeres ikke utilgængelig, og der er intet CPU-forsøg.
- Er enheden valgt eksplicit (`DevicePolicy::Gpu`; endnu ikke koblet til en
  indstilling i appen, kun bevist i klientens tests), falder den aldrig tilbage.
  Fejlen vises, og næste kald prøver GPU'en igen.
- Kun Whisper bruger GPU'en. Parakeet og VAD kører med `DevicePolicy::Cpu`,
  så de får CPU-fristerne og intet genforsøg.
- Workeren får enheden i miljøvariablen `TINGSKRIVER_STT_DEVICE` (`gpu` eller
  `cpu`). `cpu` slår GPU fra i Whisper-konteksten, ligesom Diagnose-reglen
  `cpu_required()`.

## Vulkans implicitte lag (kun Windows, kun GPU-workeren)

Overlays og optageprogrammer (OBS' virtuelle kamera, spiloverlays, GPU-
værktøjer) installerer *implicitte* Vulkan-lag, som Vulkan-loaderen indlæser i
alle processer. De kan få GPU-initialiseringen til at gå ned.

- Workerens miljø får `VK_LOADER_LAYERS_DISABLE=~implicit~`. `~implicit~` er
  loaderens eget mønster for alle implicitte lag. Eksplicitte lag (fx
  valideringslag, som en udvikler selv slår til) berøres ikke.
- Det sættes kun i *workerens* `Command`, aldrig i appens egen proces, og kun
  for en GPU-worker på Windows. CPU-workeren bruger ikke Vulkan.
- Har brugeren allerede sat `VK_LOADER_LAYERS_DISABLE`, bevares værdien.
- `TINGSKRIVER_KEEP_VULKAN_IMPLICIT_LAYERS=1` slår politikken fra.
- Politikken logges én gang pr. session (`STT worker: Vulkan implicit layers …`).
- Vulkan-loaderen skal være 1.3.234 eller nyere for at kende variablen; ældre
  loadere ignorerer den, og intet går i stykker.

## Fejlinjektion i tests

Debug-builds af workeren læser `TINGSKRIVER_STT_FAULT=<abort|segv|hang>@<init|load|run>`,
valgfrit efterfulgt af `:gpu` (så fejler kun en GPU-worker, og et CPU-genforsøg
kan vises at virke).

- `init`: ved processtart, før den læser noget.
- `load`: ved første request, altså modelindlæsningen.
- `run`: ved alle senere requests.
- `segv` opfører sig som `abort` (en unormal afslutning uden dump); det
  er ikke et rigtigt hukommelsesbrud.

Release-builds ignorerer variablen. Klientens enhedstests bruger en fake
arbejder (`stt/client.rs`, `tests`) med de samme fejl, så frister og genforsøg
bevises uden en rigtig model.

## Manuel test (Windows)

1. Start Tingskriver fra en debug-build med `TINGSKRIVER_STT_FAULT=hang@run`.
2. Diktér. Lokal diktering bruger Parakeet med ORT på CPU. Kørslens frist er
   `max(300 s, 20 × lydlængden)`. Derefter vises den danske fejl i pillen;
   optagelse og genvej virker hele tiden. Der er intet GPU-til-CPU-skift og
   intet genforsøg. Næste diktering starter en frisk CPU-worker.
3. Test separat Whisper under `Auto` på en maskine, hvor GPU vælges. Med
   `hang@run:gpu` lykkes CPU-genforsøget efter GPU-fristen
   `max(60 s, 10 × lydlængden)`, plus op til 120 s til CPU-indlæsning og
   CPU-kørslens `max(300 s, 20 × lydlængden)`. Hvis begge workere hænger under
   kørsel (`hang@run`), er disse frister tilsammen værste tilfælde fra en
   allerede indlæst GPU-worker: 480 s for korte klip. Fra en kold start
   kommer op til 120 s til GPU-indlæsning oveni, i alt 600 s for korte klip.
   Fristerne er ikke et løfte om præcis vægurtid; processtart og oprydning
   kommer oveni. Loggen viser `using the CPU for the rest of the session`.
4. Gentag Whisper-testen med `abort@load:gpu`: CPU-workeren skal overtage.
   OBS-testen fra #214 er manuel: kontrollér på Windows med OBS, at
   GPU-workeren får politikken for implicitte Vulkan-lag, og at Whisper virker.
