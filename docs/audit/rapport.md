# Aflevering af synlighedsrevisionen

Revisionen for #169 er skrevet med 62 funktionsrækker, en konkret rækkefølge og overdragelser til #171, #170 og færdiggørelse af #105 på PR #168. Tre issue-udkast dækker desktop-modelhåndtering, GPUI-aflevering og en fast synlighedskontrol. Ingen produktkode er ændret, og ingen issues, PR'er, merges eller deploys er udført.

## Resultat og kontrol

- `docs/plans/feature-synlighed-audit.md` skelner main, fire åbne PR-grene og planlagte funktioner. Alle 171 eksisterende issues/PR'er fra åbne/lukkede API-snapshot er indekseret med titel og status.
- De eksisterende UI-issues genbruges. #134 og #139 er Android-opgaver; desktop-modeldownload er et særskilt hul. #70 er lukket med core/Tauri, men GPUI-aflevering mangler. #36 er lukket og async-arbejde findes på main, selv om den ældre PR #151 stadig står åben.
- Tre Markdown-udkast matcher JSON-felterne præcist. `jq -e` godkender listen og felttyperne. Alle labels findes blandt repoets eksisterende issue-labels; alle issuehenvisninger findes i de hentede snapshots.
- Strukturel kontrol består for alle 62 tabelrækker, lokale Markdown-links, UTF-8 og kendte mønstre for private stier/hemmeligheder. Indholdet er desuden gennemlæst for offentligt scope; ingen rigtige mødedata eller nye personoplysninger er hentet eller indsat.
- `git diff origin/main --check` består. Kun filer under `docs/audit/` og revisionsdokumentet indgår i ændringen.
- Ingen pakker er installeret. Ingen Cargo-/frontend-build, produkttest eller GUI-test er kørt, da ændringen alene er dokumentation. Ingen teststatus for produktfunktionerne påstås.
- Efter første push på `31ab526` viste det offentlige Actions-API **Branch build: in_progress** og **Rust: in_progress**. Det er en observation af igangværende CI, ikke et grønt resultat. Den aktuelle CI-status skal kontrolleres før sammenfletning.

Review og oprettelse af PR/issues er reserveret til den efterfølgende gennemgang. Kontrollér nye issues igen før import af `docs/audit/issues.json`, så arbejde oprettet efter snapshot ikke dubleres. Windows, UIA, rigtige AI-klienter, installer og fejlinjiceret produktadfærd hører til implementeringsjobbenes verifikation.

## PR title

Audit feature visibility and draft uncovered GPUI work

## PR description

Audit 62 feature and visibility checkpoints across main and all four open pull-request branches. Distinguish implemented core/Tauri behavior, GPUI placeholders, unmerged work and planned features. Map gaps to existing issues and include the complete 171-item issue/PR snapshot used for duplicate checks.

Add concrete handoffs for the About page, visible Velopack updates and completion of MCP settings on PR #168. Provide three import-ready Danish issue drafts for desktop model management, GPUI meeting delivery and a recurring feature-visibility checklist. Each draft has a matching Markdown file and JSON object using existing labels.

Validation: `jq -e` accepts the issue list and field types; structural checks pass for 62 matrix rows, exact JSON/Markdown correspondence, existing issue references and labels, local document links, UTF-8 and public-content patterns. `git diff origin/main --check` passes. No product code changed, no dependencies installed, and no product build or runtime tests were run. CI was still in progress when observed after the checkpoint push; no green CI result is claimed.

Open items: fresh-context review, current CI results, and issue/PR creation remain with the follow-up workflow. Implementation and native Windows verification remain in their respective issues; this PR does not close them. Recheck the issue list before importing the drafts.

Rollback: revert this documentation change; it has no schema, runtime or deployment effect.

Fixes #169.

Refs #7, #9, #19, #22, #33, #36, #45, #47, #49, #50, #51, #53, #55, #56, #62, #64, #68, #73, #74, #83, #84, #85, #86, #105, #130, #134, #137, #139, #151, #152, #168, #170, #171.
