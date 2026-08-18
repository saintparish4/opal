# Opal Testing Strategy

This is organized by **risk category**, not by pyramid layer. For Opal, "unit vs
integration vs e2e" is less useful than asking: *where does this system actually
break, and what does a bug look like when it does?* Each section below maps to a
real failure mode in the architecture, in rough priority order.

---

## 1. Pure logic (`#[test]` per crate)

Fast, in-process, no I/O. Standard Rust unit tests, prioritized by blast radius:

- **Semver range solving (`opal-pm`)** — the highest-value target for
  `proptest`. Generate random constraint sets, assert the solver's output
  satisfies every constraint, and cross-check resolution output against real
  npm's resolution for the same `package.json` fixtures. A silent wrong-version
  install is much worse than a crash.
- **Path abstraction layer** — test it as if Windows already exists, even
  though v1 only targets Unix/WSL2. Feed it mixed separators, case collisions,
  and other inputs that would be pathological on Windows. Cheap now, expensive
  to retrofit once v2 lands.
- **Content hashing / CAS key derivation** — deterministic by construction.
  Property tests: same input → same hash; identical content with different
  mtimes → same hash.

## 2. Cache / graph invalidation correctness

Its own category, not "integration" — because a bug here doesn't crash, it
**silently serves stale output**. That's a trust failure, not a test failure,
and it surfaces long after the code that caused it.

- **Invalidation matrix tests**: for every mutation type (file content change,
  file added/removed, direct dependency bump, transitive dependency change),
  assert cache hit/miss behavior explicitly. Don't just test the happy path of
  cache hits — test that the right things *miss*.
- **"Never mtime" invariant**: touch a file's mtime without changing content,
  assert no invalidation occurs. Easy to accidentally regress if someone
  "optimizes" the hot hashing path later — guard it directly.

## 3. Cross-crate contracts

All four tools (`opal-pm`, `opal-runtime`, `opal-bundler`, `opal-test`) consume
`opal-core`'s resolved module graph. The real risk isn't a broken call — it's
**contract drift** between what the graph engine produces and what each
downstream tool assumes.

- **Golden/snapshot tests** of `opal-core`'s resolved graph output for a fixed
  set of representative projects, checked into the repo. A shape change should
  visibly break every consumer's tests, not silently get absorbed.
- **Shared-cache cross-tool test**: run two different tools (e.g. test runner,
  then bundler) against the same project and assert the second hits the shared
  memoization cache. No single crate's unit tests will catch this — it only
  exists at the seam.

## 4. V8 embedding boundary

A distinct risk class: memory safety and behavioral drift at an FFI boundary
into an engine you don't control.

- Dedicated suite exercising the embedding surface directly — module
  instantiation, error propagation across the Rust↔V8 boundary, GC pressure
  under repeated module loads. Separate from "does my JS run correctly," which
  belongs to the compatibility suite below.
- Pin the V8 version explicitly; re-run this suite on every bump. This is the
  one dependency where "just update it" carries real risk of silent behavioral
  change.

## 5. npm compatibility suite (the real "E2E" layer)

This is the actual product bet, and it deserves curation, not just breadth.

- Curate the package list by **what edge case it exercises**, not just
  download count: `exports` map edge cases, native addons (best-effort),
  circular deps, deep transitive trees, unusual `package.json` shapes
  (multiple entry points, conditional exports). A "top 50 by downloads" list
  skews toward well-behaved packages and misses exactly what breaks real
  installs.
- Run **install** and **execute** as separately gate-able CI jobs. A package
  that installs but fails to run is a different bug class (runtime/resolver)
  than one that fails to install (registry/resolution) — conflating them into
  one pass/fail slows triage.

## 6. Fuzzing

Anywhere Opal parses untrusted, real-world-messy input — `package.json`,
lockfiles, tarball contents, JS/TS source via the resolver — is fuzzing
territory, not just unit-test territory. Real npm packages *will* contain
malformed metadata. `cargo-fuzz` on these parser boundaries is cheap insurance
against the worst failure mode for a package manager: a panic on install.

## 7. Benchmarks (tracked, not gated on noise)

Cold install, cold start, and bundle time, tracked against the npm/Node/esbuild
baseline. Track every PR's numbers, but only hard-fail CI on regressions past a
noise threshold — benchmarks are inherently noisier than correctness tests, and
a flaky perf gate trains people to ignore it.

---

## 8. Crash-safety / resumability

**Decision: in scope for v1.** Not because it needs a dedicated subsystem —
because designing every write as atomic and every apply step as idempotent
from Phase 0/1 onward makes "resume" free (just re-run `opal install`),
whereas retrofitting it after Phase 0/1 ship with naive writes means
auditing every write path across `opal-core` and `opal-pm` after the fact.
No WAL, two-phase commit across CAS/lockfile/node_modules, or
resume-from-exact-point tracking is needed — content-addressing plus atomic
rename plus idempotent reconciliation gets full crash safety without that
machinery.

- **Atomic CAS writes**: temp file → verify BLAKE3 hash → atomic rename into
  place. A kill mid-write leaves an orphaned temp file, never a CAS entry
  with mismatched content. Test: fault-inject a kill between temp-write and
  rename, assert the CAS never contains an entry whose content doesn't hash
  to its key.
- **Atomic lockfile writes**: `opal.lock.tmp` → fsync → rename over
  `opal.lock`. Test: fault-inject a kill mid-resolution, assert `opal.lock`
  is always parseable and equals either the pre- or post-resolution state,
  never a torn write.
- **Reconciling linker, not an imperative one**: the `node_modules` link
  step diffs `opal.lock` (target state) against disk (actual state) and only
  creates/removes the delta — it never assumes a prior run of itself
  completed. Test: fault-inject a kill at a random hardlink index N of M,
  assert a second `opal install` run converges to the same end state as an
  uninterrupted run.
- **Convergence test (the actual chaos test)**: SIGKILL the install pipeline
  at randomized points (mid-download, mid-verify, mid-rename, mid-link)
  across many trials; after each kill, re-run install to completion and diff
  the resulting `node_modules` + `opal.lock` against a clean uninterrupted
  install of the same project. Any divergence is a bug regardless of which
  stage was interrupted.
- **Concurrency, adjacent but distinct**: a per-project flock (e.g. on
  `node_modules/.opal-lock`) serializes concurrent `opal install`
  invocations against the same project. flock releases automatically on
  crash/kill, so no stale-lock detection logic is needed. Test: two
  processes racing `opal install` on the same project — the second should
  block or fail cleanly, never interleave writes with the first.