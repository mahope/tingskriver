# Databasemigrationer, opgradering og tilbagerulning

Databasen (`meeting_minutes.sqlite`) deles af GPUI-appen og den gamle
Tauri-app, og en bruger kan rulle tilbage til en ældre installer. Migreringerne
ligger i `crates/tingskriver-core/migrations/` og køres af
`DatabaseManager::open_at` (`database/manager.rs`).

## En nyere database åbnes aldrig med en ældre version

Før noget forbinder til filen i skrivetilstand læser
`init::unknown_applied_migrations` tabellen `_sqlx_migrations` skrivebeskyttet
(`immutable`, når der ikke ligger en `-wal` eller `-journal`, så SQLite ikke
engang opretter `-shm`). Har databasen migreringer, som denne version ikke
kender, returnerer `open` `DbInitError::NewerDatabase`:

- Tekst i appen: "Databasen er oprettet af en nyere Tingskriver. Opdatér
  Tingskriver." Rådet siger, at mødernes data er urørte.
- Databasefilen ændres ikke (testet byte for byte), der tages ingen
  sikkerhedskopi (`wants_backup()` er falsk, for filen er uændret), og appen
  viser fejlsiden i stedet for at gå ned i en løkke.
- `kind_from_migrate` mapper også sqlx' `VersionMissing` hertil, hvis
  forhåndskontrollen skulle blive overhalet af et løb.

### Hvorfor ikke `set_ignore_missing`?

`Migrator::set_ignore_missing` findes i sqlx 0.8.6 og kunne lade additive
nyere migreringer passere. Det er valgt fra: `_sqlx_migrations` gemmer kun
version, beskrivelse og checksum, ikke SQL'en, så en ældre version kan ikke se,
om den ukendte migration er additiv eller har omdøbt, droppet eller ændret en
kolonne. At åbne og skrive til et skema, man ikke kender, kan ødelægge data
for begge versioner. Derfor afvises **alle** ukendte migreringer.

### `-- breaking`

En migration, der ikke er bagudkompatibel (DROP, RENAME, ændret kolonnetype
eller betydning, ændret CHECK), skal have en linje `-- breaking: <hvorfor>`
øverst i filen. I dag afvises alle nyere databaser uanset mærket, så mærket
ændrer ikke adfærden; det er til reviewet og til en senere lempelse, hvis
additive migreringer skal kunne åbnes (så skal kompatibilitetsgulvet gemmes i
databasen af selve migrationen, fordi buildet ikke kan læse en migration, det
ikke kender). Skriv ikke en additiv migration uden mærke, hvis du er i tvivl.

## Tilbagerulning af installer

1. Rul tilbage til en ældre installer. Den ser den nyere database og viser
   fejlsiden ovenfor. Intet er ændret.
2. Installér den nyeste Tingskriver igen. Databasen åbner som før.
3. Skal den ældre version bruges alligevel, så start den med en tom
   datamappe eller en kopi af databasen fra før opdateringen. Kopier
   `meeting_minutes.sqlite` sammen med eventuelle `-wal`/`-shm`-filer, mens
   appen er lukket. Møder lavet med den nyere version følger ikke med
   tilbage.

## Tests

`crates/tingskriver-core/tests/database_upgrade.rs`:

- `database_with_a_newer_migration_is_refused_and_left_byte_identical` og
  `newer_database_in_wal_mode_is_refused_without_changing_the_file`.
- `every_older_schema_version_upgrades_without_losing_rows` bygger en
  database for hvert præfiks af migreringerne (en rigtig gammel version),
  indsætter møder og transskriptioner, åbner med de nyeste migreringer og
  kontrollerer, at intet er tabt. Den dækker automatisk hver ny migration, så
  en ny migration kræver kun, at testen stadig er grøn. Kræver migrationen
  data i nye tabeller, så udvid `seeded_database_before_search_and_note_blocks_keeps_everything`
  (eller tilføj en tilsvarende test) med rækker, der viser, at de bevares.
