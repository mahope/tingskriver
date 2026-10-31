# installer

The Windows installer and updater for the GPUI app (`crates/tingskriver-app`), built with [Velopack](https://docs.velopack.io). It replaces the Tauri updater and the NSIS installer (docs/SPEC.md, D6). Until the move described below, the Tauri app is still released from `frontend/` through `.github/workflows/release.yml`.

| Piece | Where |
|---|---|
| Packing (`vpk pack`) | `installer/pack.ps1` |
| CI and GitHub Release | `.github/workflows/app-release.yml` |
| Hooks and update checks in the app | `crates/tingskriver-app/src/update.rs` |
| What the hooks do (start with Windows, leftovers, user data) | `crates/tingskriver-core/src/install/` |

## What gets installed

`Tingskriver-Setup.exe` installs per user, without administrator, into `%LOCALAPPDATA%\TingskriverApp`:

```
%LOCALAPPDATA%\TingskriverApp\
  Update.exe                  Velopack's updater and uninstaller
  current\tingskriver-app.exe the app (plus licences); replaced on update
  packages\                   the installed full package, the base for deltas
```

The pack id `TingskriverApp` never changes: updates, the install folder and the uninstall entry (`HKCU\...\Uninstall\TingskriverApp`) are keyed by it. It differs from the Tauri app's folder and uninstall key (`Tingskriver`) on purpose, so the two can sit side by side during the move. Velopack creates the shortcuts `Tingskriver` on the Desktop and in the Start menu. The Tauri installer's shortcuts have the same name and are overwritten.

The user's data is not in the install folder and is never touched by an update: meetings, settings and models in `%APPDATA%\dk.mahoje.tingskriver`, logs in `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs`, API keys in Credential Manager (`Tingskriver/…`).

## Hooks

Velopack starts the app with a hook argument and waits for it to exit (`tingskriver_app::update::handle_installer_events`, the first thing `run()` does):

| Hook | What the app does |
|---|---|
| after install | Start with Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Tingskriver` = `"…\current\tingskriver-app.exe" --hidden`. Removes leftovers of earlier builds. |
| after update | Removes leftovers again. The Run value is only rewritten if it exists and points elsewhere, so a user who turned it off keeps it off. |
| before uninstall | Removes the Run value. Deletes the user's data and `Tingskriver/*` credentials only when the marker file `%APPDATA%\dk.mahoje.tingskriver\slet-data-ved-afinstallation` exists (`tingskriver_core::install::user_data::set_remove_on_uninstall`; the settings page can offer it). Otherwise everything stays for a reinstall. |

Leftovers of earlier builds (`tingskriver_core::install::legacy`): before 0.5.0-dev.1 the Tauri app was built as `meetily.exe` in `%LOCALAPPDATA%\Tingskriver`, and the rename left the old exe next to `tingskriver.exe`. The hooks remove

- `meetily.exe` lying next to `tingskriver.exe` in `%LOCALAPPDATA%\Tingskriver` or `%LOCALAPPDATA%\Programs\Tingskriver`,
- shortcuts (`.lnk`) in the Start menu, its `Startup` folder, `Start Menu\Programs\Tingskriver`, `…\meetily` and on the Desktop that point to a `meetily.exe` that no longer exists,
- `HKCU\…\Run` values that start a `meetily.exe` that no longer exists,
- `HKCU\…\Uninstall` entries that mention meetily and whose uninstaller is gone.

A working upstream Meetily installation (exe and uninstaller still there) is left alone.

## Updates

The app checks GitHub Releases 60 s after start and then every 6 hours (`update::start_background_checks`, only in an installed copy, only in the primary instance). The feed is the `releases.win.json` asset of each of the ten newest releases, joined, so Tauri releases in between do no harm. A newer version is downloaded in the background, as a delta package when the release has one for the installed version, otherwise as the full package. It is installed the next time the app starts (Velopack applies a downloaded package at startup and restarts the app with the same arguments, so a `--hidden` start stays hidden). `update::status()` tells where it is, with a Danish label for a settings page.

Builds with a version suffix (`0.5.1-dev.1`) also see pre-releases; release builds only see releases. Auto-update only ever moves to a higher version.

`TINGSKRIVER_UPDATE_FEED` overrides the feed for tests: `off`, a folder with release files, or an `http(s)` URL to such a folder.

## Releasing

1. Set `version` in `[workspace.package]` in the root `Cargo.toml` (0.5.x PATCH steps, see SPEC; a `-dev.N` suffix for test builds), commit and push.
2. Tag and push: `git tag app-v0.5.1-dev.1 && git push origin app-v0.5.1-dev.1`. The workflow fails if the tag and the workspace version differ.
3. `app-release.yml` builds the app with the same portable CPU baseline as the Tauri installer, downloads the previous Velopack release's full package so vpk can build a delta, packs, and publishes a GitHub Release with `Tingskriver-Setup.exe`, `TingskriverApp-<version>-full.nupkg`, `TingskriverApp-<version>-delta.nupkg` (from the second release on) and `releases.win.json`. Versions with a suffix become pre-releases. The release is never marked latest (see the move below).

Branch pushes that touch the installer files, and manual runs (`workflow_dispatch`), build the same files (delta against the newest published release) as the artifact `tingskriver-app-velopack-<version>-<sha>` without publishing.

Locally on Windows (the vpk version must equal the `velopack` crate in `crates/tingskriver-app/Cargo.toml`):

```powershell
dotnet tool install -g vpk --version 1.2.161
cargo build --release -p tingskriver-app
pwsh installer/pack.ps1 -Version 0.5.1-dev.1 -Exe target/release/tingskriver-app.exe
# installer/Releases/upload/ holds the release files
```

## Testing on a clean Windows VM

Updates without publishing a release: build two versions (bump the workspace version, push the branch, download both artifacts) and point the installed app at a folder with the newer one's files. Every build, branch builds included, makes its delta against the newest published Velopack release, so a delta update needs the older version to be that release (publish it as an `app-v…-dev.N` pre-release first); between two unpublished branch builds the update is a full package.

1. Run `Tingskriver-Setup.exe` of the older version as a normal user. No UAC prompt should appear. Check: `%LOCALAPPDATA%\TingskriverApp\current\tingskriver-app.exe` exists, Start menu and Desktop have "Tingskriver", `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` has `Tingskriver` with `--hidden`, and the log `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\tingskriver-app.log` has `installer install:` lines.
2. Sign out and in: the app runs hidden (no window, process in Task Manager); starting it from the Start menu shows the window.
3. Copy the newer version's `*-full.nupkg`, `*-delta.nupkg` and `releases.win.json` into `C:\feed` (with only the newer release's feed in the folder; the older full package is not needed). Set `setx TINGSKRIVER_UPDATE_FEED C:\feed`, start the app and wait a minute: the log shows `update: downloading <version> (delta)` and then `ReadyAtRestart`. Quit the app and start it again: it starts the new version (`Tingskriver <version> (GPUI) starting` in the log), and meetings and settings are still there. Remove the variable again with `reg delete HKCU\Environment /v TINGSKRIVER_UPDATE_FEED /f`.
4. Uninstall from Settings → Apps. `%LOCALAPPDATA%\TingskriverApp` and the Run value are gone; `%APPDATA%\dk.mahoje.tingskriver` and the `Tingskriver/…` entries in Credential Manager are still there.
5. Leftovers: on a machine with the old Tauri 0.5.0-dev.0 install updated to a later Tauri build (`%LOCALAPPDATA%\Tingskriver\meetily.exe` next to `tingskriver.exe`), installing removes `meetily.exe` and shortcuts that pointed to it; the log lists each removal.

The real GitHub feed is tested the same way with a published `app-v…-dev.N` pre-release instead of `C:\feed`.

## Move from Tauri (prepared, not done)

Existing users have the Tauri app from the NSIS installer in `%LOCALAPPDATA%\Tingskriver`, and its updater reads `https://github.com/mahope/tingskriver/releases/latest/download/latest.json`. That is why Velopack releases are never marked latest: until the move, the newest Tauri release must stay latest.

The move, when the GPUI app can do what the Tauri app does:

1. A last Tauri release (NSIS, through the Tauri updater as usual) whose app, at startup, downloads `Tingskriver-Setup.exe` from the newest Velopack release, runs it with `--silent`, waits for the new app's uninstall entry `TingskriverApp`, runs the NSIS uninstaller `%LOCALAPPDATA%\Tingskriver\uninstall.exe /S` and exits. The data folders are shared, so nothing has to be copied.
2. Alternative without new Tauri code: point `latest.json` at a signed `Tingskriver-Setup.exe`. Not verified: the Tauri updater starts the file with NSIS arguments (`/P /R`), which Velopack's Setup.exe may reject.
3. From then on Velopack releases may be marked latest (drop `--latest=false` in `app-release.yml`), and release.yml and the Tauri updater can go.

Until the GPUI app is ready, the Tauri app itself can be shipped through Velopack: `pack.ps1 -Exe …\tingskriver.exe -Extra <sidecars> -SkipVeloAppCheck`. Its exe does not handle Velopack's hooks, so the Tauri crate needs `velopack::VelopackApp::build().run()` first in `main` before such a package is released; without it, the install hook would start the app with a window.
