# Opal — Development Build Guide

## 1. Prerequisites

- Rust (stable toolchain via `rustup`)
- A C/C++ toolchain (required for building/linking V8) — `build-essential` on Linux, Xcode Command Line Tools on macOS
- `git` and standard build tools (`cmake`, `ninja` recommended for V8-related builds)
- Target platforms for local development: macOS, Linux, or WSL2 on Windows

## 2. Repository Layout (proposed)

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
│   ├── prd.md
│   └── build_guide.md
└── Cargo.toml           # workspace root
```

Each subsystem is its own crate in a Cargo workspace, with `opal-core` as a shared dependency of the rest. `opal-cli` is the thin binary that ties them together as subcommands (`opal install`, `opal run`, `opal build`, `opal test`).

## 3. Build Order (recommended sequencing)

Build in this order — each phase is a prerequisite for the next, and each is independently testable.

### Phase 0 — `opal-core`
1. BLAKE3-based content hashing utilities (wrap the `blake3` crate).
2. Content-addressed store (CAS): on-disk layout, write/read by hash, basic garbage collection. **Writes must be atomic**: write to a temp file, verify the BLAKE3 hash, then rename into place at the hash-keyed path — never write directly to the final path. This makes a killed/crashed write leave orphaned temp-file garbage rather than a corrupt CAS entry, and is far cheaper to build in now than to retrofit once Phase 1+ depend on the CAS being trustworthy.
3. Module graph data structure: nodes (files) and edges (import/require relationships).
4. Resolver: given an entry file, walk imports and build the graph. Start with ESM `import`/CJS `require` parsing, using the `oxc` Rust JS parser for performance (preferred over `swc`).
5. Memoization layer: cache computed graph outputs keyed by input hash, invalidate on hash mismatch (never mtime).
6. Path abstraction layer: all path handling goes through a single utility module, even though v1 only targets Unix-like path semantics — this is what makes v2 Windows support cheap later.

**Exit criteria for Phase 0**: can resolve a real-world project's dependency graph, cache the result, and demonstrate cache hits on unchanged input. Additionally: a SIGKILL fault-injected mid-CAS-write never leaves a corrupt entry, verified by re-running to completion and checking every CAS entry's content matches its hash key.

### Phase 1 — `opal-pm` (package manager)
1. npm registry client (fetch package metadata, tarballs).
2. Dependency resolution (semver range solving).
3. Populate CAS with package contents on install.
4. Hardlink packages from CAS into project `node_modules` (or a flat alternative layout, if diverging from node_modules — decide explicitly, don't default silently). **Implement as a reconciler, not an imperative sequence**: diff `opal.lock` (target state) against what's actually on disk, then create/remove only the delta. This makes the step naturally idempotent — a run killed partway through just leaves a partial delta for the next run to finish, with no special-cased resume logic needed.
5. Lockfile read/write. **Writes must be atomic**: write to `opal.lock.tmp`, fsync, then rename over `opal.lock`. A crash mid-resolution leaves the previous valid lockfile untouched rather than a torn file.
6. Per-project install lock: an flock (e.g. on `node_modules/.opal-lock`) held for the duration of `opal install`, so two concurrent invocations against the same project serialize instead of racing. flock releases automatically on crash/kill, so no stale-lock detection is needed.

**Exit criteria for Phase 1**: `opal install` against a real `package.json` produces a working `node_modules` that Node itself can run against (this is your compatibility check). Additionally: SIGKILL fault-injected at randomized points across the install pipeline (mid-download, mid-verify, mid-rename, mid-link, mid-lockfile-write), followed by re-running `opal install` to completion, always converges to the same `node_modules` + `opal.lock` state as an uninterrupted install — see `testing_strategy.md` §8 for the full test design.

### Phase 2 — `opal-runtime`
1. Embed V8 via the `v8` crate — minimal "hello world" JS execution first.
2. Wire the runtime's module loader to `opal-core`'s resolver.
3. Lazy module instantiation.
4. TypeScript strip-types transpilation on load.

**Exit criteria for Phase 2**: `opal run` executes a real project's entrypoint, including its `node_modules` dependencies resolved via Phase 1.

### Phase 3 — `opal-bundler`
1. Consume the resolved graph from `opal-core`.
2. Tree-shaking pass.
3. Minification pass.
4. Output caching in the CAS.

### Phase 4 — `opal-test`
1. Test file discovery via the graph.
2. Assertion library (or adopt an existing minimal one).
3. Reporter (console output, pass/fail summary).
4. Wire into the runtime's execution path from Phase 2.

## 4. Testing Strategy

- **Unit tests** per crate, standard Rust `#[test]`.
- **Property-based tests** for the module graph engine (e.g., via `proptest`) — resolution and caching logic is exactly the kind of code where edge cases hide.
- **Compatibility test suite**: a curated list of real-world popular npm packages, installed and run end-to-end, checked into CI. This is the main defense against silent npm-compatibility regressions.
- **Crash-safety / fault-injection tests**: SIGKILL the install pipeline at randomized points and assert re-running converges to the same correct state as an uninterrupted run — in scope for v1, see `testing_strategy.md` §8.
- **Benchmark suite**: cold install, cold start, and bundle time, tracked over time against the npm/Node/esbuild baseline (see PRD §7 for target metrics).

## 5. CI Targets (v1)

- macOS (latest) — builds `opal-macos-x64` and `opal-macos-arm64`
- Linux (latest LTS) — builds `opal-linux-x64` and `opal-linux-arm64`; WSL2 is covered by this target, no separate job needed, but worth a manual smoke test before releases
- Windows (latest) — builds `opal-windows-x64.exe` as a native release artifact

CI is GitHub Actions + `cargo`, matrix-building all five v0.1.0 targets (linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64) on every tagged release. This is a standard Rust CI pattern and shouldn't require bespoke tooling.

## 6. Release Process

1. Tag a release (e.g. `v0.1.0`).
2. CI builds all five platform targets in release mode.
3. Each binary is packaged as a platform-appropriate archive: `opal-linux-x64.tar.gz`, `opal-linux-arm64.tar.gz`, `opal-darwin-x64.tar.gz`, `opal-darwin-arm64.tar.gz`, `opal-windows-x64.zip`.
4. A `SHA256SUMS` file is generated across all artifacts for integrity verification.
5. All artifacts are published to GitHub Releases — the canonical, single source of truth for distribution. Any future downstream package manager (Homebrew, Scoop, WinGet, npm wrapper, Docker) must fetch from here rather than building independently.
6. The install script (`https://opal.dev/install.sh`, domain TBD) and installer endpoint (`https://opal.dev/install`) are updated to point at the new release.

**Do not** document or recommend `cargo install opal` in any user-facing install instructions — it requires a Rust toolchain and is contributor-only.

## 7. Open Items to Resolve Before/During Phase 0

- Exact on-disk CAS layout (directory sharding scheme for hash-named files, to avoid huge flat directories).
- Whether `node_modules` compatibility is a hard requirement or whether a flat alternative layout is acceptable for v1 (affects Phase 1 scope significantly).
- Fallback behavior for the install script on unsupported OS/architecture combinations (hard error with a link to build-from-source, or another approach — currently undecided).