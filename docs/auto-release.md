# Automatic releases

Every 8 hours (`0 */8 * * *`, UTC) the workflow `.github/workflows/auto-release.yml` publishes a new pre-release of the GPUI app when there is something to publish. It uses the normal release process: the tag `app-v<version>` and `app-release.yml` (Velopack, see [installer/README.md](../installer/README.md)). Nothing about signing or packaging changes.

## What triggers a release

All of these must hold, otherwise the run ends without a release and the reason is in the job summary:

1. **Changes:** `main` has commits after the newest `app-v*` tag, not counting our own `Release <version>` commits.
2. **Green CI:** every push workflow of the commit under the release commit has finished without failure or cancellation, and the `Rust` workflow succeeded. A run that is still going is awaited for up to 90 minutes. Scheduled workflows (the 3-hour soak, benchmarks) are not part of the verdict. If `main` moves while CI is awaited, the run ends quietly and the next run starts over.
3. **Nothing stuck:** changes are counted after the latest release attempt, which is the newest `app-v*` tag or a newer `Release` commit. If that attempt has no published release and nothing has been merged since, the run **fails** instead of burning another version for the same content:
   - the tag exists but the build failed (`last-attempt-failed`): look at the failed `app-release.yml` run for the tag, fix the cause, and run that workflow on the tag again (Actions, GPUI app release, Run workflow, pick the tag), or merge a change to get a new version;
   - the `Release` commit is on `main` but its tag was never pushed (`bump-without-tag`): tag that commit by hand (`git tag -a app-v<version> <commit>`, push the tag) and run `app-release.yml` on it.
   Nothing is moved or deleted. A release that is still being built, or was pulled back to a draft, ends the run quietly (`release-building`, `release-is-draft`).
4. **No stable release in the making:** if `Cargo.toml` holds a stable version whose `app-v<version>` tag does not exist yet (a manual release waiting for its sign-off), the run skips (`stable-pending`).

## Version

One micro step above the highest version anywhere, always a pre-release: `0.5.0-dev.3` becomes `0.5.0-dev.4`. "Anywhere" means every `app-v*` and `v*` tag and the version in `Cargo.toml`, so the number never goes backwards and a tag that was pushed but never published is skipped, not reused. After a stable `x.y.z` that someone released by hand, the series continues with `x.y.(z+1)-dev.1`.

**Stable versions (no suffix) are never produced automatically.** [release-checkliste.md](release-checkliste.md) requires a manual performance sign-off on the reference machine for those, and the automation bypasses it for pre-releases only. To release a stable version, follow the manual steps in the installer README; the automation then carries on with the next pre-release series.

The workflow commits `Release <version>` to `main` (the workspace version in `Cargo.toml`, the matching `version` lines in `Cargo.lock`, and `docs/changelog/<version>.md`), tags that commit (annotated) and pushes both. The bump is checked before the commit: only those version lines may change.

## Changelog

`docs/changelog/<version>.md` is generated from the merged pull requests since the previous published release (titles and links, grouped by the first word of the title into Tilføjet, Ændret, Rettet and Fjernet). The same text is appended to the GitHub release description.

## How the build is started

A tag pushed with the workflow token does not start other workflows. The workflow therefore dispatches `app-release.yml` on the tag (`gh workflow run app-release.yml --ref app-v<version>`; the version check in that workflow sees a tag ref and publishes), waits for it, and then checks the result:

- the release is not a draft and is a pre-release when the version has a suffix,
- it has `Tingskriver-Setup.exe`, `TingskriverApp-<version>-full.nupkg`, a `-delta.nupkg` (when an earlier GPUI release exists) and `releases.win.json`,
- the ten newest releases, which is all the app's feed reads, join into a feed that offers the new version as Full (and Delta) and nothing higher,
- no `app-v*` release is marked Latest (the Tauri updater still reads `releases/latest`).

## Switching it off and on

- Stop the schedule: set the repository variable `AUTO_RELEASE` to `false` (Settings, Secrets and variables, Actions, Variables). Delete it or set anything else to start again.
- Or disable the workflow: `gh workflow disable auto-release.yml` (`enable` to resume).
- A manual run (Actions, Auto release, Run workflow) ignores `AUTO_RELEASE` and defaults to a dry run that does everything except commit, tag and publish: plan, version bump, `cargo update`, bump check and changelog. Untick "dry run" to release now.

## Good to know

- Each release commit touches the workspace `version` lines in `Cargo.lock`. A pull request that adds a dependency right below a workspace crate's version line can get a trivial conflict; merge `main` into the branch.
- The bump commit is pushed with the workflow token, so it gets no CI run. The CI verdict is taken on the commit below it. `scripts/ci-status.sh` on `main` will find nothing for that commit.
- `frontend/package.json` and `frontend/src-tauri/tauri.conf.json` belong to the Tauri app and are not touched; the Tauri release workflow sets its own version from its tag.
- With three pre-releases a day, the "fewer than ten releases in between" rule in the installer README's Updates section applies as soon as a stable Velopack release exists: the app then reads only the ten newest releases of any kind and filters out pre-releases. Release stable versions promptly or give the app a fixed feed first.
- The Tauri-to-GPUI move (`frontend/src-tauri/src/move_to_gpui.rs`) installs the highest non-draft `app-v*` release, pre-releases included. Once a Tauri release containing that code ships, Tauri users are moved to whatever the automation built last.

## Local checks

`python3 scripts/tests/test_auto_release.py` (version maths, release decision, changelog, CI verdict, diff check) and `python3 scripts/auto_release.py verify --tag app-v<version> --version <version>` for a published release. A wrong release is fixed forward with the next version, or pulled with `gh release edit app-v<version> --draft`.
