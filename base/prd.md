# Opal — Product Requirements Document

## 1. Summary

Opal is an all-in-one JavaScript/TypeScript toolkit combining a package manager, runtime, bundler, and test runner into a single binary, built around one shared incremental module graph engine rather than four independently-implemented resolvers.

The core bet: every JS toolchain today (npm/pnpm/yarn + Node/Bun/Deno + webpack/esbuild/vite + Jest/Vitest) re-solves the same problem — "given this file, what does it import, and how do I resolve that" — separately, in separate languages, with separate caches. Opal solves it once, natively, and lets every subsystem consume it.

## 2. Goals

- Ship a single binary that replaces: `npm`/`pnpm`, `node`, a bundler, and a test runner for the common case.
- Meaningfully faster cold installs and cold starts than the npm + Node + esbuild + Jest baseline, via a shared content-addressed cache.
- npm-compatible enough that existing `package.json` projects work with minimal or no migration.
- Run on macOS, Linux, and WSL2 in v1. Native Windows (non-WSL) is explicitly out of scope for v1.

## 3. Non-Goals (v1)

- Native Windows binary (no WSL) — deferred to v2.
- Private/scoped npm registries — public registry only in v1.
- Full Node.js native addon (node-gyp) compatibility — best-effort only.
- Monorepo/workspaces orchestration beyond basic support — not a v1 requirement unless it falls out naturally from the resolver.

## 4. Architecture Overview

### 4.1 Core stack
- **Language**: Rust for all native code.
- **JS Engine**: V8 (via the `v8` crate), chosen over JavaScriptCore for embedding maturity, JIT performance ceiling, and the amount of prior art available (Node, Deno).
- **Content addressing**: BLAKE3 for all content hashing — SIMD-accelerated, Rust-native, fast enough to be on the hot path for every file read, with cryptographic-strength collision resistance.

### 4.2 Shared Module Graph Engine (`opal-core`)
The foundational layer all four tools consume. Responsibilities:
- Parse and resolve import/require graphs for a project.
- Content-address every file and computed artifact by BLAKE3 hash.
- Maintain a content-addressed store (CAS) on disk, keyed by hash, for both source packages and computed outputs (transpiled files, bundled chunks, resolved dependency trees).
- Answer "what changed since last run" cheaply via hash comparison — never mtime, since mtimes are unreliable across git checkouts, CI runners, and Docker layers.
- Memoize expensive computations (resolution, transpilation, bundling) keyed by input hash, so repeated runs across tools (e.g., test runner and bundler both needing a resolved graph) hit cache instead of recomputing.

### 4.3 Package Manager (`opal install`)
Built on top of `opal-core`.
- Resolves dependencies against the public npm registry.
- Populates the CAS with package contents, keyed by BLAKE3 hash of package tarball contents (not just name+version — enables cross-package deduplication of identical files).
- Links packages into the project via hardlinks from the CAS (mirrors the pnpm model) to avoid redundant disk usage and redundant I/O.
- Lockfile format: `opal.lock` — a flat, fast-to-parse format (not deeply nested JSON) storing resolved versions and content hashes.
- Supports standard `package.json` semantics including the `exports` map.

#### 4.3.1 Install Pipeline

```
package.json
      ↓
registry metadata
      ↓
semver resolution
      ↓
dependency graph
      ↓
lockfile (opal.lock)
      ↓
download
      ↓
integrity verification
      ↓
global cache (CAS)
      ↓
node_modules
```

Each stage is a discrete, testable unit: registry metadata fetch and semver resolution together produce the dependency graph (built via `opal-core`); the graph is serialized to `opal.lock`; the lockfile then drives download, BLAKE3 integrity verification, population of the global CAS, and finally linking into `node_modules`. Re-runs with an unchanged `opal.lock` should skip straight from lockfile to linking, since download/verification/cache population are already satisfied by hash.

#### 4.3.2 Core Commands
- `opal add` — add packages to the project
- `opal remove` — remove dependencies from the project
- `opal update` — update dependencies to the newest versions their ranges allow
- `opal duplicate` — remove duplicate versions of packages from `opal.lock`
- `opal snip` — remove packages not present in `opal.lock` from `node_modules`
- `opalx` — run packages from npm, auto-installing if needed (Opal's equivalent of `npx`/`yarn dlx`)

#### 4.3.3 Publishing & Analysis Commands
- `opal publish` — packs the package into a tarball, strips catalog/workspace protocols from `package.json` (resolving versions where needed), and publishes to the registry set in configuration
- `opal outdated` — displays a table of dependencies with newer versions available
- `opal why <package>` — explains why a package is installed by showing the dependency chain that led to it
- `opal audit` — checks installed packages for known security vulnerabilities
- `opal info` — displays package metadata from the npm registry

### 4.4 Runtime (`opal run`)
- Executes JS/TS directly via the embedded V8 engine.
- Uses `opal-core`'s resolved module graph for import resolution — no separate resolver.
- Lazy module instantiation: modules are parsed/compiled on first use, not eagerly across the whole dependency tree, to minimize cold-start time.
- TypeScript support via on-the-fly transpilation (strip-types by default; full type-checking is explicitly out of scope for the runtime, matching the Bun/Deno model).

### 4.5 Bundler (`opal build`)
- Consumes the same module graph as the runtime.
- Adds tree-shaking and minification passes on top of the resolved graph.
- Outputs are cached in the CAS keyed by input hash, so unchanged subgraphs are never re-bundled.

### 4.6 Test Runner (`opal test`)
- Thinnest layer: reuses the runtime's module loading and execution, adds test discovery, assertion library, and a reporter.
- Test file discovery and per-file execution both go through the shared graph for resolution, so test runs benefit from the same incremental caching as everything else.

## 5. Platform Support

| Platform | v1 | v2 |
|---|---|---|
| macOS | Yes | — |
| Linux | Yes | — |
| WSL2 | Yes | — |
| Native Windows (no WSL) | No | Planned |

WSL2 runs a genuine Linux kernel, so v1's Linux build target covers it without additional path-handling or symlink-permission work. Native Windows support (v2) will require junction-based fallbacks for linking (since symlinks require elevated permissions or Developer Mode on Windows) and path-separator abstraction throughout `opal-core`.

## 6. Distribution & Shipping

### 5.1 Build & Packaging Model

Opal ships as a single native CLI binary with the package manager compiled directly in — no runtime dependency on a separate install step or interpreter.

```
opal source
    ↓
Rust compiler
    ↓
native executable
    ↓
opal
```

### 5.2 v0.1.0 Release Targets

```
opal-v0.1.0
├── opal-linux-x64
├── opal-linux-arm64
├── opal-macos-x64
├── opal-macos-arm64
└── opal-windows-x64.exe
```

Each is a fully self-contained executable — no external runtime or shared library dependencies required at install time.

### 5.3 Installation Flow

Primary install method is a curl-based install script (final domain/naming TBD — placeholder `opal.dev`):

```
curl -fsSL https://opal.dev/install.sh | bash
```

The installer:

```
Detect OS
     ↓
Detect CPU architecture
     ↓
Select binary
     ↓
Download Opal
     ↓
Install ~/.opal/bin/opal
     ↓
Add ~/.opal/bin to PATH
```

**Hard requirement**: `cargo install opal` must never be presented as the primary install path for end users — it requires a Rust toolchain, which end users should not need. It remains available only as a path for contributors building Opal itself from source.

### 5.4 Runtime Example — `opal add react`

Once installed, the binary contains all resolution/install logic natively — no additional downloads or interpreters needed to run a command:

```
package.json
     │
     ▼
Resolver
     │
     ├── React metadata
     ├── dependency metadata
     └── version constraints
     │
     ▼
Registry
     │
     ▼
Download tarballs
     │
     ▼
Cache
     │
     ▼
node_modules
     │
     ▼
opal.lock
```

(This is the same install pipeline described in §4.3.1, applied to a single-package add.)

### 5.5 Release Artifacts

Every release publishes to GitHub Releases:

```
v0.1.0
│
├── opal-linux-x64.tar.gz
├── opal-linux-arm64.tar.gz
├── opal-darwin-x64.tar.gz
├── opal-darwin-arm64.tar.gz
└── opal-windows-x64.zip
```

Plus a `SHA256SUMS` file for integrity verification of the release artifacts themselves (distinct from the BLAKE3 package-content verification in §4.3.1).

CI (GitHub Actions + `cargo`) builds all platform targets automatically per release — this is a well-trodden Rust CI pattern and shouldn't require custom tooling.

A single canonical installer endpoint (`https://opal.dev/install`, name TBD) detects OS, architecture, and desired version, then serves the matching release artifact. End users get a working `opal --version` with no Rust toolchain required.

### 5.6 Deferred: Package-Manager Integrations

Once Opal matures, add convenience installers through existing ecosystems, scoped to target audience:
- Homebrew (macOS/Linux)
- Scoop / WinGet (Windows)
- npm (as a thin wrapper/postinstall fetcher, for JS developers already in an npm-based workflow)
- Docker (for CI/containerized use)

**Important constraint**: these are convenience wrappers only. `brew install opal`, for example, should ultimately fetch and place the same canonical native binary — never build or distribute independently. GitHub release binaries remain the single source of truth; downstream package managers are distribution channels, not alternate build pipelines.

## 7. Key Risks

- **npm compatibility surface**: lockfile edge cases, `exports` map edge cases, and package.json quirks are likely to consume more engineering time than the native performance work. Should be derisked early with a compatibility test suite against real-world popular packages.
- **Native addon (node-gyp) compatibility**: packages depending on compiled native addons may not work out of the box; needs an explicit compatibility policy (best-effort, documented gaps) rather than silent failure.
- **V8 embedding complexity**: V8's embedding API is powerful but has a steep learning curve and breaking changes across versions; pin a version early and budget time for the FFI boundary.

## 8. Success Metrics (proposed — needs baseline benchmarking)

- Cold install time vs. npm/pnpm baseline on a representative mid-size project.
- Cold start (`opal run`) time vs. `node` baseline.
- Bundle build time vs. esbuild baseline for an unchanged/warm-cache run (should approach zero given the CAS).
- Percentage of top-N npm packages that install and run correctly with zero configuration.