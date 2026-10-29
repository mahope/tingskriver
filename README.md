# Tingskriver

## Dansk

Tingskriver optager og transskriberer møder på din Windows-computer, og alt kører lokalt. Lyd, transskription og resuméer forlader ikke maskinen, medmindre du selv vælger en ekstern AI-tjeneste. Programmet er lavet til danske brugere, er gratis og open source, og der er ingen Pro-version eller betalingsmur.

Projektside: <https://tingskriver.mahoje.dk>

### Installation

1. Hent installationsfilen (`.exe`) fra [den seneste udgivelse](https://github.com/mahope/tingskriver/releases/latest).
2. Kør filen. Installationsfilen er ikke signeret, så Windows SmartScreen viser en advarsel. Vælg "Flere oplysninger" og derefter "Kør alligevel".
3. Programmet opdaterer sig selv fra GitHub Releases.

Tingskriver understøtter kun Windows 11 (x64). Udgivelserne indeholder ikke macOS- eller Linux-versioner, selvom koden fra det oprindelige projekt stadig ligger i repoet.

### Om projektet

Tingskriver er en fork af [Meetily](https://github.com/Zackriya-Solutions/meetily) fra Zackriya Solutions (MIT-licens). Tingskriver har ingen tilknytning til Zackriya Solutions og er hverken godkendt eller sponsoreret af dem. Se [NOTICE.md](NOTICE.md), [LICENSE.md](LICENSE.md) og [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for licenser og tredjepartskomponenter, herunder FFmpeg (GPL-3.0).

### Udvikling

- Udviklingen foregår på Forgejo: `git.mahoje.dk/mahope/tingskriver`. GitHub-repoet [mahope/tingskriver](https://github.com/mahope/tingskriver) er et spejl.
- Et tag på formen `v*` bygger en udgivelse på GitHub Actions og lægger installationsfilen på GitHub Releases.
- Pushes til andre grene end `main` bygger uden at lave en udgivelse.
- Byggevejledning: [docs/BUILDING.md](docs/BUILDING.md). Frontenden ligger i `frontend/` (Tauri 2, Next.js, Rust).

## English

Tingskriver is a privacy-first meeting recorder and transcriber for Windows that runs locally. Audio, transcripts and summaries stay on your machine unless you choose an external AI service yourself. It is made for Danish users, is free and open source, and has no Pro version or paywall.

Project page: <https://tingskriver.mahoje.dk>

### Installation

1. Download the installer (`.exe`) from the [latest release](https://github.com/mahope/tingskriver/releases/latest).
2. Run it. The installer is unsigned, so Windows SmartScreen shows a warning. Choose "More info" and then "Run anyway".
3. The app updates itself from GitHub Releases.

Tingskriver supports Windows 11 (x64) only. Releases contain no macOS or Linux builds, although the code inherited from the upstream project is still in the repository.

### About

Tingskriver is a fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) by Zackriya Solutions (MIT License). Tingskriver is not affiliated with, endorsed by or sponsored by Zackriya Solutions. See [NOTICE.md](NOTICE.md), [LICENSE.md](LICENSE.md) and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for licenses and third-party components, including FFmpeg (GPL-3.0).

### Development

- Development happens on Forgejo: `git.mahoje.dk/mahope/tingskriver`. The GitHub repository [mahope/tingskriver](https://github.com/mahope/tingskriver) is a mirror.
- Pushing a `v*` tag builds a release on GitHub Actions and publishes the installer on GitHub Releases.
- Pushes to branches other than `main` build without creating a release.
- Build instructions: [docs/BUILDING.md](docs/BUILDING.md). The app lives in `frontend/` (Tauri 2, Next.js, Rust).
