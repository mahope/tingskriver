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

## Verification log

- Initial `cargo test --locked -p tingskriver-core --test mcp_access`: fails with
  unresolved MCP imports (red).
- After the reader/migration: 3 tests pass. Existing core warnings: unused mut
  in recording_preferences, dead Windows Vulkan loader and built-in templates.
