# Contributing to Tingskriver

Thank you for helping. Tingskriver is a local meeting assistant for Windows: recording, transcription and summaries run on the user's own machine. Issues and pull requests are welcome, in Danish or English.

## Read first

- [AGENTS.md](AGENTS.md): rules for everyone who changes code, and the table from a changed component to the CI check that covers it. Each crate has its own `AGENTS.md` (Ejer / Ejer ikke / Invarianter):
  [core](crates/tingskriver-core/AGENTS.md), [app](crates/tingskriver-app/AGENTS.md), [overlay](crates/tingskriver-overlay/AGENTS.md), [host](crates/tingskriver-host/AGENTS.md) and [installer](installer/AGENTS.md).
- [docs/testing.md](docs/testing.md): the test policy (Danish).
- [REVIEW.md](REVIEW.md): what reviewers check (Danish).
- [docs/arkitektur.md](docs/arkitektur.md) for the crates, [docs/BUILDING.md](docs/BUILDING.md) for building, [docs/agenter.md](docs/agenter.md) for building and reading CI from Linux.

## Branches

- `main` is the only long-lived branch. It must always build.
- Create a branch from `main` for each change: `fix/<topic>`, `enhance/<topic>` or `issue/<number>-<topic>`.
- Pushes to branches other than `main` build without making a release. A `v*` tag makes a release, and the maintainer owns that.

## Getting started

1. Fork the repository and clone your fork:
   ```bash
   git clone https://github.com/<your-user>/tingskriver.git
   cd tingskriver
   git remote add upstream https://github.com/mahope/tingskriver.git
   ```
2. Branch from `main`:
   ```bash
   git fetch upstream
   git checkout -b fix/your-topic upstream/main
   ```
3. Build and test as described in [docs/BUILDING.md](docs/BUILDING.md) and [docs/testing.md](docs/testing.md).

## Issues

Check for an existing issue first. A good issue has a clear title, steps to reproduce (for bugs), expected and actual behaviour, the Windows and Tingskriver version, and the log from `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\` if it is relevant. Remove personal data from logs and screenshots before you attach them.

## Pull requests

1. One change per pull request, from your branch to `main`.
2. Put `Fixes #123` in the description for each issue it closes (use `Refs #123` if a criterion is not met).
3. Fill in the PR template. It links to the [test policy](docs/testing.md).
4. State which checks from the [AGENTS.md](AGENTS.md) table you ran. For a check that could not run, write the command and the reason.
5. CI must be green, and a maintainer reviews against [REVIEW.md](REVIEW.md).
6. Keep the branch up to date with `main`. Squash commits if asked.

## Code style

- Follow the existing style of the file you edit. Format only the files you touch (`rustfmt --edition 2021 <file>`), and keep new code clippy-clean.
- User-facing text is Danish with æøå and lives in `tingskriver_core::strings` ([docs/dansk-tekst.md](docs/dansk-tekst.md)).
- Logic goes in `crates/tingskriver-core`, which depends on no UI crate. The UI crates call it.
- Do not add telemetry or network calls the user did not ask for.
- No secrets, personal data or private paths in code, tests, docs or screenshots. Use synthetic fixtures.

## Commit messages

Short, imperative subject of at most 72 characters, for example `Fix mute switching the microphone`. The body explains why. One logical change per commit.

## Porting code

If you copy code from another project, record it under "Porteret fra" in [NOTICE.md](NOTICE.md) (upstream repo, commit, files, copyright holder, licence). GPL, AGPL and unknown licences need written approval from the maintainer first.

## Getting help

Open an issue with your question.

## License

By contributing, you agree that your contributions are licensed under the project's MIT License ([LICENSE.md](LICENSE.md)).
