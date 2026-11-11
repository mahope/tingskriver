# tingskriver-app

GPUI-appen: skallen, navigation og skærme. Skallens status og GPUI-versionslåsen: [docs/gpui.md](../../docs/gpui.md). Rodens regler: [AGENTS.md](../../AGENTS.md).

## Ejer

- Vindue, sidebjælke, temaer (lys og mørk), skrifttyper, vinduesplacering, `--hidden` og enkeltinstans.
- Skærme og navigation, bygget med `gpui` og `gpui-component`.
- Opstart af databasen i baggrunden og fejlsiden, hvis den ikke kan åbnes.
- Opdateringskald til Velopack (`update.rs`) og nedbrudsrapport (`crash.rs`).

## Ejer ikke

- Logik: optagelse, transskription, database, resumé hører til `tingskriver-core`. Appen kalder core.
- Tastaturhook, globale genveje og tray: `tingskriver-host`.
- Diktér-pillen: `tingskriver-overlay`.
- Pakning og installer: `installer/`.

## Invarianter

- GPUI-trådens event loop blokeres aldrig. Core kaldes kun fra appens egen tokio-runtime, aldrig fra UI-tråden.
- Der er én version af `gpui*` i `Cargo.lock`, og den matcher gpui-components egen lås. Opgradér kun GPUI sammen med gpui-component (`scripts/check-gpui-lock.py`).
- Ny brugertekst er dansk med æøå og kommer fra `tingskriver_core::strings`.
- Nye skærme har tastatur- og skærmlæserstøtte, lys og mørk tilstand, tom tilstand og fejltilstand.
- Ny eller ændret brugeradfærd kan ses og styres i appen (indstillinger, Om-siden, ændringslog).
- Appen sender ingen data af sted uden brugerens valg. Ingen telemetri.

## Test

Windows er hovedplatformen; byg og kør dér. Fra Linux: `cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-app`. Det, der kun kan prøves med en rigtig skærm, skrives som manuel testplan i PR'en ([docs/testing.md](../../docs/testing.md)).
