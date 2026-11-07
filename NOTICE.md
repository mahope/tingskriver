# Notice

Tingskriver is a fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) (version 0.4.1), an open source project by Zackriya Solutions.

Meetily is licensed under the MIT License:

> Copyright (c) 2024 Zackriya Solutions

The full license text is in [LICENSE.md](LICENSE.md). It is kept unchanged, and it also covers the changes made in Tingskriver.

Tingskriver is not affiliated with, endorsed by or sponsored by Zackriya Solutions. "Meetily" is the name and trademark of Zackriya Solutions and is not used to name or promote Tingskriver.

Tingskriver is maintained by Mahope (Mads Holst Jensen). Licenses of bundled and downloaded third-party components are listed in [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

## Porteret fra

Kode, der er overtaget fra et andet projekt, registreres her med upstream-repo, uforanderlig revision, filer, ophavsretsindehaver og licens. Registreringen opdateres i samme PR som koden. Ingen AGPL-, GPL- eller ukendt-licenseret kode kommer ind uden skriftlig godkendelse fra ejeren. Idéer lært af andre projekter er ikke portering; kopieret kode er. Afhængigheder, der hentes af Cargo, står i `Cargo.lock` og kontrolleres af `deny.toml`. Bundlede og downloadede komponenter står i [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

| Upstream | Revision | Filer | Ophavsret | Licens |
|---|---|---|---|---|
| [Meetily](https://github.com/Zackriya-Solutions/meetily) | Version 0.4.1 (hele træet ved forken) | Hele repoets udgangspunkt: `frontend/` (Tauri-appen), `backend/` (arkiveret) og den første lyd-, transskriptions- og resumékode, som siden er flyttet til `crates/tingskriver-core` og omskrevet i stor udstrækning | Copyright (c) 2024 Zackriya Solutions | MIT ([LICENSE.md](LICENSE.md)) |

Kun Meetily er kendt som portet kode i dag. Den nøjagtige upstream-commit for forken er ikke registreret i git-historikken; første kommit i dette repo er fra 2024-12-26. Tilføj en række for hver ny portering, med fuld commit-hash.
