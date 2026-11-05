# tingskriver-overlay

Pillen og optageindikatoren: et native Win32 layered window. Rodens regler: [AGENTS.md](../../AGENTS.md). Placering i afhængighedskæden: [docs/arkitektur.md](../../docs/arkitektur.md).

## Ejer

- Tegning og layout af pillen (`render.rs`, `layout.rs`, `text.rs`, `theme.rs`).
- Pillens model og tilstande (`model.rs`).
- Win32-vinduet (`windows.rs`): layered window, placering, levetid.

## Ejer ikke

- Hvornår pillen vises, og hvad dikteringen gør: det styrer `tingskriver-core` (`dictation`).
- Tastaturhook, genveje og tray: `tingskriver-host`.
- GPUI-skærmene: `tingskriver-app`. Overlayet afhænger ikke af appen.

## Invarianter

- Overlayet stjæler aldrig fokus fra det vindue, brugeren skriver i.
- Overlayet viser tilstand; det ændrer den ikke. Ingen forretningslogik her.
- Layout og model kan testes uden Windows. Kun `windows.rs` kræver en Windows-session.
- Tekst i pillen er dansk og kommer fra core's `strings`.

## Test

`cargo test --locked -p tingskriver-overlay`. `tests/windows_overlay.rs` kører kun på Windows. Fra Linux: `cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-overlay`.
