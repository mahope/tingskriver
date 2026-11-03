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
with `VisualTestAppContext`, off-screen native windows and Direct3D texture
readback (`Window::render_to_image`), and uploads its PNGs. On runners without
a hardware adapter GPUI chooses Microsoft Basic Render Driver (WARP).
No desktop screenshot permissions or screen scraping are involved.

**The golden gate is Linux-only.** Windows is a capture smoke test, including
an assertion that light/dark differ. DirectWrite and FreeType have different
glyph rasterization, so Windows images are deliberately not compared to Linux
references. `--update` and normal comparison also support a separate
`tests/golden/windows/` inventory if Windows-specific references are added.
The native Windows job must pass before this change is considered verified
on Windows; a Linux run alone does not establish that WARP works.

The harness fixes the logical viewport to 1100×760 (headless GPUI's 2× scale
produces 2200×1520 PNGs), appearance, frame count and simulated clock. It does
not start the normal app: no database, credentials, tray, audio, network
providers or single-instance socket. Fixtures inject empty/first-launch and
error database states, Danish input text and default provider configuration
in memory. The meetings/settings/dictation pages still contain the shell's
current placeholders; new populated views need corresponding fixtures when
they are implemented. This does not test interaction, keyboard input or UIA.

## Run and update

Install the Linux system libraries listed in the `gpui-screenshots` CI job,
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

The Rust workflow runs on branch pushes (including the head commit of a PR).
Its `GPUI golden images (headless Linux)` and `Windows workspace build and
tests` jobs upload `gpui-screenshots-linux` and `gpui-screenshots-windows` for
30 days, including when comparison fails. Linux contains actual PNGs,
expected/difference PNGs for mismatches, a result text file and color mutation
evidence. Windows contains actual PNGs and an explicit capture-only result.

With GitHub authentication available:

```sh
gh run list --repo mahope/tingskriver --branch <PR-branch> --workflow rust.yml
gh run download <run-id> --repo mahope/tingskriver --name gpui-screenshots-linux --dir /tmp/gpui-linux
gh run download <run-id> --repo mahope/tingskriver --name gpui-screenshots-windows --dir /tmp/gpui-windows
```

Alternatively open the PR's Rust check, then the run's Artifacts section.
Artifact download requires an authenticated GitHub account; the repo deploy
SSH key alone cannot download Actions artifacts. Use `view_image` on a PNG
after downloading it. `TINGSKRIVER_SCREENSHOT_OUTPUT` overrides the output
directory for isolated verification runs; it does not change reference paths.
