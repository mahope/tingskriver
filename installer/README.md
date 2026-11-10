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
| before uninstall | Removes the Run value. Deletes the user's data and `Tingskriver/*` credentials only when the marker file `%APPDATA%\dk.mahoje.tingskriver\slet-data-ved-afinstallation` exists (`tingskriver_core::install::user_data::set_remove_on_uninstall`). Otherwise everything stays for a reinstall. |

Open: nothing in the app calls `set_remove_on_uninstall` yet, so the user cannot choose to have their data deleted without creating the marker file by hand. The acceptance criterion "uninstalling keeps user data and keys unless the user chooses otherwise" is only half met until the settings page offers the choice (follow-up issue).

Leftovers of earlier builds (`tingskriver_core::install::legacy`): before 0.5.0-dev.1 the Tauri app was built as `meetily.exe` in `%LOCALAPPDATA%\Tingskriver`, and the rename left the old exe next to `tingskriver.exe`. The hooks remove

- `meetily.exe` lying next to `tingskriver.exe` in `%LOCALAPPDATA%\Tingskriver` or `%LOCALAPPDATA%\Programs\Tingskriver`,
- shortcuts (`.lnk`) in the Start menu, its `Startup` folder, `Start Menu\Programs\Tingskriver`, `…\meetily` and on the Desktop that point to a `meetily.exe` that no longer exists,
- `HKCU\…\Run` values that start a `meetily.exe` that no longer exists,
- `HKCU\…\Uninstall` entries that mention meetily and whose uninstaller is gone.

A working upstream Meetily installation (exe and uninstaller still there) is left alone.

## Updates

The app checks GitHub Releases 60 s after start and then every 6 hours (`update::start_background_checks`, only in an installed copy, only in the primary instance). The feed is the `releases.win.json` asset of each of the ten newest releases in the repo, joined; Velopack's GitHub source asks for ten (`per_page=10`) and only then drops pre-releases for a release build. A few Tauri releases in between do no harm, but if the newest Velopack release is not among the ten newest releases of any kind (or, for a release build, the newest stable Velopack release is not among them), the app cannot see it: it logs a warning and `update::status()` says `NoReleaseFound`, not "up to date". So keep fewer than ten Tauri or pre-releases between two Velopack releases until the move below, after which only Velopack releases are published. A fixed feed (a `releases.win.json` at a stable URL) would remove the limit and is worth it if the two release tracks keep running side by side. A newer version is downloaded in the background, as a delta package when the release has one for the installed version, otherwise as the full package. It is installed the next time the app starts as the only instance (`update::apply_pending_update`, after the single-instance check: Velopack's updater waits for the process to exit, installs and restarts the app with the same arguments, so a `--hidden` start stays hidden). A second launch, such as opening the window from the Start menu while the app runs in the tray, only hands over to the running instance and never applies the update, so a recording is never cut off. `update::status()` tells where it is, with a Danish label for a settings page.

Builds with a version suffix (`0.5.1-dev.1`) also see pre-releases; release builds only see releases. Auto-update only ever moves to a higher version.

`TINGSKRIVER_UPDATE_FEED` overrides the feed for tests: `off`, a folder with release files, or an `http(s)` URL to such a folder.

## Releasing

1. Set `version` in `[workspace.package]` in the root `Cargo.toml` (0.5.x PATCH steps, see SPEC; a `-dev.N` suffix for test builds), commit and push.
2. Tag and push: `git tag app-v0.5.1-dev.1 && git push origin app-v0.5.1-dev.1`. The workflow fails if the tag and the workspace version differ.
3. `app-release.yml` builds the app with the same portable CPU baseline as the Tauri installer, downloads the previous Velopack release's full package so vpk can build a delta, packs, and publishes a GitHub Release with `Tingskriver-Setup.exe`, `TingskriverApp-<version>-full.nupkg`, `TingskriverApp-<version>-delta.nupkg` (from the second release on) and `releases.win.json`. Versions with a suffix become pre-releases. The release is never marked latest (see the move below).

Branch pushes that touch the installer files, and manual runs (`workflow_dispatch`) on branches, build a full package as the artifact `tingskriver-app-velopack-<version>-<sha>` without publishing or downloading previous releases. This also works when the workspace version has already been published. Only tag releases build deltas against the newest published release.

Locally on Windows (the vpk version must equal the `velopack` crate in `crates/tingskriver-app/Cargo.toml`):

```powershell
dotnet tool install -g vpk --version 1.2.161
cargo build --release -p tingskriver-app
pwsh installer/pack.ps1 -Version 0.5.1-dev.1 -Exe target/release/tingskriver-app.exe
# installer/Releases/upload/ holds the release files
```

## Testing on a clean Windows VM

Updates without publishing a release: build two versions (bump the workspace version, push the branch, download both artifacts) and point the installed app at a folder with the newer one's files. Branch builds produce full packages only. To test a delta update, publish both versions as `app-v…-dev.N` pre-releases instead.

1. Run `Tingskriver-Setup.exe` of the older version as a normal user. No UAC prompt should appear. Check: `%LOCALAPPDATA%\TingskriverApp\current\tingskriver-app.exe` exists, Start menu and Desktop have "Tingskriver", `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` has `Tingskriver` with `--hidden`, and the log `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\tingskriver-app.log` has `installer install:` lines.
2. Sign out and in: the app runs hidden (no window, process in Task Manager); starting it from the Start menu shows the window.
3. Copy the newer version's `*-full.nupkg`, `*-delta.nupkg` and `releases.win.json` into `C:\feed` (with only the newer release's feed in the folder; the older full package is not needed). Set `setx TINGSKRIVER_UPDATE_FEED C:\feed`, start the app and wait a minute: the log shows `update: downloading <version> (delta)` and then `ReadyAtRestart`. Quit the app and start it again: it starts the new version (`Tingskriver <version> (GPUI) starting` in the log), and meetings and settings are still there. Remove the variable again with `reg delete HKCU\Environment /v TINGSKRIVER_UPDATE_FEED /f`.
4. Uninstall from Settings → Apps. `%LOCALAPPDATA%\TingskriverApp` and the Run value are gone; `%APPDATA%\dk.mahoje.tingskriver` and the `Tingskriver/…` entries in Credential Manager are still there.
5. Leftovers: on a machine with the old Tauri 0.5.0-dev.0 install updated to a later Tauri build (`%LOCALAPPDATA%\Tingskriver\meetily.exe` next to `tingskriver.exe`), installing removes `meetily.exe` and shortcuts that pointed to it; the log lists each removal.

The real GitHub feed is tested the same way with a published `app-v…-dev.N` pre-release instead of `C:\feed`.

## Move from Tauri

Existing users have the Tauri app from the NSIS installer in `%LOCALAPPDATA%\Tingskriver`, and its updater reads `https://github.com/mahope/tingskriver/releases/latest/download/latest.json`. That is why Velopack releases are never marked latest: until the move, the newest Tauri release must stay latest.

1. **Done in 0.5.0-dev.3**, the last Tauri release (NSIS, through the Tauri updater as usual; `frontend/src-tauri/src/move_to_gpui.rs`). Only in Windows release builds running from `%LOCALAPPDATA%\Tingskriver`, and not when `TINGSKRIVER_NO_MIGRATE` is set. 30 s after start, in the background and only while nothing is recorded, encoded, retranscribed or imported, the app:
   - picks the highest `app-v*` version (pre-releases included, drafts not) among `GET /repos/mahope/tingskriver/releases?per_page=30` and downloads its `Tingskriver-Setup.exe` to `%TEMP%\tingskriver-move`, checking that the file is not empty and has the size GitHub reports;
   - runs it with `--silent` (Velopack then does not start the app) and waits, at most 5 minutes, for the uninstall entry `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\TingskriverApp`. If the GPUI app is already installed, this step is skipped;
   - waits for a recording (and its encoding) to finish, blocks new recordings until it exits (under the same lock that starting a recording holds), starts a copy of its own exe, `%TEMP%\tingskriver-move\tingskriver-move.exe --finish-move-to-gpui <pid>`, and exits.

   The copy is needed because the NSIS uninstaller kills every running `tingskriver.exe` of the user. Once the Tauri app has exited, the helper runs `uninstall.exe /S _?=%LOCALAPPDATA%\Tingskriver` (in place, so it can be waited for) and removes the leftover `uninstall.exe` and the empty folder. If `Tingskriver.lnk` is then missing in the Start menu or on the Desktop, it runs `Tingskriver-Setup.exe --silent` once more (a failed run is logged; the GPUI app itself is installed by then). The NSIS uninstaller also deletes `HKCU\…\Run\Tingskriver`, which the GPUI installer had just pointed at the new app, so the helper writes it again. Last it starts `%LOCALAPPDATA%\TingskriverApp\current\tingskriver-app.exe` (the Tauri app, if that fails and it is still there) and has `cmd` delete `%TEMP%\tingskriver-move` after it exits. Everything is logged in `tingskriver.log` with the prefix `move to GPUI:`.

   The data folders are shared, so nothing is copied. On any failure before the hand-off the Tauri app keeps running and tries again at its next start. The Velopack installer must exit with success, not only write its uninstall entry. If the uninstall fails and `tingskriver.exe` is still there, the helper starts the Tauri app again with `TINGSKRIVER_NO_MIGRATE` set for that run (so a failing uninstall does not repeat every minute); its next start tries again. The GPUI installer has by then pointed the shortcuts and `Run\Tingskriver` at the GPUI app, so that next Tauri start may not come by itself: the log then shows the error, and both apps are installed until it does. The tauri-bundler version in use (`@tauri-apps/cli` 2.11.1) only deletes shortcuts that point to `tingskriver.exe`, so the reinstall should not be needed; it is there in case Velopack's shortcuts were not written. Not verified on a VM yet: run the steps in "Testing the move" below before the release is published.
2. Alternative without new Tauri code (not used): point `latest.json` at a signed `Tingskriver-Setup.exe`. Not verified: the Tauri updater starts the file with NSIS arguments (`/P /R`), which Velopack's Setup.exe may reject.
3. Follow-up, once the Tauri users have moved: Velopack releases may be marked latest (drop `--latest=false` in `app-release.yml`), and `release.yml` and the Tauri updater can go.

### Testing the move

On a clean Windows VM: install the Tauri 0.5.0-dev.2 NSIS installer, start it, then let the Tauri updater install 0.5.0-dev.3 (or install the 0.5.0-dev.3 NSIS installer over it). Within a minute of the start the log `%LOCALAPPDATA%\dk.mahoje.tingskriver\logs\tingskriver.log` shows `move to GPUI: starting`, the download, `GPUI app installed`, `helper running`, `Tauri app uninstalled` and `started …tingskriver-app.exe`. Check: the GPUI app runs, `%LOCALAPPDATA%\Tingskriver` is gone, Settings → Apps lists only the GPUI app, Start menu and Desktop have one "Tingskriver" that starts the GPUI app, `HKCU\…\Run\Tingskriver` points at `tingskriver-app.exe --hidden`, and meetings and settings are still there. Repeat once with a recording running at start: nothing happens until it is stopped and encoded. With `setx TINGSKRIVER_NO_MIGRATE 1` nothing happens at all.

Until the GPUI app is ready, the Tauri app itself can be shipped through Velopack: `pack.ps1 -Exe …\tingskriver.exe -Extra <sidecars> -SkipVeloAppCheck`. Its exe does not handle Velopack's hooks, so the Tauri crate needs `velopack::VelopackApp::build().run()` first in `main` before such a package is released; without it, the install hook would start the app with a window.
