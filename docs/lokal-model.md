# Lokal model: måling af hastighed og dansk kvalitet

Status: Implementationen kan prøves med fake på Linux. Qwen3.5-4B er **ikke målt**
på Mads' referencemaskine. Ingen model er hentet på agent-worker. Issue #16 er
fortsat åbent, indtil Mads har målt, vurderet mindst tre referater og besluttet plan B.

## Det, der faktisk køres

`summary/map_reduce.rs` bruges af `processor.rs` for den indbyggede motor.
Ollama og øvrige motorer beholder deres eksisterende sti. Den tidligere lokale
opdeling havde et fast fradrag på 300 tokens og samlede alle delresuméer i ét kald.
Den nye opdeling reserverer svar, prompt og indpakning ved hvert niveau.

Harnessen bruger samme `english_report` som appen, samme skabelonbygger,
modeldefinition, samplingparametre og eksisterende `generate_with_builtin`-klient.
Som appens danske standard genererer den først et engelsk referat og oversætter
det til dansk. Tiden inkluderer derfor begge trin. `--template daily_standup`
er standard; brug samme skabelon i appen ved sammenligning. Egne noter, cache,
lagring i mødedatabasen og appens baggrundskø er ikke del af målingen.

Hver input-JSON indeholder `id`, `minutes`, `segments`, `decisions`, `tasks` og
`names`; generatoren viser formatet. Ét timestampet segment pr. linje er grænsen
for opdeling. Segmenterne pakkes hele. Kun segmenter, der alene er for store,
deles ved sætningsslutning. En sætning, der alene er for stor, deles ved en
Unicode-tegngrænse, så algoritmen altid gør fremskridt uden at smide tekst væk.
Forrige segment/del gentages som overlap, hvis der også er plads til næste del.
Overlap er således højst én del og kan være nul ved store segmenter.

Budget: `kontekst - forventet svar (4096) - indpakning (256) - prompt`.
Optællingen genbruger `rough_token_count`: `ceil(antal Unicode-tegn × 0,35)`.
Det er et estimat, ikke en garanti for modellens tokenizer. `Budget.estimate`
er en udskiftelig funktion; brug en rigtig tokenizer i en senere protokoludvidelse.
Indpakningsreserven dækker chatmarkører og mindre forskelle i escaping. Meget
usædvanlig tekst kan stadig kræve et mere konservativt estimat. Tests beviser
budgettet i denne dokumenterede måleenhed, ikke i Qwens faktiske tokens.

Når delresuméerne ikke passer i slutprompten, pakkes og reduceres de igen i
flere niveauer. Alle kald kontrolleres mod budgettet. Tomme, for lange eller
ikke-skrumpende delresuméer giver fejl frem for tavs afkortning. En model- eller
afbrydelsesfejl stopper hele referatet. Der indføres ingen ekstra retries i den
lokale sti. At hvert segment når modellen beviser ikke, at modellen bevarer alle
fakta i det færdige referat; det vurderes særskilt nedenfor.

## Testdata og automatiske kvalitetssignaler

Generatoren producerer tre deterministiske, **fiktive** danske møder: kort (2 min),
ca. 30 min og ca. 90 min. Tidskoder kommer hver 10. sekund med skiftende
talerlabels. Varigheden er simuleret ud fra cirka 120 ord/minut, ikke optaget lyd.
Alma Vinterdal og Birk Måneskov er opdigtede. Kendte beslutninger, opgaver,
navne, budget, antal og datoer er fordelt fra begyndelse til slutning.
Det gentagne fyld gør datasættet reproducerbart, men lettere og mindre varieret
end naturlige møder. En god score her erstatter ikke en senere vurdering af
varieret tale, rettelser, uenighed og støjfyldt transskription.

Beslutninger og opgaver tælles, når alle plantede felter findes på samme linje
(ufølsomt for store/små bogstaver). Opgaver kræver ejer, handling (markørerne
“bestiller” og “sender”), objekt/antal og frist.
Parafraser, oversatte navne eller felter på forskellige linjer kan give falske
negative. Ord i samme linje kan give falske positive. Kontroller manuelt.
Tal, der kun findes i referatet, og par af ord med stort begyndelsesbogstav,
der ikke findes i kilden, markeres som mistanke om opdigtning. Sidstnævnte er
kun en simpel navneheuristik: overskrifter kan udløse den, og enkeltstående navne
kan overses. Signalerne er ikke en faktuel sandhedsdom.

Fake-motoren returnerer et kendt facit og tester målekæden. Dens kvalitetsscore,
RAM og hastighed siger **intet** om Qwen. Unit-tests kontrollerer segmentdækning,
alle map/reduce-budgetter, determinisme, tom tekst, præcis grænse, et for langt
segment, hierarkisk reduktion og fejl ved manglende fremskridt.

## Målefelter og begrænsninger

Hvert transskript køres i en ny proces, som starter og stopper sin egen helper.
Samlet tid starter før første modelkald, inklusive kold modelindlæsning, og slutter
efter dansk oversættelse. Processtart og helperens nedlukning indgår ikke.
JSON har individuelle kald med fase (`Map`, `Reduce`, `Final`, `Translation`) og
tid, samt antal map-bidder, reduce-kald og reduce-niveauer. `map_seconds` og
`reduce_seconds` er delsummer fra opdelingen; de individuelle kald er facit for
modeltid pr. fase. Gentag mindst tre gange pr. kombination og behold alle rå
rapporter; sammenlign median og langsomste gennemløb, aldrig fake-tal med lokale tal.

Proces-RAM og helper-RAM er maksimalt **samplet RSS i bytes** med ca. 50 ms
interval via eksisterende `sysinfo`. Korte spidser kan overses. Det er hverken
GPU-VRAM, allokeret hukommelse eller et OS-garanteret højvandsmærke. Helperen
identificeres som et direkte barn med `llama-helper` i procesnavnet. `null` betyder
ikke observeret, ikke nul RAM. Måleprocessen kan afsluttes før første prøve ved fake.

Den eksisterende helper sender ikke tokens eller backend i sit JSON-svar.
Harnessen opsamler dens stderr i en midlertidig fil og udtrækker kun kendte
numeriske statistiklinjer. Filen slettes automatisk efter hvert transskript.
`generated_tokens`, `generation_seconds` og `tokens_per_second` sættes kun, hvis
antal statistikblokke svarer til antal modelkald. Tokens/s er summen af faktiske
outputtokens divideret med summen af helperens afrundede generationstider, uden
promptbehandling og modelindlæsning. Manglende statistik eller nul afrundet tid
giver `null`. `estimated_output_tokens` bruger tegnestimatet på alle svar;
`estimated_tokens_per_second_end_to_end` dividerer dette estimat med samlet tid.
Estimerede og faktiske tal må ikke blandes.

`requested_backend` er kun Mads' kørselsmærkat. `vulkan_active` er `true` kun ved
både en positiv `VulkanN model buffer size` og positiv offload-evidens i
helperens diagnostik. Fund af en Vulkan-enhed alene er ikke nok. Ellers `null`:
ukendt, også ved CPU-build. Et Vulkan-build beviser ikke, at Vulkan blev brugt.
Hvis det forbliver ukendt, noter det og verificer backend med GPU-overvågning på
referencemaskinen. Issue #20 ejer en eventuel stabil protokol for disse oplysninger;
den er ikke ændret her. Harnessen installerer ingen logger og videresender ikke
rå helperlog, transskript eller referat til terminalen. Brug ikke en ekstern wrapper,
der tænder tekstlogning. JSON indeholder kun målinger og korte kvalitetssignaler.
Referattekst gemmes kun ved eksplicit `--referater` til manuel vurdering.

## Køreklar opskrift

Linux/fake (eksisterende lockfil, ingen modeldownload):

```bash
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver
cargo test -p tingskriver-core summary::map_reduce
cargo run -p tingskriver-core --example lokal_model_eval -- --generer /tmp/tingskriver-eval
cargo run -p tingskriver-core --example lokal_model_eval -- --transskripter /tmp/tingskriver-eval --motor fake --out /tmp/fake.json
```

På Windows køres følgende fra repoets rod i PowerShell. Start med CPU-only helper,
som har sit eget build-output; kerneprojektets Vulkan-feature styrer Whisper og
vælger **ikke** helperens backend. Brug den allerede installerede modelmappe som
`--data-dir`: klienten leder i `<data-dir>\models\summary\` efter modellens GGUF-fil.
Tilpas kun `$dataDir`, hvis appens eksisterende data ligger et andet sted.
Ingen af nedenstående kommandoer downloader en model.

```powershell
$env:CARGO_TARGET_DIR = "$env:USERPROFILE\dev\cargo-target\tingskriver-eval"
$dataDir = "$env:APPDATA\Tingskriver"
cargo build -p llama-helper --release --no-default-features --target-dir "$env:CARGO_TARGET_DIR\helper-cpu"
if ($LASTEXITCODE -ne 0) { throw 'CPU-build fejlede' }
$env:TINGSKRIVER_LLAMA_HELPER = "$env:CARGO_TARGET_DIR\helper-cpu\release\llama-helper.exe"
cargo run -p tingskriver-core --release --example lokal_model_eval -- --generer .\eval-data
cargo run -p tingskriver-core --release --example lokal_model_eval -- --transskripter .\eval-data --motor lokal --model qwen3.5:4b --data-dir $dataDir --backend cpu --out .\cpu-1.json --referater .\referater-cpu-1
if ($LASTEXITCODE -ne 0) { throw 'CPU-måling fejlede' }
```

Vulkan (kræver eksisterende Vulkan SDK/drivere og helperens normale buildværktøjer):

```powershell
cargo build -p llama-helper --release --no-default-features --features vulkan --target-dir "$env:CARGO_TARGET_DIR\helper-vulkan"
if ($LASTEXITCODE -ne 0) { throw 'Vulkan-build fejlede' }
$env:TINGSKRIVER_LLAMA_HELPER = "$env:CARGO_TARGET_DIR\helper-vulkan\release\llama-helper.exe"
cargo run -p tingskriver-core --release --example lokal_model_eval -- --transskripter .\eval-data --motor lokal --model qwen3.5:4b --data-dir $dataDir --backend vulkan --out .\vulkan-1.json --referater .\referater-vulkan-1
if ($LASTEXITCODE -ne 0) { throw 'Vulkan-måling fejlede' }
```

Gentag med `-2` og `-3` i rapport- og referatnavne. JSON og Markdown-tabellen
skrives ved siden af hinanden. Brug nye navne: en gentagelse med samme sti
overskriver den foregående rapport. Referater gemmes med testfilens navn.
Luk andre lokale modeller, tilslut strøm, behold samme strømprofil, og noter
commit, CPU, RAM, GPU, driver, OS, helper-build, modelnavn/kvantisering,
skabelon, strømmode og anden belastning. Brug samme maskine og data til begge
motorer. Prøv også appens normale baggrundsgenerering efter et fiktivt 90 min
møde; harnessen beviser ikke UI-responsivitet eller baggrundskøen.

## Resultater fra referencemaskinen

Mål: **90 min møde behandlet i baggrunden på under 15 min** uden at forstyrre
arbejdet. Et eventuelt ændret tidsmål besluttes og begrundes af Mads efter målingen.

| Model | Backend | Møde | Samlet tid | Map/reduce/slut/oversættelse | Proces/helper RAM | Tokens/s | Vulkan aktiv |
|---|---|---|---|---|---|---|---|
| Qwen3.5-4B | CPU | Kort | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| Qwen3.5-4B | CPU | 30 min | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| Qwen3.5-4B | CPU | 90 min | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| Qwen3.5-4B | Vulkan | Kort | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| Qwen3.5-4B | Vulkan | 30 min | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| Qwen3.5-4B | Vulkan | 90 min | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |

Maskine, commit, gentagelser og baggrundsprøve: **ikke målt**.

## Manuel kvalitetsrubrik

Vurder det danske slutreferat mod kilden, ikke mod fake-output. Hver kategori
scores 0–4. Skriv konkrete fejl med segment-id i vurderingsnoterne.

| Kategori | 0 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|---|
| Beslutninger | Ingen korrekte | De fleste mangler | Omkring halvdelen korrekte | Alle væsentlige, mindre udeladelse | Alle korrekt og tydeligt adskilt fra forslag |
| Opgaver | Ingen brugbare | Handlinger uden ejere/frister | Flere felter mangler | Alle opgaver, én mindre mangel | Hvem, hvad og frist korrekt for alle |
| Ingen opdigtning | Kritisk falsk beslutning/opgave | Flere falske fakta | Ét væsentligt falsk faktum | Kun uvæsentlig upræcis formulering | Ingen tilføjede navne, tal eller fakta |
| Dansk sprog | Uforståeligt | Overvejende forkert sprog | Forståeligt, mange fejl | Godt dansk, få fejl | Flydende præcist dansk og rigtige æøå |
| Struktur | Ubrugelig | Sammenblandet tekst | Overskuelig med væsentlige mangler | Klar med mindre gentagelse | Skabelonen fulgt, beslutninger og opgaver nemme at finde |

Eksempler: “Birk bestiller mapper” mangler både antal og frist og kan højst få 2
for den opgave. “Birk bestiller 12 mapper senest 2026-12-04” bevarer alle felter.
“Birk har bestilt 20 mapper” opfinder både status og antal og kræver en alvorlig
nedvurdering af ingen-opdigtning. “Vi overvejer sal 7” må ikke blive til en
beslutning, medmindre den faktisk blev truffet.

| Referat (mindst 3) | Beslutninger /4 | Opgaver /4 | Ingen opdigtning /4 | Dansk /4 | Struktur /4 | Sum /20 | Fejl og segment-id |
|---|---|---|---|---|---|---|---|
| Kort | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| 30 min | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |
| 90 min | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt | ikke målt |

## Beslutningsramme for plan B

Foreslået godkendelsesgrænse: mindst 17/20 på hvert af mindst tre referater,
ingen kategori under 3 og ingen opdigtet beslutning, opgave, ejer, frist eller
væsentligt tal. Alle plantede beslutninger og opgaver skal kunne genfindes manuelt.
Automatiske afvigelser undersøges; de udløser ikke alene et modelskift.

- Bestås kvaliteten og 90 min-målet under 15 min på en brugbar backend uden
  hukommelsesproblemer, behold 4B som standard.
- Fejler kvaliteten på mindst ét af tre referater, eller opfindes kritiske fakta,
  gentag med kontrolleret prompt/data. Gentagen fejl kræver sammenligning med en
  større model som valgfri download, ikke en automatisk opgradering.
- Bestås kvaliteten, men overskrides 15 min eller opstår paging/OOM, undersøg
  backend og kontekstbudget først. En større model er ikke en hastighedsløsning.
- Sammenlign samme datasæt, prompt, kvantiseringstype og rubricering. En større
  kandidat skal også bestå tids- og RAM-målet; ellers må Mads ændre målet eller
  vælge en anden løsning eksplicit.

`models.rs` kender aktuelt `qwen3.5:2b` (1221 MB), `qwen3.5:4b` (2614 MB),
`gemma3:4b` (2374 MB) og `gemma3:1b` (1019 MB). Alle er konfigureret med 32768
tokens. Gemma 4B er et alternativ, ikke en større model. Der er **ingen model
større end 4B** i kataloget endnu. Størrelserne er katalogværdier, ikke målt RAM.

En valgfri download kan efter Mads' beslutning bruge den eksisterende
`ModelManager::download_model(model_name, progress_callback)` og dens
GGUF-validering/genoptagelse. Den større kandidat skal først registreres med
korrekt model-id, URL, filnavn, licens, størrelse, chattemplate, kontekst og
sampling i modelkataloget i en særskilt ændring. Derefter vælger brugeren selv
download i appens modelhåndtering. Harnessen downloader aldrig og accepterer kun
allerede kendte, installerede modeller via `--model`. Ingen hypotetisk større
model kan måles med denne gren, før den er tilføjet og installeret.

**Mads' endelige beslutning og begrundelse: ikke målt / afventer måling.**
