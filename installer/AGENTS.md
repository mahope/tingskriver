# installer

Windows-installer og opdatering til GPUI-appen, bygget med Velopack. Beskrivelsen står i [installer/README.md](README.md). Rodens regler: [AGENTS.md](../AGENTS.md).

## Ejer

- Pakning (`pack.ps1`) og installerens indhold.
- Pakke-id'et `TingskriverApp` og installationsmappen.
- Udgivelsesflowet i `.github/workflows/app-release.yml`.

## Ejer ikke

- Hook-logikken ved installation, opdatering og afinstallation: den ligger i `crates/tingskriver-core/src/install/` og kaldes fra `crates/tingskriver-app/src/update.rs`.
- Versionsnummeret: udgivelsesprocessen ejer det i `Cargo.toml`.
- Signeringsnøgler, certifikater og GitHub-secrets: ændres aldrig i en almindelig PR.

## Invarianter

- Pakke-id'et `TingskriverApp` ændres aldrig: opdateringer, installationsmappen og afinstallationsposten hænger på det.
- En opdatering rører aldrig brugerens data (møder, indstillinger, modeller, nøgler). Data slettes kun ved afinstallation, og kun hvis brugeren har valgt det.
- Installationen kræver ikke administrator (pr. bruger).
- Intet gammelt Tauri-spor overskrives eller fjernes, før flytningen er besluttet (se README).
- Installeren kan kun prøves på Windows. Skriv en manuel testplan i PR'en, når du ændrer den.

## Test

Der er ingen automatiske tests her. Hook-logikken testes i core (`cargo test --locked -p tingskriver-core install`). Resten kontrolleres af CI-jobbet "GPUI app artifact" og en manuel prøve på Windows ([docs/testing.md](../docs/testing.md)).
