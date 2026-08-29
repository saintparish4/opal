# Opal

An all-in-one JavaScript/TypeScript toolkit — package manager, runtime, bundler, and test runner in a single native binary, built around one shared incremental module graph engine (`opal-core`) instead of four independently-implemented resolvers.

> **Status**: Phase 0 (`opal-core`) and Phase 1 (`opal-pm`) are complete and meet their exit criteria. `opal install` against a real `package.json` (e.g. `express`) produces a `node_modules` tree Node can `require` against, `opal-core`'s resolver walks that tree with nothing unresolved, and SIGKILL at any of seven pipeline stages converges on re-run. `opal-runtime`, `opal-bundler`, and `opal-test` are **not implemented** and are not yet workspace members; the directories under `crates/` for those phases hold empty placeholder files only. See [Architecture](#architecture) for the target shape, [Build order](#build-order) for sequencing, and [What Phase 1 left for later](#what-phase-1-left-for-later) for what `opal install` deliberately does not do yet.

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

The binary ships only what is implemented. `run`, `build`, and `test` are absent by design — a command that exists and does nothing is worse than one that does not exist.

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

### `opal install` — install the dependencies in `package.json`

```bash
opal install [--root <ROOT>] [--cache-dir <CACHE_DIR>] [--registry <URL>] [--production] [--frozen-lockfile] [--offline | --prefer-offline]
```

| Flag | Description |
|---|---|
| `--root` | Project directory. Defaults to the current directory |
| `--cache-dir` | Cache location. Defaults to the discovered user cache directory |
| `--registry` | Registry base URL. Defaults to `$OPAL_REGISTRY`, else the public npm registry |
| `--production` | Link `dependencies` only. `opal.lock` still records `devDependencies`, so it stays byte-identical and works in CI alongside `--frozen-lockfile` |
| `--frozen-lockfile` | Fail instead of re-resolving when `opal.lock` does not match `package.json` |
| `--offline` | Resolve from cached registry metadata only; never reach the network |
| `--prefer-offline` | Use cached registry metadata however old it is, and fetch only what is missing |

Packages declaring an `os` or `cpu` this host cannot run are recorded in `opal.lock` and skipped at install time, so one committed lockfile installs the right native binary on every platform. A platform-mismatched package that nothing declared optional is `EBADPLATFORM`, matching npm — skipping it silently would produce a tree that cannot run.

`npm:` alias specifiers (`"string-width-cjs": "npm:string-width@^4.2.0"`) install one package under another's name, which is how a package depends on two majors of one dependency at once. `git:` and `file:` specifiers remain unsupported in v1: unlike an alias, they resolve somewhere other than the public registry.

Progress is reported on stderr as each stage begins — a spinner while resolving and linking, a bar advancing per package while fetching — with the summary on stdout. When stderr is not a terminal, the same stages print as plain lines, so a CI log stays readable and nothing redraws over it.

Resolves against the public npm registry, downloads tarballs into the shared CAS keyed by content hash, and links `node_modules` from the CAS via hardlinks — a reconciler that diffs `opal.lock` against disk and applies only the delta, so a killed install converges by re-running `opal install`:

```console
$ opal install
71 packages resolved in 26.8s
store:  71 fetched, 0 already present
link:   71 added, 0 unchanged, 0 removed (657 hardlinked, 2 copied, 1 bins)

$ opal install                      # warm: nothing changed
71 packages from opal.lock in 37.9ms
store:  0 fetched, 71 already present
link:   0 added, 71 unchanged, 0 removed (0 hardlinked, 0 copied, 1 bins)
```

`opal.lock` is written atomically (`opal.lock.tmp` → fsync → rename), and a per-project flock serializes concurrent installs against the same project rather than letting them interleave writes. Lifecycle scripts (`preinstall`/`install`/`postinstall`) do not run — see [What Phase 1 left for later](#what-phase-1-left-for-later).

### `opal cache` — inspect the shared CAS

```bash
opal cache verify [--cache-dir <DIR>]                                  # re-hash every object, check it against its key
opal cache gc [--cache-dir <DIR>] [--dry-run] [--project <PATH>]...    # remove objects no live project points at, plus stale temp files
opal cache path [--cache-dir <DIR>]                                    # print the cache location
```

`verify` exits non-zero if any object's content does not match its hash key. `gc --dry-run` reports what would be removed without removing it; a repeatable `--project` treats a given directory's `opal.lock` as live without recording it, for CI where the cache outlives the checkout. `gc` blocks while an install is in flight against the shared cache, and never collects a package a still-installed project needs:

```console
$ opal cache gc                     # the project from above is still here
projects: 1 tracked, 0 forgotten
packages: 71 live (0 in a lockfile but never fetched here)
0 of 686 objects removed, 0.0 MiB
pointers: 0 pruned
temp files: 0 swept, 0 still in flight
```

## Development

```bash
cargo build --workspace
cargo test --workspace --all-features
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- Format with `cargo fmt`.
- Lint with `cargo clippy -- -D warnings` — Clippy warnings are treated as errors; do not hand-format against rustfmt defaults or leave warnings unaddressed.
- Run both before considering a change complete.
- `--all-features` matters: it's what turns on the `fixtures` module both integration suites (`install-pipeline`, `install-crash-safety`) build against. Without it those suites don't compile, and clippy won't see them either.

### Repository layout

A Cargo workspace. Only the crates below marked *implemented* are workspace members — the rest are added as their phase begins, so the workspace never carries a crate whose API has not been designed yet.

```
opal/
├── crates/
│   ├── opal-core/       # implemented — module graph, resolver, CAS, BLAKE3 hashing, memoization
│   ├── opal-cli/        # implemented — `opal` binary; dispatches `graph`, `install`, and `cache`
│   ├── opal-pm/         # implemented — semver resolution, registry client, lockfile, node_modules linker, GC
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

Inside `opal-pm`:

| Module | Responsibility |
|---|---|
| `semver` | Version and range parsing, matching, `max_satisfying` |
| `manifest` | `package.json` parsing |
| `registry` | npm registry client (packument fetch, dist-tag resolution) |
| `resolve` | Dependency graph resolution against the registry |
| `integrity` | `dist.integrity` (sha512) and legacy `shasum` (sha1) verification |
| `package` | Tarball extraction into the CAS, content-addressed and pointer-backed |
| `lockfile` | `opal.lock` read/write, atomic (`opal.lock.tmp` → fsync → rename) |
| `link` | The `node_modules` reconciler — diffs `opal.lock` against disk, applies only the delta |
| `install` | The end-to-end pipeline wiring the above together |
| `diagnose` | Classifies unresolved imports (missing optional dep, undeclared import, etc.) |
| `projects` | Tracks which projects are live, for GC |
| `gc` | Mark-and-sweep collection of CAS objects no live project's lockfile points at |
| `locks` | The two flocks: per-project install lock, shared cache lock |
| `fixtures` (feature `fixtures`) | A file-backed registry shared by `opal-pm`'s and `opal-cli`'s integration suites |

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
| 1 | `opal-pm` | **complete** | `opal install` against a real `package.json` produces a working `node_modules` that Node can run against; SIGKILL at randomized pipeline points always converges on re-run |
| 2 | `opal-runtime` | not started | `opal run` executes a real project's entrypoint, including `node_modules` dependencies from Phase 1 |
| 3 | `opal-bundler` | not started | Tree-shaking + minification over the resolved graph, outputs cached in the CAS |
| 4 | `opal-test` | not started | Test discovery via the graph, wired into the Phase 2 execution path |

## Testing

```bash
cargo test --workspace --all-features
```

256 tests currently pass, organized by **risk category** rather than a unit/integration/e2e pyramid — the question is where the system actually breaks, and what a bug looks like when it does:

| Suite | Count | Covers |
|---|---|---|
| `opal-core` unit | 51 | Hashing, path abstraction, CAS layout, graph construction, resolver internals |
| `opal-pm` unit | 93 | Semver parsing/matching, manifests, registry client and retry policy, integrity verification, tarball ingestion and its ceilings, lockfile, linker planning, platform matching, locks, GC bookkeeping |
| `tests/cache-invalidation.rs` (`opal-core`) | 16 | The invalidation matrix: content change, add/remove, direct and transitive dependency change — asserting the right hits *and* misses. Includes the "never mtime" invariant as a direct test, and memo-record pruning |
| `tests/graph-resolution.rs` (`opal-core`) | 15 | Resolution against fixture trees, plus a golden/snapshot test of resolved graph output (`tests/golden/`) |
| `tests/cas-crash-safety.rs` (`opal-core`) | 6 | Atomic CAS writes under fault injection — a killed write leaves orphaned temp files, never a corrupt entry |
| `tests/install-pipeline.rs` (`opal-pm`) | 39 | The full install pipeline end to end, incl. `test_node_can_require_the_installed_tree` and `test_the_module_graph_resolves_against_the_installed_tree` — the Phase 0 ↔ Phase 1 contract |
| `tests/packument-cache.rs` (`opal-pm`) | 8 | When the registry client reaches the wire and when it does not: freshness, revalidation, `--offline`, and never answering one registry from another's cache |
| `tests/resolution-properties.rs` (`opal-pm`) | 10 | `proptest` over generated registries: every resolved edge satisfies the range that asked for it, every root resolves to a version its own spec allows, and the layout places everything the resolution keeps |
| `tests/semver-properties.rs` (`opal-pm`) | 12 | `proptest` over the range algebra in isolation |
| `tests/install-crash-safety.rs` (`opal-cli`) | 6 | SIGKILL at each of seven pipeline stages converges on re-run; a killed lockfile rewrite leaves the previous lockfile byte-identical; two racing installs serialize instead of interleaving; `opal cache gc` blocks on an in-flight install rather than racing it |
| `tests/npm-compatibility.rs` (`opal-cli`) | 15 | Real packages from the public registry, curated by the edge case each exercises. `#[ignore]` by default; install and execute run as separate CI jobs |

Cache invalidation is the highest-risk area in this architecture: a bug there does not crash, it silently serves stale output. Any change to CAS key derivation, integrity verification, or invalidation logic must add or update the invalidation-matrix tests.

`--all-features` turns on the `fixtures` module both `install-pipeline.rs` and `install-crash-safety.rs` build against — a file-backed registry so those suites run offline, without hitting the real npm registry.

Fuzzing lives in `fuzz/`, its own workspace so that `cargo fuzz`'s sanitizer flags never reach an ordinary build. Four targets cover the inputs that are not trusted — registry JSON, tarball bytes, `package.json`, and `opal.lock` — and standing them up found three bugs in the lockfile round trip, one of which let a dependency write lines into the lockfile of every project installing it. See `fuzz/README.md`.

Benchmarks live in `benches/install-pipeline`, which times four scenarios separately (`cold`, `resolve`, `link`, `noop`) because collapsing them into one number is how a ten-minute install can look ordinary. Per the testing strategy it tracks numbers and never gates CI on them.

Still to come as later phases land: a V8 embedding-boundary suite, and overlapping the tarball downloads — the last remaining round-trip stall, and the only one the metadata cache did not remove.

CI (GitHub Actions) runs fmt, clippy, test, and build on `ubuntu-latest` and `macos-latest` for every push and PR against `master`. Native Windows is out of scope for v1 — see [Deployment](#deployment).

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OPAL_REGISTRY` | No | `https://registry.npmjs.org` | Registry base URL used by `opal install`. Overridden per-invocation by `--registry` |

Cache location has no environment variable — it's set via `--cache-dir`, or discovered from the platform's user cache directory.

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
- **`opal-pm`** (`opal install`): resolves against the public npm registry, populates the CAS keyed by tarball content hash (enabling cross-package dedup), links packages via hardlinks from the CAS (pnpm-style), and writes a flat `opal.lock` lockfile. Install pipeline: `package.json → registry metadata → semver resolution → dependency graph → opal.lock → download → BLAKE3 integrity verification → CAS → node_modules`. The link step is a reconciler that diffs `opal.lock` against disk and applies only the delta, so an interrupted run resumes by re-running `opal install`. The resolved tree feeds straight back into `opal-core`'s resolver, with nothing unresolved — see [`opal install`](#opal-install--install-the-dependencies-in-packagejson) and [What Phase 1 left for later](#what-phase-1-left-for-later).

**Target shape, not yet built** — the sections below describe intended design, and none of these commands exist in the binary today:

- **`opal-runtime`** (`opal run`): executes JS/TS directly via embedded V8, using `opal-core`'s resolved graph for imports; lazy module instantiation for fast cold start; TypeScript via strip-types transpilation (no type-checking, matching the Bun/Deno model).
- **`opal-bundler`** (`opal build`): consumes the same graph, adds tree-shaking and minification, caches outputs in the CAS keyed by input hash.
- **`opal-test`** (`opal test`): thinnest layer — reuses the runtime's module loading/execution, adds test discovery, assertions, and a reporter.

The architectural bet is one resolver shared by every tool. A tool that implements its own import resolution, or shortcuts around the shared graph, defeats the entire design.

### What Phase 1 left for later

`opal install` deliberately does not do the following yet — see `base/directive/p1.md` for the full rationale:

- **Lifecycle scripts** (`preinstall`/`install`/`postinstall`) do not run. A package needing `node-gyp` installs but does not build — native addons are best-effort per the PRD.
- **Peer auto-install.** Peers are recorded and classified, never fetched.
- **`opal add` / `remove` / `update` / `why` / `outdated` / `audit` / `publish`** are not implemented, and deliberately absent from the CLI rather than stubbed.
- **Parallel downloads.** Installs fetch sequentially. With metadata now cached across runs, a re-resolve makes no round trips at all, but a cold install is still roughly half round-trip stall — all of it tarballs, and the only stall left in the pipeline.
- **Git and `file:` specifiers** are reported as unsupported, never guessed at — v1 resolves the public registry only. `npm:` aliases *are* supported, since they resolve there too.
- **Lifecycle scripts still do not run**, so a deprecation notice is the only thing a package gets to say during an install.
- **Collector starvation.** The cache flock isn't fair, so a continuous stream of installs can keep `opal cache gc` waiting indefinitely. Nothing is lost when it does, since collection isn't on any critical path.

### Fixed since v0.1.0

Every gap the `create-next-app` validation found is closed, each with the regression test it should have had. Full detail in `base/optimization-plan.md`.

- **`os`/`cpu` filtering.** Platform variants of a native optional are recorded in `opal.lock` and skipped at install time — `esbuild` declares 25 of them totalling 256 MB, and one 9.8 MB binary is installed. A mismatched package nothing declared optional is `EBADPLATFORM`, matching npm.
- **Root versions are pinned.** A transitive dependency's higher major no longer displaces the version the project asked for — that was a silent wrong-version install, the worst failure mode the testing strategy names.
- **`--production` and `--frozen-lockfile` work together.** `opal.lock` stays dev-complete and byte-identical, so the ordinary CI invocation no longer fails against a valid lockfile.
- **Metadata is cached across runs.** A re-resolve against a warm store went from 7.6s to 0.36s on a 74-package tree, and makes no round trips at all.
- **Extensionless shebang scripts are walked.** `opal graph` against `typescript/bin/tsc` reported one module and zero edges; it now reports three modules and ten edges.
- **`diagnose` stopped giving advice that cannot work.** A dependency's own devDependency reads as a note, not an error telling you to run a command that cannot fix it.

## Deployment

Opal ships as a single self-contained native binary — no runtime dependency on a separate install step or interpreter.

**Release targets**: `opal-linux-x64`, `opal-linux-arm64`, `opal-macos-x64`, `opal-macos-arm64`. Native Windows is out of scope for v1 (see platform support note below) — no `windows-x64` artifact until v2.

**Release process**:
1. Tag a release (e.g. `v0.2.0`).
2. CI (GitHub Actions) builds all four platform targets in release mode, from the same matrix run.
3. Each binary is packaged as a `.tar.gz` archive.
4. A `SHA256SUMS` file is generated across all artifacts.
5. Artifacts publish to GitHub Releases — the single canonical source of truth. Any future downstream package manager (Homebrew, Scoop, WinGet, npm wrapper, Docker) must fetch from here, never build independently.
6. The install script and installer endpoint are updated to point at the new release, once a branded domain exists to host it under.

A release ships all four targets or none — partial releases create version skew between platforms.

**End-user install** is a curl-based script that detects OS/arch and places the binary at `~/.opal/bin/opal`, added to `PATH`. There's no branded install domain yet — `opal.dev` turned out to already be a live, unrelated site, so the final domain is still undecided. Until one exists, the same script is reachable straight off `master`, and has already been self-tested end to end: a real tagged release, a real CI-built binary, real checksum verification.

> **Not live under a branded domain, and experimental** — Opal is pre-1.0, the install pipeline itself is only days old, and `opal install` has [known gaps](#known-gaps-from-real-world-validation) found by real-world testing. **Run at your own risk.**
>
> Safer: try it in an isolated shell first, so nothing touches your real `PATH` or shell config until you've checked it works:
> ```bash
> env -i HOME="$HOME" PATH="/usr/bin:/bin" bash --noprofile --norc
> curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash
> ```
>
> Or straight into your current shell, once you're comfortable with it:
> ```bash
> curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash
> ```

**Hard constraint**: `cargo install opal` must never be presented as the primary install path for end users (requires a Rust toolchain) — it's contributor-only, for building Opal itself from source.

Platform support: macOS, Linux, and WSL2 in v1 (WSL2 runs a genuine Linux kernel, so the Linux build target covers it directly). Native Windows (non-WSL) is v2, requiring junction-based fallbacks for linking and path-separator abstraction throughout `opal-core`.

## Contributing

- Follow the naming and package-layout conventions described under [Repository layout](#repository-layout).
- Run `cargo fmt` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` before opening a PR — CI treats Clippy warnings as errors.
- Open PRs against `master`.
- New subsystem work should follow the phased build order above — don't start a later phase's crate, or add it to the workspace, before the prior phase's exit criteria are met.
- Core architecture — resolver design, pipeline stages, cache scheme, the on-disk lockfile/CAS format — gets discussed before it gets changed. Those decisions constrain every later phase.

## License

[MIT](./LICENSE) © Sharif Parish / Bluesky Labs
