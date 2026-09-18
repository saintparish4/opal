# Opal

An all-in-one JavaScript/TypeScript toolkit — package manager, runtime, bundler, and test runner in a single native binary, built around one shared incremental module graph engine (`opal-core`) instead of four independently-implemented resolvers.

> **Status**: Beta. The package manager works today — `opal install` resolves against the real npm registry and produces a `node_modules` tree Node runs against, validated on real projects (a Next.js scaffold at 365 packages, express, webpack, and a curated compatibility suite), and it is under active development, so expect rough edges and breaking releases. The runtime (`opal run`), bundler (`opal build`), and test runner (`opal test`) are **not implemented**; their directories under `crates/` hold placeholder files only. See [Architecture](#architecture) for the target shape.

## Requirements

- **Language/runtime**: Rust, `stable` channel, pinned via [`rust-toolchain.toml`](./rust-toolchain.toml) (installs the `rustfmt` and `clippy` components automatically via `rustup`).
- **Package manager**: Cargo (ships with the Rust toolchain above).
- **C/C++ toolchain**: required once V8 embedding lands in `opal-runtime` — `build-essential` on Linux, Xcode Command Line Tools on macOS. Not needed to build the current workspace.
- **`cmake` and `ninja`**: recommended for the future V8-related builds. Not needed today.
- **Database**: none — state is a local content-addressed store (CAS) on disk, not a database.
- **Other services**: none currently.
- **OS-level dependencies**: `git`.
- **Supported dev platforms**: macOS, Linux, or WSL2 on Windows. Native Windows is a v2 target.

## Installation

**Using Opal** (prebuilt binary, no Rust required):

```bash
curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash
```

The script detects OS/arch, downloads the latest [GitHub Release](https://github.com/saintparish4/opal/releases), verifies its SHA256 checksum, and places the binary at `~/.opal/bin/opal` on your `PATH`. Releases exist and are real — currently `v0.3.0` — but this is beta software: pre-1.0, breaking lockfile changes happen between minor versions, and it is not yet something to run against a project you cannot reinstall. To try it without touching your shell config first:

```bash
env -i HOME="$HOME" PATH="/usr/bin:/bin" bash --noprofile --norc
curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash
```

**Building from source** (contributors):

```bash
git clone https://github.com/saintparish4/opal
cd opal

cargo build --release   # binary at ./target/release/opal
```

`cargo install opal` is not the end-user path — end users should never need a Rust toolchain.

## Usage

The binary ships only what is implemented. `run`, `build`, and `test` are absent by design — a command that exists and does nothing is worse than one that does not exist. The same goes for `add`, `remove`, `update`, `why`, `outdated`, `audit`, and `publish`: not implemented yet, so not stubbed.

### `opal graph` — resolve a module graph

```bash
opal graph <ENTRY> [--root <ROOT>] [--cache-dir <CACHE_DIR>] [--json]
```

| Flag | Description |
|---|---|
| `<ENTRY>` | Entry file to walk from (required) |
| `--root` | Project root; resolution happens against this directory's `node_modules`, and module paths are reported relative to it. Defaults to the entry's parent directory — **pass `--root .` from your project root whenever the entry is not a top-level file**, or packages will not resolve |
| `--cache-dir` | Cache location. Defaults to `$OPAL_CACHE_DIR`, else the platform cache directory |
| `--json` | Print the resolved graph as JSON instead of the summary |

Walking a tree installed by `opal install` (works identically against a tree npm created — the compatibility check that matters is that Opal resolves a `node_modules` it did not build):

```console
$ opal graph index.js --root .
146 modules, 311 edges in 139.4ms
cache:  MISS (no record)
digest: 17f9f3684825419c873c15d69062b41b4b1fb79bbf742c384dda551fd03e0185
graph:  a4de6468733c97fbfb374dcbe7e647f8824ec95216ba2b80c99985a1b287184f

$ opal graph index.js --root .
146 modules, 311 edges in 26.6ms
cache:  HIT
digest: 17f9f3684825419c873c15d69062b41b4b1fb79bbf742c384dda551fd03e0185
graph:  a4de6468733c97fbfb374dcbe7e647f8824ec95216ba2b80c99985a1b287184f
```

The digest is identical across both runs; only the cache status differs. Touching a file's mtime still reports `HIT` — invalidation is content-hash only, never mtime. Changing a byte reports `MISS (changed: <path>)`, naming the file that moved.

A specifier that cannot be resolved is reported as a diagnostic, never a fatal error — real projects import optional dependencies, platform-specific natives, and packages that are not installed:

```console
unresolved specifiers: 1
  node_modules/debug/src/node.js: cannot resolve "supports-color": package "supports-color" is not installed under the project root
```

Known friction: needing an explicit entry file and `--root .` is more ceremony than the command deserves. A planned change makes `opal graph` with no arguments discover the entry from `package.json` and default the root to the current directory.

### `opal install` — install the dependencies in `package.json`

```bash
opal install [--root <ROOT>] [--cache-dir <CACHE_DIR>] [--registry <URL>] [--production] [--frozen-lockfile] [--offline | --prefer-offline]
```

| Flag | Description |
|---|---|
| `--root` | Project directory. Defaults to the current directory |
| `--cache-dir` | Cache location. Defaults to `$OPAL_CACHE_DIR`, else the platform cache directory |
| `--registry` | Registry base URL. Defaults to `$OPAL_REGISTRY`, else the public npm registry |
| `--production` | Link `dependencies` only. `opal.lock` still records `devDependencies`, so it stays byte-identical and works in CI alongside `--frozen-lockfile` |
| `--frozen-lockfile` | Fail instead of re-resolving when `opal.lock` does not match `package.json` |
| `--offline` | Resolve from cached registry metadata only; never reach the network |
| `--prefer-offline` | Use cached registry metadata however old it is, and fetch only what is missing |

Packages declaring an `os` or `cpu` this host cannot run are recorded in `opal.lock` and skipped at install time, so one committed lockfile installs the right native binary on every platform. A platform-mismatched package that nothing declared optional is `EBADPLATFORM`, matching npm — skipping it silently would produce a tree that cannot run.

`npm:` alias specifiers (`"string-width-cjs": "npm:string-width@^4.2.0"`) install one package under another's name, which is how a package depends on two majors of one dependency at once.

Versions are picked the way npm picks them: the `latest` dist-tag when it satisfies the range and isn't deprecated, otherwise the newest satisfying version that isn't deprecated, otherwise the newest. A release published without moving `latest` is therefore not installed until the tag moves, which is how npm treats it too. npm's `engines` check is the one rule left out, because it needs the running Node's version. Before any of that, a version already chosen elsewhere in the tree is reused if it satisfies the range, which keeps the tree small.

Progress is reported on stderr as each stage begins — a spinner while resolving and linking, a bar advancing per package while fetching — with the summary on stdout. When stderr is not a terminal, the same stages print as plain lines, so a CI log stays readable and nothing redraws over it.

Resolves against the public npm registry, downloads tarballs into the shared CAS keyed by content hash, and links `node_modules` from the CAS via hardlinks — a reconciler that diffs `opal.lock` against disk and applies only the delta, so a killed install converges by re-running `opal install`. The summary reports each phase separately, because the phases are slow for unrelated reasons:

```console
$ opal install
71 packages resolved in 18.1s  (resolve 4.7s, fetch 12.5s, link 924.4ms)
store:  71 fetched, 0 already present
link:   71 added, 0 unchanged, 0 removed (657 hardlinked, 2 copied, 1 bins)

$ opal install                      # warm: nothing changed
71 packages from opal.lock in 58.8ms  (resolve 0.0ns, fetch 5.9ms, link 15.0ms)
store:  0 fetched, 71 already present
link:   0 added, 71 unchanged, 0 removed (0 hardlinked, 0 copied, 1 bins)
```

Registry metadata is cached on disk between runs, which is what makes a re-resolve cheap: on a 74-package tree with a warm store, deleting `opal.lock` and re-resolving takes ~0.4s instead of the 7.6s it cost before the cache existed. Commit `opal.lock` and leave it alone — an install it answers skips resolution entirely.

`opal.lock` is written atomically (`opal.lock.tmp` → fsync → rename), and a per-project flock serializes concurrent installs against the same project rather than letting them interleave writes.

Current limitations worth knowing before pointing this at a project:

- **Lifecycle scripts (`preinstall`/`install`/`postinstall`) do not run.** Packages shipping prebuilt binaries (`esbuild`, `sharp`, `@next/swc`) work; a package that needs `node-gyp` to compile at install time installs but does not build. `opal install` says so on every run: it names each dependency whose install scripts were skipped (including native addons that declare none and rely on npm running `node-gyp rebuild` for their `binding.gyp`), and the project's own lifecycle scripts, `prepare` included.
- **Peers are recorded and classified, never auto-installed.**
- **`git:` and `file:` specifiers are unsupported** and reported as such — resolution is against the public registry only.
- **Downloads are sequential.** Linking runs in parallel, one `node_modules` depth at a time, but a cold install is still round-trip bound; parallel fetching is planned.
- **If your project and cache sit on different filesystems** (a project on `/mnt/c` under WSL2 with the default cache, for instance), every file is copied instead of hardlinked and the install warns. Keep both on the same filesystem, or set `OPAL_CACHE_DIR`.

### `opal cache` — inspect the shared CAS

```bash
opal cache verify [--cache-dir <DIR>]                                  # re-hash every object, check it against its key
opal cache gc [--cache-dir <DIR>] [--dry-run] [--project <PATH>]...    # remove objects no live project points at, plus stale temp files
opal cache path [--cache-dir <DIR>]                                    # print the cache location
```

`verify` exits non-zero if any object's content does not match its hash key. `gc --dry-run` reports what would be removed without removing it; a repeatable `--project` treats a given directory's `opal.lock` as live without recording it, for CI where the cache outlives the checkout. `gc` blocks while an install is in flight against the shared cache, never collects a package a still-installed project needs, and also prunes graph records whose project is gone and registry metadata untouched for 30 days:

```console
$ opal cache gc                     # the project from above is still here
projects: 1 tracked, 0 forgotten
packages: 71 live (0 in a lockfile but never fetched here)
0 of 687 objects removed, 0.0 MiB
pointers: 0 pruned
records:  0 graph, 0 metadata pruned
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

A Cargo workspace. Only the crates below marked *implemented* are workspace members — the rest are added when work on them begins, so the workspace never carries a crate whose API has not been designed yet.

```
opal/
├── crates/
│   ├── opal-core/       # implemented — module graph, resolver, CAS, BLAKE3 hashing, memoization
│   ├── opal-cli/        # implemented — `opal` binary; dispatches `graph`, `install`, and `cache`
│   ├── opal-pm/         # implemented (beta) — semver resolution, registry client, lockfile, node_modules linker, GC
│   ├── opal-runtime/    # planned — placeholder files only, not a workspace member
│   ├── opal-bundler/    # planned — placeholder files only, not a workspace member
│   └── opal-test/       # planned — placeholder files only, not a workspace member
├── fuzz/                # cargo-fuzz targets, its own workspace (see Testing)
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
| `manifest` | `package.json` parsing, including `os`/`cpu` and `npm:` aliases |
| `registry` | npm registry client: transport seam, retries, timeouts, abbreviated packuments |
| `packuments` | The on-disk registry-metadata cache, with ETag revalidation |
| `resolve` | Dependency graph resolution against the registry |
| `integrity` | `dist.integrity` (sha512) and legacy `shasum` (sha1) verification |
| `package` | Tarball extraction into the CAS, content-addressed, pointer-backed, and bounded against decompression bombs |
| `platform` | npm's `os`/`cpu` host matching |
| `lockfile` | `opal.lock` read/write, atomic (`opal.lock.tmp` → fsync → rename) |
| `link` | The `node_modules` reconciler — diffs `opal.lock` against disk, applies only the delta |
| `install` | The end-to-end pipeline wiring the above together |
| `progress` | The reporting seam the pipeline calls; rendering lives in `opal-cli` |
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

Crates are built strictly in sequence — each is a prerequisite for the next, and each has a concrete definition of done.

| Crate | Status | Definition of done |
|---|---|---|
| `opal-core` | **done** | Resolve a real-world project's dependency graph, cache the result, demonstrate cache hits on unchanged input; SIGKILL mid-CAS-write never leaves a corrupt entry |
| `opal-pm` | **working, beta** | `opal install` against a real `package.json` produces a working `node_modules` that Node can run against; SIGKILL at randomized pipeline points always converges on re-run |
| `opal-runtime` | not started | `opal run` executes a real project's entrypoint, including its `node_modules` dependencies |
| `opal-bundler` | not started | Tree-shaking + minification over the resolved graph, outputs cached in the CAS |
| `opal-test` | not started | Test discovery via the graph, wired into the runtime's execution path |

## Testing

```bash
cargo test --workspace --all-features
```

293 tests currently pass, organized by **risk category** rather than a unit/integration/e2e pyramid — the question is where the system actually breaks, and what a bug looks like when it does:

| Suite | Count | Covers |
|---|---|---|
| `opal-core` unit | 54 | Hashing, path abstraction, CAS layout, graph construction, resolver internals |
| `opal-pm` unit | 112 | Semver parsing/matching, manifests and their lifecycle scripts, npm's version preference (`latest`, then not deprecated, then newest), registry client and retry policy, integrity verification, tarball ingestion and its ceilings, lockfile, linker planning and depth ordering, the linking worker pool, platform matching, locks, GC bookkeeping |
| `tests/cache-invalidation.rs` (`opal-core`) | 16 | The invalidation matrix: content change, add/remove, direct and transitive dependency change — asserting the right hits *and* misses. Includes the "never mtime" invariant as a direct test, and memo-record pruning |
| `tests/graph-resolution.rs` (`opal-core`) | 16 | Resolution against fixture trees, plus a golden/snapshot test of resolved graph output (`tests/golden/`) |
| `tests/exports-properties.rs` (`opal-core`) | 1 | `proptest` over `exports` maps and specifiers built from the segments that move a path: whatever a package's exports resolve to stays inside the package, which is Node's rule |
| `tests/cas-crash-safety.rs` (`opal-core`) | 6 | Atomic CAS writes under fault injection — a killed write leaves orphaned temp files, never a corrupt entry |
| `tests/install-pipeline.rs` (`opal-pm`) | 48 | The full install pipeline end to end, incl. `test_node_can_require_the_installed_tree` and `test_the_module_graph_resolves_against_the_installed_tree` — the `opal-core` ↔ `opal-pm` contract |
| `tests/packument-cache.rs` (`opal-pm`) | 8 | When the registry client reaches the wire and when it does not: freshness, revalidation, `--offline`, and never answering one registry from another's cache |
| `tests/resolution-properties.rs` (`opal-pm`) | 11 | `proptest` over generated registries (with `latest` tags that lag and deprecated releases): every resolved edge satisfies the range that asked for it, version preference matches npm's order, every root resolves to a version its own spec allows, and the layout places everything the resolution keeps |
| `tests/semver-properties.rs` (`opal-pm`) | 12 | `proptest` over the range algebra in isolation |
| `tests/install-crash-safety.rs` (`opal-cli`) | 8 | SIGKILL at each of seven pipeline stages converges on re-run, including mid-link in a tree three `node_modules` levels deep, and so do kills at random moments (seeded: replay a failure with the `OPAL_CHAOS_SEED` it prints, run longer with `OPAL_CHAOS_TRIALS`); a killed lockfile rewrite leaves the previous lockfile byte-identical; two racing installs serialize instead of interleaving; `opal cache gc` blocks on an in-flight install rather than racing it |
| `tests/install-relative-root.rs` (`opal-cli`) | 1 | `opal install --root .` from inside a project: an unchanged tree stays unchanged, and nothing outside the project is touched |
| `tests/npm-compatibility.rs` (`opal-cli`) | 15 | Real packages from the public registry, curated by the edge case each exercises. `#[ignore]` by default; install and execute run as separate CI jobs |
| `tests/npm-cross-check.rs` (`opal-pm`) | 7 | npm and opal resolve the same `package.json`, and the trees must match package for package: same versions for the project's dependencies, the same set of package versions overall, and each side reading the other's picks as inside their ranges (npm's own `semver` checks opal's). A seventh test holds opal's version preference to `npm-pick-manifest` itself across 12,032 generated cases. `#[ignore]` by default; its own CI job |

Cache invalidation is the highest-risk area in this architecture: a bug there does not crash, it silently serves stale output. Any change to CAS key derivation, integrity verification, or invalidation logic must add or update the invalidation-matrix tests.

`--all-features` turns on the `fixtures` module both `install-pipeline.rs` and `install-crash-safety.rs` build against — a file-backed registry so those suites run offline, without hitting the real npm registry.

Fuzzing lives in `fuzz/`, its own workspace so that `cargo fuzz`'s sanitizer flags never reach an ordinary build. Five targets cover the inputs that are not trusted: registry JSON, tarball bytes, `package.json`, `opal.lock`, and JS/TS source through the resolver, together with a dependency's `exports` map. Standing them up found three bugs in the lockfile round trip, one of which let a dependency write lines into the lockfile of every project installing it, and one in the resolver, which let a package's `exports` resolve outside the package. See `fuzz/README.md`.

Benchmarks live in `benches/install-pipeline`, which times four scenarios separately (`cold`, `resolve`, `link`, `noop`) because collapsing them into one number is how a ten-minute install can look ordinary. Per the testing strategy it tracks numbers and never gates CI on them: a CI job runs it on every push and PR, at 0 ms and 25 ms of simulated latency, and records the results in the job summary and as a JSON artifact. Nothing fails on a measurement until a noise threshold is agreed.

Still to come: a V8 embedding-boundary suite once the runtime exists, and parallel fetching — the remaining cost the metadata cache did not remove.

CI (GitHub Actions) runs fmt, clippy, test, and build on `ubuntu-latest` and `macos-latest` for every push and PR against `master`, plus three registry-backed jobs (npm compatibility install, npm compatibility execute, and the npm resolution cross-check) and the install benchmark, which records numbers and never fails the build. Native Windows is out of scope for v1.

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `OPAL_REGISTRY` | No | `https://registry.npmjs.org` | Registry base URL used by `opal install`. Overridden per-invocation by `--registry` |
| `OPAL_CACHE_DIR` | No | The platform user cache directory (`~/.cache/opal` on Linux, `~/Library/Caches/opal` on macOS) | Where the shared CAS, graph records, and registry metadata live. Overridden per-invocation by `--cache-dir` |

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
- **`opal-pm`** (`opal install`): resolves against the public npm registry, populates the CAS keyed by tarball content hash (enabling cross-package dedup), links packages via hardlinks from the CAS, and writes a flat `opal.lock` lockfile. Install pipeline: `package.json → registry metadata → semver resolution → dependency graph → opal.lock → download → BLAKE3 integrity verification → CAS → node_modules`. The link step is a reconciler that diffs `opal.lock` against disk and applies only the delta, so an interrupted run resumes by re-running `opal install`. The resolved tree feeds straight back into `opal-core`'s resolver, with nothing unresolved.

**Target shape, not yet built** — the sections below describe intended design, and none of these commands exist in the binary today:

- **`opal-runtime`** (`opal run`): executes JS/TS directly via embedded V8, using `opal-core`'s resolved graph for imports; lazy module instantiation for fast cold start; TypeScript via strip-types transpilation (no type-checking, matching the Bun/Deno model).
- **`opal-bundler`** (`opal build`): consumes the same graph, adds tree-shaking and minification, caches outputs in the CAS keyed by input hash.
- **`opal-test`** (`opal test`): thinnest layer — reuses the runtime's module loading/execution, adds test discovery, assertions, and a reporter.

The architectural bet is one resolver shared by every tool. A tool that implements its own import resolution, or shortcuts around the shared graph, defeats the entire design.

## Deployment

Opal ships as a single self-contained native binary — no runtime dependency on a separate install step or interpreter.

**Release targets**: `opal-linux-x64`, `opal-linux-arm64`, `opal-macos-x64`, `opal-macos-arm64`, all built from the same CI matrix run with a combined `SHA256SUMS` — a release ships all four targets or none, so platforms never skew. [GitHub Releases](https://github.com/saintparish4/opal/releases) are the single canonical source of binaries; any future package-manager integration (Homebrew, npm wrapper, Docker) must fetch from there, never build independently.

Platform support: macOS, Linux, and WSL2 in v1 (WSL2 runs a genuine Linux kernel, so the Linux build target covers it directly). Native Windows (non-WSL) is v2, requiring junction-based fallbacks for linking and path-separator abstraction throughout `opal-core`.

## License

MIT — see [LICENSE](./LICENSE).
