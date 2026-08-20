# Opal

An all-in-one JavaScript/TypeScript toolkit — package manager, runtime, bundler, and test runner in a single native binary, built around one shared incremental module graph engine (`opal-core`) instead of four independently-implemented resolvers.

> **Status**: Phase 0 (`opal-core`) is complete and meets its exit criteria — Opal resolves a real npm-installed dependency tree it did not create, content-addresses the result, and serves cache hits on unchanged input. `opal-pm`, `opal-runtime`, `opal-bundler`, and `opal-test` are **not implemented** and are not yet workspace members; the directories under `crates/` for those phases hold empty placeholder files only. See [Architecture](#architecture) for the target shape and [Build order](#build-order) for sequencing.

## Requirements

- **Language/runtime**: Rust, `stable` channel, pinned via [`rust-toolchain.toml`](./rust-toolchain.toml) (installs the `rustfmt` and `clippy` components automatically via `rustup`).
- **Package manager**: Cargo (ships with the Rust toolchain above).
- **C/C++ toolchain**: required once V8 embedding lands (`opal-runtime`, Phase 2) — `build-essential` on Linux, Xcode Command Line Tools on macOS. Not needed to build the current workspace.
- **`cmake` and `ninja`**: recommended for V8-related builds (Phase 2 onward).
- **Database**: none — state is a local content-addressed store (CAS) on disk, not a database.
- **Other services**: none currently.
- **OS-level dependencies**: `git`.
- **Supported dev platforms**: macOS, Linux, or WSL2 on Windows (native Windows is a v2 target — see [Deployment](#deployment)).

## Installation

```bash
git clone <repo-url>
cd opal

cargo build --release   # binary at ./target/release/opal
```

> For end users (once releases exist), the intended install path is a curl-based installer script, **not** `cargo install` — see [Deployment](#deployment). `cargo install opal` is reserved for contributors building from source.

## Usage

The binary ships only what is implemented. `install`, `run`, `build`, and `test` are absent by design — a command that exists and does nothing is worse than one that does not exist.

### `opal graph` — resolve a module graph

```bash
opal graph <ENTRY> [--root <ROOT>] [--cache-dir <CACHE_DIR>] [--json]
```

| Flag | Description |
|---|---|
| `<ENTRY>` | Entry file to walk from (required) |
| `--root` | Project root; module paths are reported relative to it. Defaults to the entry's parent directory |
| `--cache-dir` | Cache location. Defaults to the discovered user cache directory |
| `--json` | Print the resolved graph as JSON instead of the summary |

Walking a tree installed by npm — the compatibility check that matters is that Opal resolves a `node_modules` it did not create:

```console
$ opal graph index.js --root . --cache-dir /tmp/opal-cache
141 modules, 273 edges in 30.1ms
cache:  MISS (no record)
digest: f5282b03c8203a839cd2a9d851d9147e479f1a9d9983d0c0beff7a10938606e9
graph:  be15ccc48bd0b870626db386f096c26116a77afa687292d4afda1aaa28198178

$ opal graph index.js --root . --cache-dir /tmp/opal-cache
141 modules, 273 edges in 7.5ms
cache:  HIT
digest: f5282b03c8203a839cd2a9d851d9147e479f1a9d9983d0c0beff7a10938606e9
graph:  be15ccc48bd0b870626db386f096c26116a77afa687292d4afda1aaa28198178
```

The digest is identical across both runs; only the cache status differs. Touching a file's mtime still reports `HIT` — invalidation is content-hash only, never mtime. Changing a byte reports `MISS (changed: <path>)`, naming the file that moved.

A specifier that cannot be resolved is reported as a diagnostic, never a fatal error — real projects import optional dependencies, platform-specific natives, and packages that are not installed:

```console
unresolved specifiers: 1
  node_modules/debug/src/node.js: cannot resolve "supports-color": package "supports-color" is not installed under the project root
```

### `opal cache` — inspect the shared CAS

```bash
opal cache verify [--cache-dir <DIR>]          # re-hash every object, check it against its key
opal cache gc [--cache-dir <DIR>] [--dry-run]  # remove objects no memo record points at, plus stale temp files
opal cache path [--cache-dir <DIR>]            # print the cache location
```

`verify` exits non-zero if any object's content does not match its hash key. `gc --dry-run` reports what would be removed without removing it.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

- Format with `cargo fmt`.
- Lint with `cargo clippy -- -D warnings` — Clippy warnings are treated as errors; do not hand-format against rustfmt defaults or leave warnings unaddressed.
- Run both before considering a change complete.

### Repository layout

A Cargo workspace. Only the crates below marked *implemented* are workspace members — the rest are added as their phase begins, so the workspace never carries a crate whose API has not been designed yet.

```
opal/
├── crates/
│   ├── opal-core/       # implemented — module graph, resolver, CAS, BLAKE3 hashing, memoization
│   ├── opal-cli/        # implemented — `opal` binary; dispatches `graph` and `cache`
│   ├── opal-pm/         # Phase 1, placeholder files only, not a workspace member
│   ├── opal-runtime/    # Phase 2, placeholder files only, not a workspace member
│   ├── opal-bundler/    # Phase 3, placeholder files only, not a workspace member
│   └── opal-test/       # Phase 4, placeholder files only, not a workspace member
└── Cargo.toml           # workspace root
```

Inside `opal-core`:

| Module | Responsibility |
|---|---|
| `hash` | BLAKE3 content hashing |
| `path` | Path abstraction layer — all path handling routes through here, so v2 Windows support is a cheap addition |
| `atomic` | Atomic write primitives (temp file → verify → rename) |
| `cas` | Content-addressed store: on-disk layout, write/read by hash, `cas::gc` for collection |
| `graph` | Module graph, `graph::resolver` (ESM/CJS parsing via `oxc`), `graph::memo` (memoization keyed by input hash) |
| `cache` | Cache root discovery and the combined CAS + memo handle |
| `fault` | Fault injection used by the crash-safety suite |

Naming follows the standard Rust conventions — `UpperCamelCase` for types and
traits, `snake_case` for everything value-level, `SCREAMING_SNAKE_CASE` for
constants and statics, acronyms counted as one word (`Uuid`, not `UUID`).
Layout follows Cargo's defaults: crate source in `src/`, extra binaries in
`src/bin/`, integration tests in `tests/`, benches in `benches/`, examples in
`examples/`. Binary, test, bench, and example *target* names are kebab-case;
modules inside them are snake_case.

### Build order

Phases are sequential — each is a prerequisite for the next, and each has its own exit criteria.

| Phase | Crate | Status | Exit criteria |
|---|---|---|---|
| 0 | `opal-core` | **complete** | Resolve a real-world project's dependency graph, cache the result, demonstrate cache hits on unchanged input; SIGKILL mid-CAS-write never leaves a corrupt entry |
| 1 | `opal-pm` | not started | `opal install` against a real `package.json` produces a working `node_modules` that Node can run against; SIGKILL at randomized pipeline points always converges on re-run |
| 2 | `opal-runtime` | not started | `opal run` executes a real project's entrypoint, including `node_modules` dependencies from Phase 1 |
| 3 | `opal-bundler` | not started | Tree-shaking + minification over the resolved graph, outputs cached in the CAS |
| 4 | `opal-test` | not started | Test discovery via the graph, wired into the Phase 2 execution path |

## Testing

```bash
cargo test --workspace
```

82 tests currently pass, organized by **risk category** rather than a unit/integration/e2e pyramid — the question is where the system actually breaks, and what a bug looks like when it does:

| Suite | Count | Covers |
|---|---|---|
| `opal-core` unit | 51 | Hashing, path abstraction, CAS layout, graph construction, resolver internals |
| `tests/cache-invalidation.rs` | 13 | The invalidation matrix: content change, add/remove, direct and transitive dependency change — asserting the right hits *and* misses. Includes the "never mtime" invariant as a direct test |
| `tests/graph-resolution.rs` | 12 | Resolution against fixture trees, plus a golden/snapshot test of resolved graph output (`tests/golden/`) |
| `tests/cas-crash-safety.rs` | 6 | Atomic CAS writes under fault injection — a killed write leaves orphaned temp files, never a corrupt entry |

Cache invalidation is the highest-risk area in this architecture: a bug there does not crash, it silently serves stale output. Any change to CAS key derivation, integrity verification, or invalidation logic must add or update the invalidation-matrix tests.

Planned as later phases land: property-based semver range solving (`proptest`), a curated npm compatibility suite gating install and execute as separate CI jobs, `cargo-fuzz` on every parser of untrusted input, a V8 embedding-boundary suite, and a benchmark suite. **There is no benchmark harness yet** — no performance claims are published until one exists, since speed work is justified by measurement, not intuition.

CI (GitHub Actions) runs fmt, clippy, test, and build on `ubuntu-latest` and `macos-latest` for every push and PR against `master`. Native Windows is out of scope for v1 — see [Deployment](#deployment).

## Environment Variables

None currently defined. This section will be filled in as `opal-pm`'s registry client, cache paths, and any config overrides are implemented. Cache location is currently set via `--cache-dir`, or discovered from the platform's user cache directory.

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

**Implemented today:**

- **Core stack**: Rust for all native code; BLAKE3 for all content hashing (SIMD-accelerated, on the hot path for every file read); `oxc` as the JS parser (preferred over `swc` for performance).
- **`opal-core`**: parses/resolves import graphs, content-addresses every file and computed artifact, maintains an on-disk CAS, and answers "what changed since last run" via hash comparison — never mtime (unreliable across git checkouts, CI runners, and Docker layers). Every CAS write is atomic: temp file → verify BLAKE3 → rename into place, so a killed write leaves orphaned garbage rather than a corrupt entry.

**Target shape, not yet built** — the sections below describe intended design, and none of these commands exist in the binary today:

- **`opal-pm`** (`opal install`): resolves against the public npm registry, populates the CAS keyed by tarball content hash (enabling cross-package dedup), links packages via hardlinks from the CAS (pnpm-style), and writes a flat `opal.lock` lockfile. Install pipeline: `package.json → registry metadata → semver resolution → dependency graph → opal.lock → download → BLAKE3 integrity verification → CAS → node_modules`. The link step is a reconciler that diffs `opal.lock` against disk and applies only the delta, so an interrupted run resumes by re-running `opal install`.
- **`opal-runtime`** (`opal run`): executes JS/TS directly via embedded V8, using `opal-core`'s resolved graph for imports; lazy module instantiation for fast cold start; TypeScript via strip-types transpilation (no type-checking, matching the Bun/Deno model).
- **`opal-bundler`** (`opal build`): consumes the same graph, adds tree-shaking and minification, caches outputs in the CAS keyed by input hash.
- **`opal-test`** (`opal test`): thinnest layer — reuses the runtime's module loading/execution, adds test discovery, assertions, and a reporter.

The architectural bet is one resolver shared by every tool. A tool that implements its own import resolution, or shortcuts around the shared graph, defeats the entire design.

## Deployment

Opal ships as a single self-contained native binary — no runtime dependency on a separate install step or interpreter.

**v0.1.0 release targets**: `opal-linux-x64`, `opal-linux-arm64`, `opal-macos-x64`, `opal-macos-arm64`. Native Windows is out of scope for v1 (see platform support note below) — no `windows-x64` artifact until v2.

**Release process**:
1. Tag a release (e.g. `v0.1.0`).
2. CI (GitHub Actions) builds all four platform targets in release mode, from the same matrix run.
3. Each binary is packaged as a `.tar.gz` archive.
4. A `SHA256SUMS` file is generated across all artifacts.
5. Artifacts publish to GitHub Releases — the single canonical source of truth. Any future downstream package manager (Homebrew, Scoop, WinGet, npm wrapper, Docker) must fetch from here, never build independently.
6. The install script and installer endpoint (`opal.dev`, domain TBD) are updated to point at the new release.

A release ships all four targets or none — partial releases create version skew between platforms.

**End-user install** is a curl-based script that detects OS/arch and places the binary at `~/.opal/bin/opal`, added to `PATH`:

```bash
curl -fsSL https://opal.dev/install.sh | bash
```

**Hard constraint**: `cargo install opal` must never be presented as the primary install path for end users (requires a Rust toolchain) — it's contributor-only, for building Opal itself from source.

Platform support: macOS, Linux, and WSL2 in v1 (WSL2 runs a genuine Linux kernel, so the Linux build target covers it directly). Native Windows (non-WSL) is v2, requiring junction-based fallbacks for linking and path-separator abstraction throughout `opal-core`.

## Contributing

- Follow the naming and package-layout conventions described under [Repository layout](#repository-layout).
- Run `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings` before opening a PR — CI treats Clippy warnings as errors.
- Open PRs against `master`.
- New subsystem work should follow the phased build order above — don't start a later phase's crate, or add it to the workspace, before the prior phase's exit criteria are met.
- Core architecture — resolver design, pipeline stages, cache scheme, the on-disk lockfile/CAS format — gets discussed before it gets changed. Those decisions constrain every later phase.

## License

[MIT](./LICENSE) © Sharif Parish / Bluesky Labs
