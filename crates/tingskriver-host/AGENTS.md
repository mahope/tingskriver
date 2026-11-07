# tingskriver-host

Det, der ikke må vente på GPUI's event loop: diktérhook, globale genveje, tray og broen til core. Detaljer: [docs/host.md](../../docs/host.md). Rodens regler: [AGENTS.md](../../AGENTS.md).

## Ejer

- Host-tråden med egen Win32-meddelelsesløkke: tray-ikon og menu, `RegisterHotKey`-genveje (`win.rs`, `hotkeys.rs`).
- `Bridge` (`bridge.rs`): GPUI-appens implementering af `AppHost`.
- Status til tray (`status.rs`) og lyttere på genvejsændringer (`watch.rs`).

## Ejer ikke

- Selve tastaturhooken (`WH_KEYBOARD_LL`) og dikteringsforløbet: de ligger i `tingskriver-core` (`dictation`).
- Pillens tegning: `tingskriver-overlay`.
- Vinduer og skærme: `tingskriver-app`. Host afhænger ikke af GPUI eller Tauri.

## Invarianter

- Hooken svarer på hvert tastetryk inden for ca. 1 ms, uanset hvad UI'et laver. Intet tungt arbejde på hooktråden.
- Intet, der tastes, ses, gemmes, sendes eller logges. Loggen får genvejsetiketter og tal, aldrig tastenavne eller -koder (`tests/hook_through_host.rs`).
- Andre tråde taler til host-tråden via en kø og en besked til dens vindue, ikke til tråden, så beskeder overlever modale løkker som den åbne tray-menu.
- Indstillingsfiler skrives via en midlertidig fil og omdøbning.

## Test

`cargo test --locked -p tingskriver-host`. Hook-testen kører kun på Windows. Fra Linux: `cargo xwin check --target x86_64-pc-windows-msvc -p tingskriver-host`.
