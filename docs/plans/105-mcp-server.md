# Issue 105: local MCP server

Base: `003a3eb39c6eeffa75595045613bef5e29bb03f3` (PR #166, delivery).

## Implementation checkpoints

1. Read-only meeting reader with fixture-tested access policy and opt-in migration.
2. rmcp stdio tools and bounded local application IPC, including delivery.
3. Settings, per-meeting sharing, installer, bilingual setup documentation.
4. Focused tests throughout; full checks once at completion. Push checkpoint commits;
   no PR, deploy, or change to the owner's actual installation.

## Dependency assessment

`rmcp = 3.8.0`, the official `modelcontextprotocol/rust-sdk`, published
2026-10-26. Previous releases: 3.7.0 on October 22, 3.6.0 on October 20.
The project has published Rust SDK releases since 2025. Only `server` and
`transport-io` are enabled; HTTP transports and authentication are disabled.
Requires Rust 1.88 (the agent uses 1.97.1).

The lockfile adds two registry packages: rmcp and pastey 0.2.3. Existing locked
dependencies are reused. pastey is maintained by Aditya Kumar, derived from
David Tolnay's paste; release history extends to 2025. Version 0.2.3 was
published May 20, 2026, after 0.2.2 on April 22. It has no build script or
runtime network code. Its explicitly requested `env!` token expansion can
read environment variables at compile time.

rmcp's build.rs reads CARGO_MANIFEST_DIR and can configure git hooks only when
the package's grandparent contains both `.git` and `.githooks`. In Cargo's
registry layout this is the registry source directory, not our repository;
both directories are absent there. The stdio transport performs local I/O,
including nonblocking file descriptor setup on Unix. The selected transport
does not open a network port. No application credentials are needed to load it.

Sources inspected before compilation: published Cargo manifests, rmcp build.rs,
lib.rs and transport/io.rs, pastey source and crates.io sparse index publication
times. crates.io's API returned HTTP 403; sparse index, static package archives
and the official GitHub release API were accessible.

Conclusion: **installér**. Neither package is younger than three days; no
install-script bypass or minimum-release-age override is used.

Optional React Doctor check: registry metadata on 2026-11-04 reports
react-doctor 0.9.22, published 2026-11-02 (under three days). Maintainer:
millionco/react-doctor, releases since February 2026. It declares 19 direct
dependencies including native parsers, playwright-core and @sentry/node.
No install lifecycle script is declared. Runtime behavior and the full new
transitive graph were not audited because the publication-age stop rule already
applies. Conclusion: **lad være**. It was not installed or executed; existing
TypeScript/build/tests are used instead.

## Verification log

- Initial `cargo test --locked -p tingskriver-core --test mcp_access`: fails with
  unresolved MCP imports (red).
- After the reader/migration: 3 tests pass. Existing core warnings: unused mut
  in recording_preferences, dead Windows Vulkan loader and built-in templates.
- Stdio test initially failed at initialization (EOF from the placeholder main).
  After rmcp implementation it initializes and lists ten tools, lists/gets a
  fixture, applies a live metadata policy change and reports the closed app.
- Six access fixtures and two app-action fixtures pass. Plain task-list fixture
  initially returned zero tasks, then passed after recognizing task headings.
- Linux `netstat -anp` during the live stdio test finds no TCP/UDP socket owned
  by the MCP subprocess. Windows netstat and actual Claude/Codex use remain
  release verification, not claimed by the Linux test.
- Frontend tests initially failed on the missing component; after implementation
  two tests pass (explicit access save, exact setup copy, failed sharing write).
  `pnpm exec tsc --noEmit` passes.
- Both app hosts pass Linux `cargo check`; Tauri was checked with real local
  MCP/STT sidecars, system FFmpeg and the same llama-helper placeholder used
  for type-checking in CI. No placeholder is used for an installer.
- Production frontend build passes. The combined `bun test` has the same
  pre-existing failures as an archived origin/main (48 pass/3 fail on base,
  50 pass/3 fail with MCP). Existing tests install incompatible global window
  mocks. Per-file processes pass all 55 Bun tests; the assertion-based onboarding
  .mjs file also passes under Node. No assertions were removed or skipped.
- `pnpm lint` and the identical command on origin/main both exit 1 at Next's
  interactive ESLint setup prompt. No new lint dependency/config was added.
- Eight before/after screenshots of the real web controls with synthetic data
  were produced at 375 and 1440 px, light/dark, using sandboxed Chrome Stable.
  Outputs: `/tmp/tingskriver-105-screenshots/`. These inspect the components,
  not a live native app or native GPUI rendering.
- CI on checkpoint f03af4b passed Windows xwin and cargo-deny. Local tools are
  absent, so no additional tool packages were installed. CI exposed an FTS
  migration snapshot comparing later additive columns; the test now scopes
  migration execution to FTS itself, preserving the full data equality check.
