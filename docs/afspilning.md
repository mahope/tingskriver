# Lydafspilning i core

`tingskriver_core::playback` streamer fra en lokal fil. Modulet afhænger ikke af
UI. `audio/playback_monitor.rs` vedrører enhedsovervågning og er ikke ændret.

```rust,no_run
use std::{path::PathBuf, time::Duration};
use tingskriver_core::playback::{CpalOutput, Player, Source};
# fn example() -> Result<(), tingskriver_core::playback::PlaybackError> {
let output = CpalOutput::default_device()?;
let (mut player, events) = Player::open(Source::Mixed(PathBuf::from("audio.mp4")), output)?;
player.seek(Duration::from_secs(42))?;
player.set_speed(1.5)?;
player.play();
let position = player.position();
player.pause();
// Keep player alive while playing; receive events on the application's event loop.
# Ok(())
# }
```

`Source::Tracks { mikrofon, system }` åbner to filer på samme tidsakse.
`set_track_gains(mikrofon, system)` accepterer 0..=1; nul slår sporet fra.
Sporene nedmixes til mono ved gennemsnit af kanalerne, konverteres til udgangens
samplerate, summeres og begrænses til -1..=1. Et afsluttet spor erstattes af
stilhed, mens det andet fortsætter. CPAL sender mono til alle udgangskanaler.

`open` og ændringer af tidsakse returnerer `PlaybackError`. `set_speed` afviser
NaN, uendelighed og værdier uden for 0,5..=2,0. `play`, `pause` og `position`
arbejder på atomiske værdier. Positionen er medietid, beregnet fra leverede
frames, og står stille ved pause og bufferunderløb. Den omfatter ikke lydkortets
hardwarelatens. `seek` kan også bruges efter `Ended`; kald derefter `play`.
Et kald til `play` alene efter slut starter ikke forfra.

`Event::Position(Duration)`, `Ended` og `Error(PlaybackError)` kommer gennem
modtageren fra `open`. Køen har 64 pladser; positionsopdateringer kan springes
over, når forbrugeren er langsom. Slut og fejl forsøges afleveret igen, indtil
der er plads. Modtageren bør tømmes løbende; `position()` er altid tilgængelig.
Drop stopper og joiner afkodningstråden. Filstier og lydindhold indgår ikke i
fejlbeskeder eller logs.

## Streaming og formater

Symphonia læser via `MediaSourceStream<File>` og afkoder én pakke ad gangen på
en arbejdstråd. En lock-free ringbuffer rummer højst 8.192 mono-frames med
position og generation. Søgning ugyldiggør gamle frames; callbacken kasserer dem.
Callbacken allokerer ikke, låser ikke og læser ikke filer. `AudioOutput` adskiller
udgangen fra afspilleren; `FakeOutput::pull` bruger samme `Render` som CPAL uden
lydhardware. CPAL understøtter f32/i16/u16 og returnerer en typet fejl for andre
udgangsformater.

PCM-WAV, AAC i M4A/MP4, MP3, FLAC og Ogg/Vorbis understøttes af de eksisterende
Symphonia-features. B1-grenen bruger PCM-s16 mono WAV-segmenter og AAC i
`microphone.m4a`, `system.m4a` og `audio.mp4`. Ingen feature eller pakke er tilføjet.
Ukendte codecs giver `PlaybackError::Unsupported` med dansk besked.

Søgning bruger `SeekMode::Accurate`, nulstiller decoderen og afkoder fra 250 ms før søgepunktet frem til
det ønskede sample ud fra pakkernes tidsstempler og timebase. Forløbet før
søgepunktet klargør transform-codecs og MP3's bitreservoir. MP4's edit-liste
læses separat med faste små læsninger, fordi Symphonia 0.5 ikke anvender den;
encoderforsinkelsen indregnes i både søgning og afkodning. En enkelt almindelig
edit understøttes (version 0/1, positiv medietid, rate 1); mere komplekse
edit-lister afvises tydeligt som et ikke-understøttet format. Hvis demuxerens
søgning fejler, genåbnes filen og afkodes frem fra start med samme begrænsede
buffer. Det kan tage længere tid for filer uden søgeindeks. Demuxerens egen
metadata, f.eks. MP4's sampletabeller, kan vokse med optagelsens længde; PCM-data
læses aldrig samlet ind.

## Hastighed og lydkvalitet

Hastighed og samplerate konverteres med sample-hold-resampling på den fælles
medietidsakse. Outputantal beregnes fra et heltals-frameindeks, så flydende
akkumulationsfejl ikke forlænger afspilningen. Ét sekund giver 32.000/16.000/8.000
frames ved 16 kHz output og henholdsvis 0,5/1/2x. Tonehøjden ændres med
hastigheden. Metoden har ingen anti-aliasfiltrering og er en basal afspilningsvej;
WSOLA med bevaret tonehøjde og filtreret resampling er en opfølgning før krav om
høj lydkvalitet. Rubato er ikke brugt, og ingen ny tung afhængighed er indført.

## Test og måling

Alle normale tests bruger fake output og genererer deres tone-WAV selv.
Markøren skifter frekvens hver 100 ms, og nulpassager i de første 50 ms efter
søgning skal identificere den rigtige markør. Positionen kontrolleres særskilt.
Der søges både frem og tilbage ved 38 faste og pseudotilfældige tidskoder pr. format. Formatprøven bruger FFmpeg til at generere de
komprimerede varianter og kræver ingen lydhardware.

```sh
cargo test -p tingskriver-core playback::tests
cargo test -p tingskriver-core playback::tests::seek_markers_all_formats -- --ignored --nocapture
cargo test -p tingskriver-core playback::tests::hour_startup_and_bounded_memory -- --ignored --nocapture --test-threads=1
```

Ydelsesprøven skriver en fysisk 3.600 s PCM16-WAV på 115.200.044 bytes med en
fast skrivebuffer og sammenligner med en 1 s WAV. Den måler fra `open` til
levering af de første 100 ms via fake output, kræver under 500 ms og kræver
RSS-tilvækst under 16 MiB på Linux. Målt 2026-11-01 på Linux agent-worker (debug-build, én testtråd): 1 s WAV
startede på 1,64 ms med 8.328 KiB RSS-tilvækst; 1 times WAV startede på 1,98 ms
med 36 KiB yderligere RSS-tilvækst. Målingen inkluderer åbning, afkodning og
fake-levering, men ikke disk-cache-tømning eller lydkortets latens. Den viser
konstant PCM-bufferforbrug for de to størrelser, ikke en universel RSS-grænse
for alle codecs. Hardwarelatens og subjektiv lydkvalitet måles manuelt.

## Manuel prøve på Windows

Fra en lille Rust-vært: åbn en færdig `audio.mp4` med `CpalOutput`, behold
`Player` og tøm hændelseskanalen. Afspil, pausér, søg frem og tilbage og kontrollér
at lyden matcher tidskoden. Prøv 0,5x/1x/2x. Åbn derefter mikrofon- og systemspor
sammen, slå hver kilde fra og til, og kontrollér synkronisering og klipbeskyttelse.
Kontrollér også fejl ved manglende lydenhed og afbrydelse af udgangen. Denne gren
indeholder ingen UI-integration, så den gamle Tauri-hook kan ikke bruges til prøven.
