# Compare transcription providers

Open **Måling** in the GPUI app (`tingskriver-app --page measurement`).
Import a roughly 60-second recording as a mono 16 kHz PCM16 WAV file (maximum
90 seconds), then type the exact reference transcript. You can export a recording
from your audio editor to this format. The page does not record from a microphone.

Select the models to compare. Nothing is selected initially. The list contains
configured providers' default transcription models, model overrides in the
transcription and dictation chains, and the default local Parakeet and Whisper
models. Repeated models appear once; blank overrides use the provider's default
and do not add an empty choice. Use **Genindlæs udbyderindstillinger** after
changing `providers.json`.
Models and API keys use the existing provider configuration and Windows Credential
Manager; the measurement tool never downloads models or asks for keys in files.

Only the explicitly selected providers receive the sample, one model at a time.
Each gets identical PCM audio, Danish language (`da`), no dictionary hints and no
diarization. Reference text stays on the computer. No fallback chain is used.
Missing keys or unavailable local models produce a visible skipped row. Errors
produce a failed row, never an accuracy score of zero. HTTP redirects are refused.

Results show:

- WER as a percentage and total word errors (substitutions + deletions + insertions).
  Both reference and hypothesis use the existing `wer` module's
  `unicode-alphanumeric-lower-v1` normalization. WER may exceed 100%.
- Transcription elapsed time in milliseconds, including network transport for
  remote services. Local model loading, sample import and WER scoring are excluded.
  These are single runs, not latency percentiles or time to first token.
- Optional user-entered DKK price per audio hour. Empty means unknown, including
  local models. This is a tariff estimate, not a live vendor quote or an invoice;
  it excludes electricity, hardware, minimum billing units and subscriptions.

Metrics are saved in `<app-data>/measurements/<uuid>.json` with timestamp,
normalization version, duration, provider/model, status, word counts, WER,
latency, entered tariff and estimated cost of the sample. Audio, reference,
hypothesis, endpoints and credentials are not saved in these reports. The original
imported audio file stays where you put it. **Åbn gemte resultater** opens the
folder, including results from previous app sessions. A save failure is visible
and leaves results on screen.

## Verification without services

```sh
cargo test --locked --offline -p tingskriver-core measurement:: --lib
cargo test --locked --offline -p tingskriver-core wer:: --lib
cargo test --locked --offline -p tingskriver-app --lib
```

The tests synthesize PCM audio in memory and temporary files. They require
no real voices, credentials, models or internet access. Loopback HTTP contract
tests check identical uploads, Danish language, excluded providers, missing keys,
redirect refusal and validation before the first upload. A non-silent sample is
also compared across two separate HTTP endpoints, checking model selection,
normalization, word errors and that reference text is neither sent nor saved.
WER arithmetic and Danish normalization regression cases remain in the shared
`wer` unit tests.

Manual Windows check: import your sample, enter its reference, select a local
installed model, then compare selected remote services. Confirm missing-key
messages, tariff input with decimal comma, responsive navigation during inference,
local JSON persistence after restarting, keyboard focus, and both light/dark themes.

## Headless måling: `tingskriver-bench` (#212)

Samme måling uden GPUI, til agenter og CI. Binæren bruger samme WAV-import
(mono, 16 kHz, PCM16, højst 90 s), samme motorkald, dansk sprog (`da`) og samme
WER-normalisering som Målingssiden. Kun installerede lokale modeller (Parakeet og
Whisper) måles; intet downloades, ingen skyudbyder kaldes, og modellerne køres én
ad gangen.

```sh
cargo build --release -p tingskriver-core --bin tingskriver-bench
tingskriver-bench --list-models [--models-dir <mappe>] [--json]
tingskriver-bench --file proeve.wav --model parakeet-tdt-0.6b-v3-int8 --repeat 5 \
  [--reference facit.txt] [--models-dir <mappe>] [--json]
```

`--model` kan gives flere gange. `--models-dir` er som standard appens modelmappe
(`paths::app_data_dir()/models`, på Windows
`%APPDATA%\dk.mahoje.tingskriver\models\`). På Windows skal `onnxruntime.dll` ligge ved
siden af exe-filen, eller `ORT_DYLIB_PATH` pege på den.

JSON (`schema_version` 1) har pr. model: `status` (`completed`, `skipped` hvis
modellen ikke er installeret, `failed` ved indlæsnings- eller kørselsfejl),
`load_ms`, `timing` med `runs_ms`,
`best_ms`, `median_ms`, `mean_ms`, `rtf_best` og `rtf_median` (lydsekunder pr.
beregningssekund, højere er hurtigere), `wer` og `score` (kun med `--reference`)
og `peak_rss_mb` (Linux: procesens spidshukommelse, som aldrig falder, så kør én
model pr. proces for et rent tal). `--repeat` er 1–100; én fejlet kørsel gør hele
modellens måling til `failed`, så der aldrig rapporteres delvise statistikker.
Afslutningskode: 0 alt målt, 1 mindst én model fejlet, 3 ingen fejlet men mindst
én model sprunget over (ikke installeret), 2 ugyldige argumenter eller lyd.
Som på Målingssiden udelades loadtid fra ventetiden; `load_ms` er med separat.
Ingen lyd, facit eller tekst skrives til uddata eller logs.

Tal fra Målingssiden og fra `tingskriver-bench` for samme lyd og model er
sammenlignelige inden for målestøj (første kørsel er ofte langsommere end de
følgende, derfor `--repeat`). Kør `--repeat 5` og brug `median_ms` til
sammenligning.

Test uden modeller: `cargo test --locked -p tingskriver-core measurement::bench --lib`
(median/bedste, argumenter, WER kun med facit, fejlede kørsler, afslutningskoder,
manglende model giver `skipped`, beskadiget installation giver `failed`).
Selve motorkørslen og "samme tal som Målingssiden" er ikke dækket af automatiske tests, fordi de kræver installerede modeller: det
skal bevises med en kørsel af begge veje på en maskine med en model.

Målingsside og indstillinger er uændrede: kommandolinjen er et udviklerværktøj og
vises ikke i appen. Hvis Mads vil se tallene i appen, skal Målingssiden læse de
samme `bench::Report`-rapporter.
