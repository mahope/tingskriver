# AI-forslag til noter og referat (#205)

Ingen AI-vej skriver direkte til et mødes noter eller referat. Referatgenskabelse,
assistent, chat, kommandotilstand og MCP-værktøjerne (`add_note`,
`regenerate_summary`) opretter et **forslag** med
`tingskriver_core::proposals::ProposalStore::create`. Først når brugeren trykker
Anvend, skriver `apply` ændringen.

## Model

- Tabel `meeting_proposals` (migration `20261105090000`): møde, felt (`notes` eller
  `summary`), grundversion (`base_content` og `base_updated_at`), det foreslåede
  indhold, en unified diff, kilde, status og begrundelse.
- Noter gemmes som note-markdown, én blok pr. linje. Linjer, som forslaget lader
  stå, får deres tidsstempel (#15) tilbage, så Anvend ikke sletter dem.
- Referat: forslaget ændrer `markdown` i referatet. Øvrige felter i resultatet er
  uændrede; `markdown` har forrang, hvor referatet vises.
- Status: `pending`, `applied`, `rejected`, `stale`, `expired`, `undone`.

## Regler

- `create` ændrer intet. `apply` og `undo` kræver et `UserConfirmed`, som kun
  UI'et opretter ud fra et klik (`UserConfirmed::from_ui_click`). Søg på den
  funktion for at se alle steder, hvor et menneske udløser en skrivning.
- **Forældet grundversion:** hvis noterne eller referatet er ændret, siden forslaget
  blev lavet, afvises forslaget automatisk (`stale`) med forklaring. Det sker ved
  visning (`pending`/`list`) og igen ved `apply`. Teksten afgør, ikke tidsstemplet
  (det har sekunders opløsning).
- **Fortryd** vender ændringen med en three-way merge: det, brugeren skrev efter
  Anvend, bevares. Har brugeren ændret de samme linjer, sker der ingenting, og
  brugeren får en forklaring.
- **Udløb:** chat- og kommandoforslag udløber, når sessionen lukkes
  (`close_session`) eller ved opstart (`expire_orphaned_sessions`). Forslag fra
  assistent, genskabelse og MCP overlever genstart.

## Brug fra en ny funktion

```rust
let store = ProposalStore::new(pool);
store.create(NewProposal {
    meeting_id,
    field: ProposalField::Summary,
    proposed: new_markdown,
    source: ProposalSource::new(SourceKind::Mcp).labelled("regenerate_summary"),
}).await?;
```

## UI

`tingskriver-app/src/proposals.rs` (`ProposalReview`) viser båndet «N ventende
ændringer», diff pr. forslag og knapperne Anvend, Afvis og Fortryd. Den skal
monteres i mødedokumentet (#51); siden findes ikke på `main` endnu, så visningen er
ikke koblet på navigationen.

## Afgrænsning (åbne punkter, #205)

- **Eksisterende skrivevej:** `summary::commands::api_save_meeting_summary` (Tauri,
  udfases) skriver stadig direkte til referatet, når brugeren gemmer. GPUI-appen har
  ingen genskab-referat-, MCP- eller assistentvej på `main` endnu; hver af dem skal
  bruge `ProposalStore::create` fra start (#44, #88, #78, #105). Koblingen på
  Tauri-stien er bevidst ikke lavet.
- **Visning:** `ProposalReview` er endnu ikke monteret, fordi mødedokumentet (#51)
  ikke findes på `main`. Skærmbilledtest, Om-/ændringslogpunkt og
  skærmlæserrækkefølge (#73, #49) skal laves samtidig med monteringen.
- **Udløb ved opstart** er koblet på `startup::open_database`. `close_session` skal
  kaldes af chat/kommandotilstand, når de lukker.
- **`UserConfirmed`:** konstruktøren er `pub`; testen `only_ui_code_creates_user_confirmed`
  fejler, hvis andre filer end modulet og review-visningen kalder den.
