# Notice

Tingskriver is a fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) (version 0.4.1), an open source project by Zackriya Solutions.

Meetily is licensed under the MIT License:

> Copyright (c) 2024 Zackriya Solutions

The full license text is in [LICENSE.md](LICENSE.md). It is kept unchanged, and it also covers the changes made in Tingskriver.

The installer smoke test (`.github/workflows/installer-smoke.yml` and `installer/smoke-test.ps1`) follows the steps of the desktop CI workflow of [anarlog](https://github.com/fastrepl/anarlog) (`desktop_ci.yaml`): refuse to run over an existing installation, silent install, check the uninstall entry, start the app and require that it stays up, silent uninstall, check that files and the entry are gone, upload the installer and its SHA-256, and print the event log tail on failure. The code is written for Tingskriver, but the idea and the sequence come from there. anarlog is licensed under the MIT License:

> Copyright (c) Fastrepl, Inc.

Tingskriver is not affiliated with, endorsed by or sponsored by Zackriya Solutions. "Meetily" is the name and trademark of Zackriya Solutions and is not used to name or promote Tingskriver.

Tingskriver is maintained by Mahope (Mads Holst Jensen). Licenses of bundled and downloaded third-party components are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## Porteret fra

Kode, der er overtaget fra et andet projekt, registreres her med upstream-repo, uforanderlig revision, filer, ophavsretsindehaver og licens. Registreringen opdateres i samme PR som koden. Ingen AGPL-, GPL- eller ukendt-licenseret kode kommer ind uden skriftlig godkendelse fra ejeren. Idéer lært af andre projekter er ikke portering; kopieret kode er. Afhængigheder, der hentes af Cargo, står i `Cargo.lock` og kontrolleres af `deny.toml`. Bundlede og downloadede komponenter står i [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

| Upstream | Revision | Filer | Ophavsret | Licens |
|---|---|---|---|---|
| [Meetily](https://github.com/Zackriya-Solutions/meetily) | Version 0.4.1, commit `a2cb62e827da7ef59f65064c97233efb2313878e` (tag `v0.4.1`; hele træet ved forken) | Hele repoets udgangspunkt: `frontend/` (Tauri-appen), `backend/` (arkiveret) og den første lyd-, transskriptions- og resumékode, som siden er flyttet til `crates/tingskriver-core` og omskrevet i stor udstrækning | Copyright (c) 2024 Zackriya Solutions | MIT ([LICENSE.md](LICENSE.md)) |

Kun Meetily er kendt som portet kode i dag. Forkens første egne commits har `a2cb62e` som forælder; Meetilys historik ligger i dette repos git-historik (første kommit er fra 2024-12-26). Tilføj en række for hver ny portering, med fuld commit-hash.
