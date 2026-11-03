# Dansk tekst og formater

Nye GPUI-skærme fra issue #47 og frem skal bruge `tingskriver_core::strings`
til brugertekst og `tingskriver_core::format_da` til visningsformater.
Core afhænger ikke af UI-crates, og kontrollerne kører på Linux i det eksisterende
CI-job `Test tingskriver-core` samt i Windows-workspace-testen.

## Tilføj en streng

Tilføj en dansk konstant i `catalogue!` i `crates/tingskriver-core/src/strings/mod.rs`.
Makroen registrerer automatisk alle tekster i `ALL`, som brandtesten gennemløber.
Brug konstanten direkte, fx `.child(strings::MEETINGS)`.
Parametriseret tekst skrives som en registreret skabelon med `{value}` og en
funktion i `parameterized!`, fx `strings::version(version)`.
Sammensæt ikke sætninger med `format!` i komponenter. Parameterværdier er data;
kataloget oversætter eller validerer ikke eksterne data.

`ui.rs` er migreret uden ændring af layout. Eksisterende tekst i `probe.rs`,
`crash.rs`, `startup.rs` og `update.rs` ligger uden for denne migrering og skal
flyttes, når de tilhørende skærme omskrives. Logtekster og tekniske id'er er
ikke katalogtekst.

## Formatregler

| Funktion | Eksempel / regel |
| --- | --- |
| `date` | `3. november 2026` |
| `date_with_weekday` | `tirsdag den 3. november 2026` |
| `short_date` | `3. nov. 2026` |
| `time` | `kl. 14.30`, midnat `kl. 00.00`, middag `kl. 12.00` |
| `date_time` | `3. november 2026 kl. 14.30` |
| `integer` | Eksakte i64-værdier med punktum som tusindtalsseparator |
| `number(value, decimals)` | `1.000,50`, fast antal decimaler, højst 15 |
| `percent(value, decimals)` | `12,5 %` for input `12.5`, ikke `0.125` |
| `duration(seconds)` | `12 s`, `45 min`, `1 t`, `1 t 5 min` |
| `file_size(bytes)` | SI-enheder: 1 KB = 1.000 B, `1,5 KB`, `1,2 MB` |

Datoer og tider er lokale `chrono::NaiveDate/NaiveTime/NaiveDateTime`;
kalderen foretager eventuel tidszonekonvertering. Sekunder vises ikke i klokkeslæt.
Varigheder afkortes til hele minutter fra 60 sekunder. Filstørrelser har én
decimal fra KB og understøtter B, KB, MB og GB. Enheden vælges før afrunding.
Flydende tal bruger Rusts faste decimalafrunding (ties-to-even for præcise
halvvejsværdier); binære f64-tal kan ikke repræsentere alle decimaltal præcist.
Negativt nul normaliseres. NaN og uendelighed vises som `—`.
Talte tal i `dictation/` har andre regler og berøres ikke.

## Tests og lint

Kør `cargo test -p tingskriver-core` eller målrettet
`cargo test -p tingskriver-core --test danish_ui` og
`cargo test -p tingskriver-core format_da`.

`tests/danish_ui.rs` kontrollerer hele `strings::ALL` for det gamle brandnavn
uden forskel på store og små bogstaver. En positiv kontrol beviser detektionen.
En ekstra kontrol undersøger alle Rust-kilder i app og overlay, inklusive
kommentarer og tests. React-kilderne i `frontend/` er ikke omfattet.

Linten gennemløber rekursivt `crates/tingskriver-app/src/**/*.rs` og fanger
strengliteraler med bogstaver i `child`, `label`, `title`, `placeholder`,
`tooltip`, `Label::new`, `SharedString::from`, `format!` og de eksisterende
tekst-wrappere i `ui.rs`. Fejl viser fil og linje og henviser til kataloget.
Positive kontroller bruger syntetiske kilder med rå tekst, katalogkald,
logmakroer, miljømakroer og kommentarer. Betingede tekster i `Label::new` og
`SharedString::from` kontrolleres også. Linten ændrer ikke kildefiler.

Undtagelser står kun i `ALLOW` i samme testfil som eksakte par af relativ
filsti og literal. Listen omfatter tekniske IPC-stier, element-id'er,
diagnostik og enkelte eksisterende formater i filer uden for issue #65's
filejerskab. Tilføj aldrig en hel fil eller et generelt mønster som undtagelse.
Ny brugertekst skal i kataloget; eksisterende undtagelser fjernes ved migrering.

Linten er en leksikalsk heuristik, ikke en Rust-parser. Den følger ikke
variabler, aliaser, brugerdefinerede wrappere eller makroudvidelser og er
ikke et bevis for, at enhver mulig tekst er dansk. Den ignorerer kommentarer,
log-/miljømakroer og items markeret med `#[cfg(test)]`. Sammensatte cfg-udtryk
analyseres ikke. Escape-sekvenser uden synlige bogstaver tæller ikke som tekst.
Komplekse rå strenge og indlejrede blokkommentarer kan kræve en stærkere lexer.
