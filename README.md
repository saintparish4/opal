# Opal

An all-in-one JavaScript/TypeScript toolkit — package manager, runtime, bundler, and test runner in a single native binary, built around one shared incremental module graph engine (`opal-core`) instead of four independently-implemented resolvers.

> **Status**: early skeleton. The workspace split, `opal-core`, and every subsystem described below are still being built out — see [Architecture](#architecture) for the target shape and `base/build_guide.md` for the phased build order.

## Requirements

- **Language/runtime**: Rust, `stable` channel, pinned via [`rust-toolchain.toml`](./rust-toolchain.toml) (installs the `rustfmt` and `clippy` components automatically via `rustup`).
- **Package manager**: Cargo (ships with the Rust toolchain above).
- **C/C++ toolchain**: required once V8 embedding lands (`opal-runtime`) — `build-essential` on Linux, Xcode Command Line Tools on macOS.
- **`cmake` and `ninja`**: recommended for V8-related builds.
- **Database**: none — state is a local content-addressed store (CAS) on disk, not a database.
- **Other services**: none currently.
- **OS-level dependencies**: `git`.
- **Supported dev platforms**: macOS, Linux, or WSL2 on Windows (native Windows is a v2 target — see [Deployment](#deployment)).

## Installation

```bash
git clone <repo-url>
cd opal

# build the current skeleton
cargo build
```

> For end users (once releases exist), the intended install path is a curl-based installer script, **not** `cargo install` — see [Deployment](#deployment). `cargo install opal` is reserved for contributors building from source.

## Development

```bash
cargo build
cargo run
```

- Format with `cargo fmt`.
- Lint with `cargo clippy -- -D warnings` — Clippy warnings are treated as errors; do not hand-format against rustfmt defaults or leave warnings unaddressed.
- Run both before considering a change complete.

### Repository layout

Currently a flat single-binary crate (`src/main.rs`). Per `base/build_guide.md`, this is expected to become a Cargo workspace as each subsystem comes online:

```
opal/
├── crates/
│   ├── opal-core/       # shared module graph engine, CAS, BLAKE3 hashing
│   ├── opal-pm/         # package manager (install, resolve, lockfile)
│   ├── opal-runtime/    # V8 embedding, module execution
│   ├── opal-bundler/    # bundling, tree-shaking, minification
│   ├── opal-test/       # test runner
│   └── opal-cli/        # unified CLI entrypoint, dispatches to the above
├── docs/
└── Cargo.toml           # workspace root
```

Naming conventions and package-layout rules are defined in [`CLAUDE.md`](./CLAUDE.md).

### Build order

Phases are sequential — each is a prerequisite for the next, and each has its own exit criteria (full detail in `base/build_guide.md` §3):

| Phase | Crate | Exit criteria |
|---|---|---|
| 0 | `opal-core` | Resolve a real-world project's dependency graph, cache the result, demonstrate cache hits on unchanged input |
| 1 | `opal-pm` | `opal install` against a real `package.json` produces a working `node_modules` that Node can run against |
| 2 | `opal-runtime` | `opal run` executes a real project's entrypoint, including `node_modules` dependencies from Phase 1 |
| 3 | `opal-bundler` | Tree-shaking + minification over the resolved graph, outputs cached in the CAS |
| 4 | `opal-test` | Test discovery via the graph, wired into the Phase 2 execution path |

## Testing

```bash
cargo test
```

- **Unit tests**: standard Rust `#[test]`, per crate.
- **Property-based tests**: planned for the module graph engine (via `proptest`) — resolution and caching are where edge cases hide.
- **Compatibility test suite**: a curated set of real-world npm packages, installed and run end-to-end, checked into CI — the main defense against npm-compatibility regressions.
- **Benchmark suite**: cold install, cold start, and bundle time, tracked against the npm/Node/esbuild baseline (see `base/prd.md` §7–8 for target metrics).

CI (GitHub Actions + `cargo`) matrix-builds macOS (x64/arm64), Linux (x64/arm64), and Windows (x64) on every tagged release.

## Environment Variables

None currently defined. This section will be filled in as `opal-pm`'s registry client, cache paths, and any config overrides are implemented.

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| — | — | — | — |

## Architecture

Every JS toolchain today (npm/pnpm/yarn + Node/Bun/Deno + webpack/esbuild/vite + Jest/Vitest) re-solves "given this file, what does it import, and how do I resolve that" separately, in separate languages, with separate caches. Opal solves it once, natively, in `opal-core`, and every other subsystem consumes it.

```
                         ┌─────────────────────────┐
                         │        opal-core         │
                         │  module graph · CAS ·    │
                         │  BLAKE3 content hashing   │
                         └────────────┬─────────────┘
                 ┌────────────┬───────┴───────┬─────────────┐
                 ▼            ▼               ▼             ▼
            opal-pm     opal-runtime     opal-bundler    opal-test
          (install)      (V8 exec)      (tree-shake +   (discovery +
                                          minify)         assertions)
```

- **Core stack**: Rust for all native code; V8 (via the `v8` crate) as the JS engine; BLAKE3 for all content hashing (SIMD-accelerated, on the hot path for every file read).
- **`opal-core`**: parses/resolves import graphs, content-addresses every file and computed artifact, maintains an on-disk CAS, and answers "what changed since last run" via hash comparison — never mtime (unreliable across git checkouts, CI runners, and Docker layers).
- **`opal-pm`** (`opal install`): resolves against the public npm registry, populates the CAS keyed by tarball content hash (enabling cross-package dedup), links packages via hardlinks from the CAS (pnpm-style), and writes a flat `opal.lock` lockfile. Install pipeline: `package.json → registry metadata → semver resolution → dependency graph → opal.lock → download → BLAKE3 integrity verification → CAS → node_modules`.
- **`opal-runtime`** (`opal run`): executes JS/TS directly via embedded V8, using `opal-core`'s resolved graph for imports; lazy module instantiation for fast cold start; TypeScript via strip-types transpilation (no type-checking, matching the Bun/Deno model).
- **`opal-bundler`** (`opal build`): consumes the same graph, adds tree-shaking and minification, caches outputs in the CAS keyed by input hash.
- **`opal-test`** (`opal test`): thinnest layer — reuses the runtime's module loading/execution, adds test discovery, assertions, and a reporter.

See `base/prd.md` for full command reference (`opal add`, `remove`, `update`, `duplicate`, `snip`, `opalx`, `publish`, `outdated`, `why`, `audit`, `info`) and key risks (npm compatibility surface, native addon/node-gyp support, V8 embedding complexity).

## Deployment

Opal ships as a single self-contained native binary — no runtime dependency on a separate install step or interpreter.

**v0.1.0 release targets**: `opal-linux-x64`, `opal-linux-arm64`, `opal-macos-x64`, `opal-macos-arm64`, `opal-windows-x64.exe`.

**Release process** (`base/build_guide.md` §6):
1. Tag a release (e.g. `v0.1.0`).
2. CI (GitHub Actions) builds all five platform targets in release mode.
3. Each binary is packaged as a platform-appropriate archive (`.tar.gz` for Unix targets, `.zip` for Windows).
4. A `SHA256SUMS` file is generated across all artifacts.
5. Artifacts publish to GitHub Releases — the single canonical source of truth. Any future downstream package manager (Homebrew, Scoop, WinGet, npm wrapper, Docker) must fetch from here, never build independently.
6. The install script and installer endpoint (`opal.dev`, domain TBD) are updated to point at the new release.

**End-user install** is a curl-based script that detects OS/arch and places the binary at `~/.opal/bin/opal`, added to `PATH`:

```bash
curl -fsSL https://opal.dev/install.sh | bash
```

**Hard constraint**: `cargo install opal` must never be presented as the primary install path for end users (requires a Rust toolchain) — it's contributor-only, for building Opal itself from source.

Platform support: macOS, Linux, and WSL2 in v1 (WSL2 runs a genuine Linux kernel, so the Linux build target covers it directly). Native Windows (non-WSL) is v2, requiring junction-based fallbacks for linking and path-separator abstraction throughout `opal-core`.

## Contributing

- Follow the naming and package-layout conventions in [`CLAUDE.md`](./CLAUDE.md).
- Run `cargo fmt` and `cargo clippy -- -D warnings` before opening a PR — CI treats Clippy warnings as errors.
- Open PRs against `master`.
- New subsystem work should follow the phased build order above — don't start a later phase's crate before the prior phase's exit criteria are met.

## License

[MIT](./LICENSE) © Sharif Parish / Bluesky Labs
