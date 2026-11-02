# Fuldtekstsøgning

`database::repositories::search` i `tingskriver-core` er et lokalt API uden
Tauri- eller GPUI-afhængigheder. Den eksisterende `search_transcripts` med LIKE
er bevaret; UI og MCP kan senere bruge det nye API.

## Skema og opdatering

Migration `20261101120000_add_search_fts5.sql` opretter og fylder indekset i
samme migrationstransaktion som sqlx bruger. Den kræver ingen nye indeks på
`transcripts` og sorterer efter opstart-db-migrationen fra 2026-10-31.

| Kilde | Autoritativ tekst | Tidskode |
| --- | --- | --- |
| `Title` | `meetings.title` | Ingen |
| `Note` | `meeting_notes.notes_markdown`, ellers tekst fra `notes_json` | Ingen |
| `Summary` | `summary_processes.result`: JSON-feltet `markdown`, ellers JSON-tekstværdier eller ældre råtekst | Ingen |
| `Transcript` | `transcripts.transcript` | `audio_start_time`, sekunder fra optagelsens start |

Referaternes JSON-nøgler indekseres ikke. JSON-fallbacken medtager alle
tekstværdier, også metadata i ældre formater. Markdown foretrækkes, så moderne
referater med både markdown og blokdata ikke dobbelttælles. Backup-referater,
segmenternes gamle `summary`-felt og `transcript_chunks` indekseres ikke:
det er henholdsvis backups, et ubrugt legacy-felt og en kopi af transskriptet.

`search_source_documents` er en projektion af disse fire kilder.
`search_documents` er en afledt dokumenttabel med stabil heltalsprimærnøgle og
unik `(source, source_id)`. Dermed afhænger FTS-række-id'er ikke af implicitte
række-id'er, som `VACUUM` kan ændre i kildetabellerne. `search_fts` er et FTS5
external-content-indeks over dokumenttabellen. Kun `body` tokeniseres.
Et fælles korpus gør bm25-scorerne sammenlignelige på tværs af kilder.

Insert/update/delete-triggere på alle fire kildetabeller holder dokumenter og
FTS opdateret i samme transaktion. Sletning via appens `delete_meeting` og via
`ON DELETE CASCADE` er testet, inklusive noterne som appen ikke selv sletter.
Forbindelser skal have `PRAGMA foreign_keys=ON`, som sqlx SQLite har som standard.
Skriv gennem kildetabellerne, aldrig direkte i de afledte tabeller.

## Tokenisering og forespørgsler

`unicode61 remove_diacritics 0` bevarer æ, ø og å, også ved søgning med store
bogstaver. Standardens diakritikfoldning kan blandt andet sammenblande å og a.
`øl` og `ol` er forskellige ord. Søgningen er uafhængig af store/små bogstaver,
men laver hverken stemming, substring-søgning eller dansk sproglig normalisering.

Input opdeles ved whitespace. Hvert led citeres med escaped anførselstegn og
kombineres med AND. `NEAR`, `OR`, minus og parenteser udføres ikke som operatorer.
Et afsluttende `*` tillader prefiks: `planl*` finder `planlægning`. Tegnsætning
inde i et citeret led følger unicode61's frase-tokenisering. Tomt input og led
uden bogstaver/tal giver ingen resultater. Rå FTS-syntaks accepteres ikke.

## API

```rust,no_run
use tingskriver_core::database::repositories::search::{
    search, rebuild, SearchFilters, SearchSource,
};
# async fn example(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
let filters = SearchFilters {
    source: Some(SearchSource::Transcript),
    ..Default::default()
};
let hits = search(pool, "årsplan budget*", &filters).await?;
rebuild(pool).await?;
# Ok(())
# }
```

Filtrene `created_from` (inklusive) og `created_before` (eksklusive) er
`DateTime<Utc>` og gælder **mødets oprettelsestid**, ikke segmentpositionen.
`source` er valgfri. Standardgrænsen er 50 resultater, højst 500; nul giver ingen.

`SearchHit` indeholder kilde, kilde-id, møde-id, uddrag, valgfri tidskode og
bm25-rang (laveste værdi først). Rækkefølgen er stabil: rang, møde-id, kilde,
kilde-id. SQLite bruger bm25 til rangering og `snippet()` til uddrag.
Uddrag har op til 32 tokens og markerer match med `MATCH_START` (U+001E) og
`MATCH_END` (U+001F). Vis uddrag som tekst og oversæt markørerne til fremhævning;
behandl aldrig uddraget som HTML. Markørerne er ikke escape-tegn for kildetekst.

Tidskoden er `None` for titel, note, referat og gamle transskripter uden
`audio_start_time`. Opfind ikke en position ud fra det absolutte `timestamp`.
Et transskriptresultat med `Some(seconds)` kan åbnes ved den position i mødet;
andre resultater åbner selve mødet.

## Genopbygning og verifikation

Kald `rebuild(&pool).await?` ved behov. Funktionen erstatter dokumenttabellen
fra kilderne og genopbygger FTS' inverted index i én transaktion. Kildedata
ændres ikke, og læsere ser ikke et halvt genopbygget indeks. Tag normal
DB-backup før vedligeholdelse. Der er ingen automatisk CLI eller UI til dette.

```bash
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$HOME/dev/cargo-target/tingskriver
cargo test -p tingskriver-core --test search_fts5 -- --nocapture
```

Benchmarken genererer deterministisk 100 møder med 800 segmenter på 4,5 sekunder
hver, i alt 100 timers møder. En fast LCG-seed giver danske ord i hvert segment.
Efter opvarmning måles medianen af 15 søgninger, inklusive prefiks og flere ord.
Testen kræver under 100 ms i release-build. Debug-build har en løsere grænse
på 500 ms, fordi SQLite uden optimering og samtidige CI-job gør 100 ms ustabilt.
CI kører release-grænsen i `.github/workflows/search-benchmark.yml`.
Begge måler hele API-kaldet med rangering, 50 uddrag og sqlx-dekodning.
Datagenerering indgår ikke i tiden. Kør den stramme grænse med:

```bash
cargo test --release -p tingskriver-core --test search_fts5 hundred_hours -- --nocapture
```

Målt på agent-worker 2026-11-01 med det ovenstående korpus: 65,0 ms median i
release og 95,5 ms i en målrettet debug-kørsel. Tallene er maskinafhængige;
CI håndhæver grænserne ovenfor. Ved samtidige worktrees kan Cargos delte
buildmappe genbruge et andet jobs core-artefakt. Lokalt blev debug-kørslerne
derfor kørt med `cargo --config 'profile.dev.package.tingskriver-core.debug=1'
test ...`, stadig uden optimering og med samme aftalte `CARGO_TARGET_DIR`.
