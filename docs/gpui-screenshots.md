# GPUI screenshot tests

Issue #49 uses GPUI's real scene rasterization, not Playwright or a second
implementation of the UI. `Page::ALL` supplies the navigation and the fixture
inventory. Every current page (meetings, dictation, measurement, settings),
plus loading and database failure, has a committed PNG in light and dark.

## Rendering and CI

The canonical comparison runs headlessly on Ubuntu 24.04 with Mesa Vulkan
software rendering. `HeadlessAppContext` supplies deterministic scheduling,
the platform's real text system, bundled fonts/icons and GPUI's WGPU renderer.
It needs neither Xvfb nor a logged-in desktop. Windows runs the same views
with the normal GPUI application event loop, off-screen native windows and Direct3D texture
readback (`Window::render_to_image`), and uploads its PNGs. On runners without
a hardware adapter GPUI chooses Microsoft Basic Render Driver (WARP).
No desktop screenshot permissions or screen scraping are involved.

**The golden gate is Linux-only.** Windows is a capture smoke test, including
an assertion that light/dark differ. DirectWrite and FreeType have different
glyph rasterization, so Windows images are deliberately not compared to Linux
references. Windows currently supports `--capture` only.
The native Windows job must pass before this change is considered verified
on Windows; a Linux run alone does not establish that WARP works. In pinned
GPUI 0.3.10, `VisualTestAppContext` is exported only on macOS, despite its
generic implementation. The native capture adapter is also compiled on Linux
to catch use of unavailable GPUI APIs before the Windows CI job.
Both adapters draw through the same untyped window context. Drawing inside
`WindowHandle<Root>::update` recursively borrows Root and panics; the Linux
golden run exercises this shared path as a regression check.
Windows compilation is a separate CI step. The native executable exits with
failure after 180 seconds if rendering or shutdown blocks; `run.log` records
the last opening/drawing/readback stage and is uploaded even without PNGs.
`scripts/test-gpui-watchdog.py` proves the real watchdog exits a deliberately
parked child process with a shortened timeout, without starting GPUI.
Native capture yields to the event loop after opening/resizing each off-screen
window. Windows queues these operations; capturing during the launch callback
otherwise produces 1×1 PNGs. Both adapters require full viewport dimensions
matching the window's logical size and scale, and reject uninitialized targets.
The native desktop may clamp the requested dimensions; capture must match the
actual viewport and exceed the app's minimum window size at that DPI. Linux's
headless viewport remains fixed for golden comparison.
The native snapshot executable exits explicitly after every image, viewport
and appearance assertion and the result file have been written. Any capture
error exits with failure. GPUI 0.3.10's event loop did not return from quit on
the hosted runner, so this disposable process does not exercise graceful app
shutdown. The production quit path remains a separate manual test.

The Linux harness fixes the logical viewport to 1100×760 (headless GPUI's 2× scale
produces 2200×1520 PNGs), appearance, frame count and simulated clock. Reduced
motion pins animations to a static frame because GPUI animations use wall-clock
time independently of the test scheduler. The loading fixture must remain
pixel-identical after a delayed redraw. It does
not start production services: no database, credentials, tray, audio, network
providers or single-instance socket. Fixtures inject empty/first-launch and
error database states, Danish input text and default provider configuration
in memory. Windows shares the fixtures and appearance but uses real event-loop
time for its capture smoke test. The meetings/settings/dictation pages still contain the shell's
current placeholders; new populated views need corresponding fixtures when
they are implemented. This does not test interaction, keyboard input or UIA.
The fixture version is `0.0.0-test`; ordinary release version bumps therefore
do not invalidate every image, while the production app shows its real version.

## Run and update

Install the Linux system libraries listed in the screenshot workflow's Linux job,
then run from the repository root:

```sh
cargo test --locked -p tingskriver-app --features screenshot-tests --test screenshots
python3 scripts/test-gpui-color-gate.py
```

Comparison allows a difference of at most two levels per RGBA channel and
at most 0.01% of pixels beyond that. Different dimensions or missing images
fail. The color-gate script temporarily changes the actual light background
token to green, requires a pixel-comparison failure and a difference PNG,
and restores the exact source bytes in `finally`. Its artifacts are kept
separately under `target/screenshots/color-mutation/`.

For an intentional visual change, on the same Linux/Mesa environment as CI:

```sh
cargo test --locked -p tingskriver-app --features screenshot-tests --test screenshots -- --update
cargo test --locked -p tingskriver-app --features screenshot-tests --test screenshots
git diff --stat -- crates/tingskriver-app/tests/golden
```

Inspect the changed PNGs and commit them with the UI change. CI never updates
references automatically. A new page automatically requires two new goldens;
do not use `--update` to conceal an unexplained failure. On Windows, use
`-- --capture` to render without modifying references.

## Retrieve PR artifacts

The `GPUI screenshots` workflow runs on branch pushes and PRs, independently
of the full Rust build so UI feedback does not wait for unrelated tests.
Its Linux and Windows D3D jobs upload `gpui-screenshots-linux` and `gpui-screenshots-windows` for
30 days, including when comparison fails. Linux contains actual PNGs,
expected/difference PNGs for mismatches, a result text file and color mutation
evidence. Windows contains actual PNGs and an explicit capture-only result.

With GitHub authentication available:

```sh
gh run list --repo mahope/tingskriver --branch <PR-branch> --workflow gpui-screenshots.yml
gh run download <run-id> --repo mahope/tingskriver --name gpui-screenshots-linux --dir /tmp/gpui-linux
gh run download <run-id> --repo mahope/tingskriver --name gpui-screenshots-windows --dir /tmp/gpui-windows
```

Alternatively open the PR's GPUI screenshots check, then the run's Artifacts section.
Agents with the GitHub connector can call `fetch_workflow_run_artifacts` for
the run, then `download_workflow_artifact` with `repo_full_name` and the
returned artifact ID. This path was exercised against the branch's Linux
artifact, including its actual/expected/difference PNGs and result files.
Artifact download requires GitHub authentication (CLI or connector); the repo
deploy SSH key alone cannot download Actions artifacts. Use `view_image` on a PNG
after downloading it. `TINGSKRIVER_SCREENSHOT_OUTPUT` overrides the output
directory for isolated verification runs; it does not change reference paths.

## Dependency review

`image 0.25.10` was already locked through GPUI; the harness uses its PNG
codec directly without another image version. Enabling the pinned GPUI
`test-support` features adds seven packages to the lockfile:

| Package | Maintainer/repository | Selected release date |
| --- | --- | --- |
| atomic 0.5.3 | Amanieu, atomic-rs | 2023-05-15 |
| convert_case 0.6.0 | rutrum, convert-case | 2022-09-08 |
| fixedbitset 0.5.7 | bluss/petgraph | 2024-04-14 |
| futures-concurrency 7.7.1 | Yoshua Wuyts | 2026-01-18 |
| pollster 0.2.5 | zesterer | 2022-01-18 |
| postage 0.5.0 | Austin Jones | 2022-04-14 |
| proptest-macro 0.4.0 | proptest-rs | 2025-10-26 |

Reviewed Cargo manifests, normal dependency trees and Rust sources before
compilation. These are established repositories, with selected versions older
than three days; no suspicious new-release burst was found. The seven packages
have no build/install scripts or environment/network access at library load.
The network example in futures-concurrency is not built. proptest-macro is a
compiler procedural macro. Their other dependencies were already locked.
Conclusion: **install** through Cargo's existing checksum-verified registry
mechanism; no JavaScript or Python package installation is needed.
