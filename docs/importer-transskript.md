# Importér transskript (.vtt og .srt)

Et eksisterende transskript fra Teams, Zoom eller Meet kan importeres som et møde uden lyd. Der køres ingen transskription; referatet laves direkte.

## Sådan bruges det

- Tryk på **Importér transskript** i sidemenuen og vælg en `.vtt`- eller `.srt`-fil, eller træk filen ind i vinduet.
- Der oprettes et møde med tidskoder og talernavne (WebVTT `<v Navn>` og "Navn: tekst" i Zoom-filer). En `.srt` uden talere giver ét transskript med tidskoder.
- Mødedatoen er filens oprettelsestid (ellers ændringstid) minus transskriptets længde.
- Mødet har ingen lyd. Knappen **Optagelse** erstattes af en kort besked.
- Referatet laves med den udbyder og model, der er valgt under Indstillinger.

## Samme fil to gange

Importen får en nøgle af selve indholdet (tider, talere og tekst), uafhængigt af linjeskift, BOM og om filen er `.vtt` eller `.srt`. Samme indhold giver derfor ét møde. Importeres filen igen, og det tidligere referat fejlede eller blev annulleret, forsøges referatet igen.

## Når referatet fejler

Mødet og transskriptet bliver stående. Referatpanelet viser fejlen og knappen **Lav referat igen**.

## Kode

- Parser: `crates/tingskriver-core/src/subtitle/parser.rs` (rene funktioner, syntetiske testfiler).
- Import og database: `crates/tingskriver-core/src/subtitle/import.rs`, migrering `20261105090000_add_meeting_import_key.sql`.
- Tauri-kommandoer: `frontend/src-tauri/src/audio/import.rs`; UI: `ImportDialogContext`, `Sidebar`, `TranscriptDropBridge`.
- GPUI-appen: mødelisten findes endnu kun som pladsholder (`MEETINGS_LATER`). Strengene `IMPORT_TRANSCRIPT*`, `SUMMARY_REDO` og `NO_AUDIO_FOR_MEETING` i `strings` er klar til, når den kommer.
