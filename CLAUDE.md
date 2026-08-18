# General Naming Conventions

In general, Rust tends to use UpperCamelCase for “type-level” constructs (types and traits) and snake_case for “value-level” constructs. More precisely, the proposed (and mostly followed) conventions are:

| Item | Convention |
| --- | --- |
| Crates | snake_case (but prefer single word) |
| Modules | snake_case |
| Types | UpperCamelCase |
| Traits | UpperCamelCase |
| Enum variants | UpperCamelCase |
| Functions | snake_case |
| Methods | snake_case |
| General constructors | `new` or `with_more_details` |
| Conversion constructors | `from_some_other_type` |
| Local variables | snake_case |
| Static variables | SCREAMING_SNAKE_CASE |
| Constant variables | SCREAMING_SNAKE_CASE |
| Type parameters | concise UpperCamelCase, usually single uppercase letter: `T` |
| Lifetimes | short, lowercase: `'a` |

## Fine points

- In UpperCamelCase, acronyms count as one word: use `Uuid` rather than `UUID`. In snake_case, acronyms are lower-cased: `is_xid_start`.
- In UpperCamelCase names multiple numbers can be separated by a `_` for clarity: `Windows10_1709` instead of `Windows101709`.
- In snake_case or SCREAMING_SNAKE_CASE, a “word” should never consist of a single letter unless it is the last “word”. So, we have `btree_map` rather than `b_tree_map`, but `PI_2` rather than `PI2`.

## `unwrap`, `into_foo` and `into_inner`

There has been a long running debate about the name of the `unwrap` method found in `Option` and `Result`, but also a few other standard library types. Part of the problem is that for some types (e.g. `BufferedReader`), `unwrap` will never panic; but for `Option` and `Result` calling `unwrap` is akin to asserting that the value is `Some`/`Ok`.

There’s basic agreement that we should have an unambiguous term for the `Option`/`Result` version of `unwrap`. Proposals have included `assert`, `ensure`, `expect`, `unwrap_or_panic` and others. No clear consensus has emerged.

This RFC proposes a simple way out: continue to call the methods `unwrap` for `Option` and `Result`, and rename other uses of `unwrap` to follow conversion conventions. Whenever possible, these panic-free unwrapping operations should be `into_foo` for some concrete `foo`, but for generic types like `RefCell` the name `into_inner` will suffice. By convention, these `into_` methods cannot panic; and by (proposed) convention, `unwrap` should be reserved for an `into_inner` conversion that can.

# Package Layout

Cargo uses conventions for file placement to make it easy to dive into a new Cargo package:

```
.
├── Cargo.lock
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── main.rs
│   └── bin/
│       ├── named-executable.rs
│       ├── another-executable.rs
│       └── multi-file-executable/
│           ├── main.rs
│           └── some_module.rs
├── benches/
│   ├── large-input.rs
│   └── multi-file-bench/
│       ├── main.rs
│       └── bench_module.rs
├── examples/
│   ├── simple.rs
│   └── multi-file-example/
│       ├── main.rs
│       └── ex_module.rs
└── tests/
    ├── some-integration-tests.rs
    └── multi-file-test/
        ├── main.rs
        └── test_module.rs
```

- `Cargo.toml` and `Cargo.lock` are stored in the root of your package (package root).
- Source code goes in the `src` directory.
- The default library file is `src/lib.rs`.
- The default executable file is `src/main.rs`.
- Other executables can be placed in `src/bin/`.
- Benchmarks go in the `benches` directory.
- Examples go in the `examples` directory.
- Integration tests go in the `tests` directory.
- If a binary, example, bench, or integration test consists of multiple source files, place a `main.rs` file along with the extra modules within a subdirectory of the `src/bin`, `examples`, `benches`, or `tests` directory. The name of the executable will be the directory name.

# Formatting and Linting

Use `cargo fmt` and `cargo clippy` to format and lint this project. `rustfmt` and `clippy` are installed via `rust-toolchain.toml`.

- Format with `cargo fmt`.
- Lint with `cargo clippy`. Treat Clippy warnings as errors: `cargo clippy -- -D warnings`.
- Run both before considering a change complete. Do not hand-format against rustfmt defaults or leave Clippy warnings unaddressed.

# Testing Strategy

Tests are organized by **risk category**, not by unit/integration/e2e pyramid. Ask: *where does this system actually break, and what does a bug look like when it does?* Full design lives in `base/testing_strategy.md`. Follow that document when adding tests; the categories below are the required coverage, in priority order.

## 1. Pure logic (`#[test]` per crate)

Fast, in-process, no I/O. Prioritize by blast radius:

- **Semver range solving (`opal-pm`)**: highest-value `proptest` target. Random constraint sets must produce a solution that satisfies every constraint; cross-check against real npm on the same `package.json` fixtures. A silent wrong-version install is worse than a crash.
- **Path abstraction layer**: test as if Windows already exists (mixed separators, case collisions), even though v1 is Unix/WSL2 only.
- **Content hashing / CAS key derivation**: same input → same hash; identical content with different mtimes → same hash.

## 2. Cache / graph invalidation

A bug here does not crash — it **silently serves stale output**.

- Invalidation matrix: for file content change, add/remove, direct dependency bump, and transitive dependency change, assert the right cache hits *and* misses.
- **Never mtime**: touching mtime without content change must not invalidate.

## 3. Cross-crate contracts

`opal-pm`, `opal-runtime`, `opal-bundler`, and `opal-test` all consume `opal-core`'s resolved graph.

- Golden/snapshot tests of resolved graph output for a fixed fixture set, checked into the repo.
- Shared-cache cross-tool test: two tools against the same project; the second must hit the shared memoization cache.

## 4. V8 embedding boundary

Dedicated suite for the FFI surface (module instantiation, error propagation, GC pressure under repeated loads). Separate from JS compatibility. Pin the V8 version; re-run this suite on every bump.

## 5. npm compatibility suite

Curate packages by the edge case they exercise (`exports` maps, native addons, circular deps, deep trees, unusual `package.json` shapes) — not by download count. Gate **install** and **execute** as separate CI jobs.

## 6. Fuzzing

`cargo-fuzz` on every parser of untrusted input: `package.json`, lockfiles, tarball contents, JS/TS source via the resolver. A panic on install is the worst failure mode for a package manager.

## 7. Benchmarks

Cold install, cold start, and bundle time vs. npm/Node/esbuild. Track every PR; hard-fail CI only past a noise threshold. Do not add a flaky perf gate.

## 8. Crash-safety / resumability (in scope for v1)

Every write is atomic; every apply step is idempotent. Resume is re-running `opal install`. No WAL or two-phase commit.

- Atomic CAS writes: temp file → verify BLAKE3 → rename. Kill mid-write must never leave a CAS entry whose content does not match its key.
- Atomic lockfile writes: `opal.lock.tmp` → fsync → rename. Kill mid-resolution: `opal.lock` is always parseable and equals the pre- or post-resolution state.
- Reconciling linker: diff `opal.lock` against disk; never assume a prior run finished. Kill at random hardlink N of M; a second `opal install` must converge.
- Convergence (chaos) test: SIGKILL at randomized pipeline points; re-run to completion; diff `node_modules` + `opal.lock` against a clean install.
- Concurrency: per-project flock serializes concurrent `opal install`. Two racing processes must block or fail cleanly, never interleave writes.

# Comments

Do not narrate the implementation. Do not add comments that restate what the code does. Do not generate comments for every function, variable, loop, or block. Do not add comments solely to make generated code appear documented. When reviewing AI-generated code, delete unnecessary comments rather than keeping them because they look helpful.

Comments must explain **why**, not **what**, unless the what is genuinely difficult to understand.

Only add comments when they explain:

- Why a decision was made, or why something is implemented a certain way
- Non-obvious constraints, invariants, or design decisions
- Algorithmic reasoning
- Performance considerations
- Safety requirements
- External or system constraints
- Compatibility requirements or workarounds
- Important architectural decisions
- Behavior that would otherwise be surprising

Make the code obvious instead: good names, clear control flow, small functions, strong types, clear abstractions, tests.

- Use tests to document behavior (`test_cache_hit`, `test_cache_miss`, `test_incremental_invalidation`).

# Commit Messages

Format: `<type>: <subject>`

| Type | Purpose |
|------|---------|
| `feat` | New feature |
| `fix` | Bug fix |
| `perf` | Performance improvement — must cite the benchmark, per the "optimize without evidence" rule above |
| `refactor` | Code refactor, no behavior change |
| `test` | Testing additions/changes |
| `docs` | Documentation |
| `chore` | Build/tooling changes |
| `types` | Type definition changes |

- **subject** — imperative, present tense, no trailing period (e.g. `fix: dedupe concurrent downloads of the same sha256`).
- **Body** (optional) — explain *why*, same rule as code comments. Use `-` bullets to separate points, keep each one concise, and skip the body entirely when the subject already says everything.
