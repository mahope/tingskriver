# Automatic releases

Every 8 hours (`0 */8 * * *`, UTC) the workflow `.github/workflows/auto-release.yml` publishes a new pre-release of the GPUI app when there is something to publish. It uses the normal release process: the tag `app-v<version>` and `app-release.yml` (Velopack, see [installer/README.md](../installer/README.md)). Nothing about signing or packaging changes.

## What triggers a release

All three must hold, otherwise the run ends without a release and the reason is in the job summary:

1. **Changes:** `main` has commits since the last published `app-v*` release, not counting earlier release commits.
2. **Green CI:** every push workflow of the commit under the release commit has finished without failure or cancellation, and the `Rust` workflow succeeded. A run that is still going is awaited for up to 90 minutes. Scheduled workflows (the 3-hour soak, benchmarks) are not part of the verdict.
3. **A pre-release version:** the highest version is `x.y.z-dev.N` (or another suffix ending in a number). Stable versions need the manual performance sign-off in [release-checkliste.md](release-checkliste.md); set the repository variable `AUTO_RELEASE_STABLE` to `true` to let the workflow step stable versions as well (`x.y.z` becomes `x.y.(z+1)`).

## Version

One micro step above the highest version anywhere: `0.5.0-dev.3` becomes `0.5.0-dev.4`. "Anywhere" means every `app-v*` and `v*` tag and the version in `Cargo.toml`, so the number never goes backwards and a tag that was pushed but never published is skipped, not reused. The workflow commits `Release <version>` to `main` (the workspace version in `Cargo.toml`, the matching lines in `Cargo.lock`, and `docs/changelog/<version>.md`), tags that commit (annotated) and pushes both. It aborts if `main` moved in between.

## Changelog

`docs/changelog/<version>.md` is generated from the merged pull requests since the previous release (titles and links, grouped by the first word of the title into Tilføjet, Ændret, Rettet and Fjernet). The same text is appended to the GitHub release description.

## How the build is started

A tag pushed with the workflow token does not start other workflows. The workflow therefore dispatches `app-release.yml` on the tag (`gh workflow run app-release.yml --ref app-v<version>`; the version check in that workflow sees a tag ref and publishes), waits for it, and then checks the result:

- the release is not a draft and is a pre-release when the version has a suffix,
- it has `Tingskriver-Setup.exe`, `TingskriverApp-<version>-full.nupkg`, a `-delta.nupkg` (when an earlier GPUI release exists) and `releases.win.json`,
- the ten newest releases, which is all the app's feed reads, join into a feed that offers the new version as Full (and Delta) and nothing higher,
- no `app-v*` release is marked Latest (the Tauri updater still reads `releases/latest`).

## Switching it off and on

- Stop the schedule: set the repository variable `AUTO_RELEASE` to `false` (Settings, Secrets and variables, Actions, Variables). Delete it or set anything else to start again.
- Or disable the workflow: `gh workflow disable auto-release.yml` (`enable` to resume).
- A manual run (Actions, Auto release, Run workflow) ignores `AUTO_RELEASE` and defaults to a dry run that only prints the version and changelog. Untick "dry run" to release now.

## When something goes wrong

- A failed build leaves a tag without a release. The number is burned; the next run uses the next one. Nothing is moved or deleted.
- The run fails visibly (GitHub mails failed scheduled runs) if the release does not pass the checks above. The release may then exist but be wrong: fix forward with the next version, or pull it with `gh release edit app-v<version> --draft`.
- Local checks: `python3 scripts/tests/test_auto_release.py` (version maths, changelog, CI verdict, diff check) and `python3 scripts/auto_release.py verify --tag app-v<version> --version <version>` for a published release.
