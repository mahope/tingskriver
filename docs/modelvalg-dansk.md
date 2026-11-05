# Kandidatmodeller til dansk: go/no-go (#184)

Status 5. november 2026: **ingen model er målt på vores eget danske sæt, og ingen
model er skiftet som standard.** Testsættet fra #30 findes ikke endnu:
`testsaet/dansk-wer/manifest.json` på `main` har `"clips": []` (se
[KILDER.md](../testsaet/dansk-wer/KILDER.md)). Uden lyd og facit kan en
fejlrate hverken måles eller forsvares, og jeg har ikke opfundet tal. Tabellen
nedenfor skelner derfor tydeligt mellem **offentliggjorte tal** (andre er målt på
FLEURS, oplæst tale) og **vores målinger** (ikke udført).

## Tabel

| Model | Offentliggjort dansk WER (FLEURS) | Vores WER | Ventetid 30 s (Mads' pc) | Hukommelse | Licens |
|---|---:|---|---|---|---|
| parakeet-tdt-0.6b-v3 (nu) | 18,41 % (NVIDIA-kort), 18,64 % (Handy) | ikke målt | ikke målt | ikke målt | CC-BY-4.0 |
| canary-1b-v2 | 11,25 % (NVIDIA-kort), 11,34 % (Handy) | ikke målt | ikke målt | ikke målt | CC-BY-4.0 |
| Parakeet Ultra | 15,98 % (Handy); moondream angiver ikke dansk alene | ikke målt | ikke målt | ikke målt | CC-BY-4.0 |
| Whisper large-v3-turbo | 13,60 % (Handy) | ikke målt | ikke målt | ikke målt | MIT (modelkort); Handy angiver Apache-2.0 |
| Whisper large-v3 (reference) | 12,48 % (Handy) | ikke målt | ikke målt | ikke målt | Apache-2.0 (Handy) |

Handys tal er målt på Q8_0 GGUF via transcribe.cpp på en Ryzen 4750U og siger
intet om vores ONNX-bygning. Handys katalogdata er ikke kopieret ind i repoet; kun
de tal, issuet selv citerer, er gengivet.

Målt på agent-worker (Linux, CPU): **intet**. Maskinen har 4 kerner og 7 GB RAM
(ikke 16 GB), der findes ingen installerede modeller, og der er intet dansk lydsæt
at måle på. Jeg hentede ingen modelfiler (op til 1,6 GB pr. model) til en måling
uden facit. Tal fra agent-worker ville under alle omstændigheder aldrig gælde for
Mads' pc.

## Kommandoer til Mads (Windows, ved et færdigt testsæt fra #30)

Byg og forbered lyd (30 s, mono, 16 kHz, PCM16) og facit i `facit.txt`:

```powershell
cargo build --release -p tingskriver-core --bin tingskriver-bench
ffmpeg -i moede.wav -t 30 -ar 16000 -ac 1 -c:a pcm_s16le proeve30.wav
```

Mål én model ad gangen, så spidshukommelsen er ren for hver:

```powershell
$b = "target\release\tingskriver-bench.exe"
& $b --list-models --json
& $b --file proeve30.wav --model parakeet-tdt-0.6b-v3-int8 --repeat 5 --reference facit.txt --json > parakeet-v3.json
& $b --file proeve30.wav --model large-v3-turbo --repeat 5 --reference facit.txt --json > whisper-turbo.json
```

`median_ms` er ventetiden for 30 s lyd, `wer` fejlraten og `peak_rss_mb` kun
udfyldt på Linux; på Windows aflæses hukommelsen i Task Manager (Arbejdssæt, privat)
mens målingen kører. Fejlrate pr. klip i testsættet: kør `examples/wer_run.rs`
(se [wer.md](wer.md)) med hver model; den samlede WER er sum af fejl over sum af
referenceord. Møder og diktering måles på hver sin del af sættet (diktering: korte
klip, ventetid mod grænsen på 1,5 s; møder: samtale, hvor ventetiden er uden
betydning).

Whisper large-v3-turbo er allerede en installerbar lokal model i appen
(`large-v3-turbo`, også `large-v3-turbo-q5_0`) og kan måles i dag. Parakeet v3 er
standard. Canary og Parakeet Ultra kan ikke måles i vores værktøj uden kodeændring
(næste afsnit).

## Findes der en ONNX- eller whisper-rs-vej?

Alle oplysninger er læst fra modelkort og filoversigter på Hugging Face den
5. november 2026 (`curl`, ingen browser). Intet er hentet eller kørt.

**Whisper large-v3-turbo: ja, ingen ny motor.** Støttes allerede af whisper-rs i
`whisper_engine` som GGML (`ggerganov/whisper.cpp`: `ggml-large-v3-turbo.bin`
1,6 GB, `-q8_0` 874 MB, `-q5_0` 574 MB). Det er også appens
`DEFAULT_WHISPER_MODEL`. Der kræves ingen ændring for at måle den. Ifølge issuet
er den ca. 1,4x realtid på CPU, så den er for langsom til diktering uden GPU
(Vulkan/CUDA skal måles), men brugbar til møder efter mødet.

**Parakeet Ultra: sandsynligvis ja, men uverificeret.** `moondream/parakeet-ultra`
er en efter-trænet version af `nvidia/parakeet-tdt-0.6b-v3` med samme arkitektur,
tokenizer og 0,6 mia. parametre. ONNX-eksporter findes hos tredjepart:
`Olicorne/parakeet-tdt-0.6b-v3-ultra-onnx` (opdateret 22/10-26; `int8/`
encoder 649 MB + decoder 18 MB, `nemo128.onnx`, `vocab.txt`, altså samme filnavne
som vores motor forventer) og `beshkenadze/parakeet-ultra-onnx` (kun fp16-encoder
1,2 GB). Hindringer, som skal afklares, før der siges go:
1. Vores `ParakeetEngine` kontrollerer eksakte filstørrelser og kender kun de to
   v2/v3-modeller; en ny modelspecifikation (navn, størrelser, kilde) kræves.
2. Encoderen i `int8/` er kvantiseret med `MatMulNBits` (ONNX Runtime-udvidelse),
   formentlig ikke som istupakovs v3-int8 (ikke kontrolleret). Den skal prøves med ONNX Runtime 1.22 (vores
   version), ikke antaget.
3. Eksporterne er enkeltpersoners tredjepartsbygninger af et nyt, hurtigt
   ændrende repo (moondream, september 2026). Kildekontrol og versionslåsning
   (revision + SHA-256) kræves, før noget leveres. Moondreams egen kørsel kræver
   Photon/GPU og er ikke en vej for os.
Moondreams kort angiver 9,55 % mod 11,62 % for v3 som gennemsnit over 25 sprog
(GPU, deres pipeline), ikke dansk alene. Handys danske 15,98 % mod 18,64 % er
mere beskedent.

**canary-1b-v2: ONNX findes, men kræver en ny motor.** 978 mio. parametre,
encoder-dekoder (32 + 8 lag) med autoregressiv dekoder og promptede sprog- og
opgave-tokens, ikke TDT. ONNX: `istupakov/canary-1b-v2-onnx` (feb. 2026; int8
encoder 859 MB + int8 dekoder 170 MB, fp32 ca. 4,8 GB inkl. datafil, til
`onnx-asr`) og enkelte andre. Vores `ort`-baserede Parakeet-kode dækker ikke en
AED-dekoder (ingen greedy/beam-løkke, ingen promptformat, ingen chunking af lyd
over 40 s som modelkortet beskriver). Ingen whisper-rs-vej (det er ikke Whisper).
Det er en ny motor, som dette issue ikke må tilføje. Der findes GGUF til
transcribe.cpp (Handys motor), men den er ikke Rust/whisper-rs og ikke en
afhængighed, vi har.

## Anbefaling

**Møder (batch efter mødet, ventetid underordnet): GO til at måle, NO-GO til at
skifte endnu.** Mål først `large-v3-turbo` på Mads' pc (ingen kodeændring). Hvis
den slår Parakeet v3 tydeligt på vores sæt, kan den tilbydes som valgfri
"mere nøjagtig"-model til mødetransskription, ikke som standard. Canary er
den mest nøjagtige på papiret (ca. 11 % mod ca. 18 %), men kræver en ny motor og
en ny model på ca. 1 GB (int8): beslut først efter turbo-målingen, og kun hvis
forskellen er stor nok til at retfærdiggøre vedligeholdelsen af en motor mere.

**Diktering (under 1,5 s): NO-GO til turbo og canary på CPU; Parakeet Ultra er
den eneste kandidat, og kun hvis en Ultra-ONNX kan indlæses.** Whisper turbo
(~1,4x realtid) og canary (8x på Handys CPU, men med ca. 1 GB model og
ekstra dekoderløkke) er ikke målt mod grænsen. Ultra har Parakeets hastighed
(13x realtid) og antagelig samme latens som v3, og kan være en ren modelopgradering
uden ny motor. Næste skridt er en lille prototype (ikke dette issue): tilføj en
modelspecifikation, indlæs Olicornes `int8/`, mål med `tingskriver-bench`.

**Det, der skal ændres, hvis der siges go:** (1) selve testsættet fra #30
(forudsætning for alt), (2) en `ParakeetEngine`-modelspecifikation for Ultra
(filnavne, eksakte størrelser, revision, SHA-256) og en prøve af
`MatMulNBits` på ONNX Runtime 1.22, (3) for canary en ny motor med dekoderløkke og
chunking, (4) mærkning i modelvælgeren og i `THIRD_PARTY_LICENSES.md`,
(5) målinger på Mads' pc med kommandoerne ovenfor.

## Licens og attribution

- **Parakeet v3, Parakeet Ultra og canary-1b-v2: CC-BY-4.0.** Krav: navngiv
  ophavet (NVIDIA; for Ultra desuden moondream som efter-træner og eventuelle
  ONNX-konverterere), link til licensen, angiv at modellen er konverteret/ændret,
  og antyd ingen godkendelse. Mønster findes i `THIRD_PARTY_LICENSES.md` for
  Parakeet v3 (skal udvides pr. model, vi leverer). Ultra er afledt af v3, så
  begge kilder skal nævnes. Hentes vægtene af brugeren ved første brug (som i dag),
  er vi ikke distributør, men attributionen holdes alligevel opdateret.
- **Whisper large-v3-turbo: MIT** på OpenAIs modelkort; issuet (Handys katalog)
  angiver Apache-2.0. Begge er tilladende og kræver kopi af licensteksten; sæt
  den som MIT, som `THIRD_PARTY_LICENSES.md` allerede gør for Whisper, og tjek
  modelkortet igen ved levering.
- `@handy-computer/models`-pakkens licens er **ikke** undersøgt, og ingen
  katalogdata er kopieret ind i repoet. Handys kode er MIT; gør det før kopiering.
- Træningsdata for Ultra er ikke fuldt dokumenteret i modelkortet (jeg har kun læst
  de første ca. 80 linjer); læs resten, før vi leverer den.

## Genskab beslutningsgrundlaget

```sh
curl -sL https://huggingface.co/nvidia/canary-1b-v2/raw/main/README.md
curl -sL https://huggingface.co/moondream/parakeet-ultra/raw/main/README.md
curl -sL "https://huggingface.co/api/models/Olicorne/parakeet-tdt-0.6b-v3-ultra-onnx?blobs=true"
curl -sL "https://huggingface.co/api/models/istupakov/canary-1b-v2-onnx?blobs=true"
curl -sL "https://huggingface.co/api/models/ggerganov/whisper.cpp?blobs=true"
```
