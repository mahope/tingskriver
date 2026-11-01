# Egne noter i core

`tingskriver_core::notes` er et UI-uafhængigt API. Ingen frontend eller
opstartskode ændres i denne gren.

## Model og klokke

`Document { blocks: Vec<Block> }` indeholder afsnit, overskrift (niveau 1–3),
punkt, nummereret punkt og afkrydsning med tilstand. `Span` indeholder UTF-8-tekst
og uafhængige `bold`/`italic`-flag. `Document::canonical()` fjerner tomme spans,
slår nabospans med ens stil sammen og begrænser overskriftsniveau til 1–3.
Blokke, også tomme afsnit, bevares i rækkefølge.

`created_offset_ms: Option<u64>` sættes af kalderen ved oprettelse, med antal
millisekunder fra optagestart. Redigering ændrer ikke automatisk tidsstemplet.
`None` bruges uden optagelse og til alle gamle noter; historiske tidspunkter
opfindes ikke. SQLite accepterer højst `i64::MAX`; større værdier giver fejl,
og hele gemningen rulles tilbage.

## Markdown-lite v1

`markdown::serialize` og `markdown::parse` giver
`parse(serialize(doc)) == doc.canonical()`, inklusive tidsstempler.
Formatet er en afgrænset markdown-dialekt, ikke en generel CommonMark-parser:

- En fysisk linje pr. blok; `# `, `## `, `### `, `- `, `1. `,
  `- [ ] ` og `- [x] ` angiver bloktypen.
- `**tekst**`, `*tekst*`, `***tekst***` angiver fed, kursiv og begge.
- `\*`, `\#`, `\-`, `\.` og `\\` er bogstavelige tegn. Punktummer og
  bindestreger escapes også inde i linjer for en enkel, entydig kanonisk form.
- `\n` og `\r` repræsenterer linjeskift inde i en span. `\l` repræsenterer `<`,
  så brugertekst aldrig kan blive fortolket som metadata. Unicode bevares.
- `<!--offset:123--> ` foran en blok bevarer dens optagelsesoffset.
- `<!--s-->` adskiller spans, så tilstødende stjerner aldrig bliver tvetydige.
- `<!--empty-->` bevarer en tom blok. Den tomme streng betyder nul blokke.

HTML-kommentarerne er usynlige i almindelige markdown-visere. Ved redigering
skal egne UI'er bruge modellen; fremmede markdown-værktøjer kan fjerne metadata.

## SQLite og transaktioner

Migration `20261101130000_add_meeting_note_blocks.sql` opretter:

- `meeting_note_documents(meeting_id)`: markerer et initialiseret dokument,
  også hvis dokumentet er tomt. Det forhindrer genimport efter sletning af alle blokke.
- `meeting_note_blocks(meeting_id, position, block_type, level, checked,
  text_markdown, created_offset_ms)`: primærnøgle `(meeting_id, position)`,
  typer og værdier med CHECK-begrænsninger, fremmednøgle med `ON DELETE CASCADE`.
  `text_markdown` indeholder kun inline-indhold; type og offset læses fra kolonnerne.

`NotesStore::save` erstatter blokkene og opdaterer
`meeting_notes.notes_markdown` i samme transaktion. Eksisterende
`meeting_notes.notes_json` indgår aldrig i UPDATE eller DELETE. Ingen gamle
kolonner ændres. Forbindelsen skal have SQLite foreign keys aktiveret (sqlx-standard).

`NotesStore::load` laver doven migrering, før det læser blokkene. Importens
markering og gemning sker i samme transaktion. Samtidige importer og gemninger
serialiseres af SQLite; ved låsefejl får kalderen en fejl og kan forsøge igen.

## Migrering og opstartsintegration

`migrate_legacy_notes(&pool).await?` returnerer `MigrationReport` med antal
`migrated`, `skipped`, `failed` og `unsupported_blocks`. Hvert møde har sin egen
transaktion; en fejl tælles uden at standse de andre møder. JSON og notetekst
skrives ikke til logs eller fejlbeskeder. Fejlede møder forsøges igen næste gang.

BlockNote 0.36-arrays understøttes: `type`, `props`, inline `content`/`styles`
og rekursive `children`. Indrykning flades ud i læserækkefølge, da målmodellen
ikke har en indrykningsdybde. Ukendte typer bliver afsnit; tekst i indhold,
tabellens rækker/celler, billedtekster og filnavne bevares. Styling ud over
fed/kursiv, links, billeder og tabellayout gemmes fortsat i original-JSON.
Uden JSON importeres den eksisterende markdown-kopi.

**Placering af det fremtidige opstartskald:** Opstart-db-grenen skal kalde
`tingskriver_core::notes::migrate_legacy_notes(&pool).await?` umiddelbart efter
`sqlx::migrate!("./migrations").run(&pool).await?` og før poolen udleveres til
repositories. På denne base ligger punktet i
`crates/tingskriver-core/src/database/manager.rs`, `DatabaseManager::new`;
hvis opstart-db flytter migrationerne til `database/init.rs`, placeres kaldet
der efter de samme SQL-migrationer. Rapportens `failed` skal håndteres som
antal fejl, uden at udskrive noteindhold. Funktionen er bevidst ikke tilkoblet her.

## Autosave og GPUI (#47)

Opret en `NotesSession::load(store, meeting_id).await?`. Direkte ejere kan
bruge `document()`, `edit(|doc| ...)`, `is_dirty()` og `flush().await?`.
Kalderen skal drive autosave; konstruktion alene starter ingen skjult opgave.

Til løbende autosave oprettes en bounded `tokio::sync::mpsc::channel<Document>`.
Start `runtime::spawn(session.run(receiver))`, og send dokument-snapshots ved
ændringer. Løkken venter på 500 ms ro efter seneste ændring, men gemmer senest
to sekunder efter første ugemte ændring; senere ændringer flytter aldrig denne frist. Tidsfristen har
prioritet over flere indgående ændringer. Luk senderen og await opgaven ved
normal lukning: sidste ændring flushes straks. Drop/abort simulerer nedbrud.
Kalderen skal overvåge opgavens `Result`; lagringsfejl returneres, aldrig skjules.
Garantien forudsætter en fungerende database og en kørende runtime; selve
transaktionen har naturligvis I/O-latens. `NotesPersistence` er en lille trait,
så klokke- og fejltests kan bruge en deterministisk hukommelsesfake.

GPUI opretter nye blokke med optagelsesklokkens aktuelle offset og beholder
samme offset ved redigering. Der følger intet UI med denne ændring.

## Tidsopslag og fuldtekstsøgning (#31)

`NotesStore::notes_around(meeting_id, t_ms, window_ms).await?` læser mødet og
returnerer tidsstemplede blokke med `abs_diff(offset, t_ms) <= window_ms` i
dokumentrækkefølge. Begge grænser er inklusive, også ved `u64::MAX`.
`Document::around` udfører samme rene beregning uden database.

FTS5 kan fortsat indeksere `meeting_notes.notes_markdown` og behøver ikke kende
bloktabellen. Kopien opdateres atomisk, så triggerne fra #31 ser det nye indhold
sammen med blokkene. Markdown-metadata kan optræde som tokens; søgeresultater og
snippets bør vises via modellen, hvis markørerne skal skjules.

## Test

`cargo test -p tingskriver-core --test notes` dækker alle bloktyper og escaping,
512 property-eksempler med vilkårlig Unicode, 11 syntetiske BlockNote-fixtures
med faste markdown-facit, bevaring af original-JSON, idempotens, isoleret fejl,
doven migrering, transaktionsrollback, cascade, tidsgrænser og autosave.
Databaseprøver bruger in-memory SQLite med hele det rigtige migrationssæt.
Den deterministiske autosave-test sender ændringer hver 100 ms, ser seneste
ændring gemt præcis ved 2 s og simulerer nedbrud ved 3,999 s uden ny gemning.
