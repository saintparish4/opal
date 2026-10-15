---
title: "Opal v0.2.0"
description: "Correctness, speed, and visibility across the package manager. Four bugs in this release produced a wrong node_modules without ever failing an install."
date: 2026-08-29T18:28:07Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.2.0"
lede: "Correctness, speed, and visibility across the package manager. Four bugs in this release produced a wrong `node_modules` without ever failing an install."
summary: "Correctness, speed, and visibility across the package manager. Four bugs fixed here produced a wrong `node_modules` without ever failing an install. `opal.lock` is now v3."
commands:
  - title: "To install Opal v0.2.0"
    id: post-install
    command: "curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.2.0 bash"
---

## Breaking {#breaking}

**`opal.lock` is now v3.** A v0.1.0 binary refuses a v3 lockfile rather than misreading it, so upgrade before sharing a lockfile with one. Going the other way, v0.2.0 replaces a v1 or v2 lockfile by re-resolving — except under `--frozen-lockfile`, where it is an error, because a build that promised not to rewrite the lockfile must not rewrite it to upgrade it either.

**A required dependency this platform cannot run is now `EBADPLATFORM`**, matching npm. Previously it installed and produced a tree that could not run. A package declared in `optionalDependencies` is still skipped, which is how `jest`, `vite` and anything else depending on `fsevents` keeps working.

The graph cache misses once after upgrading: the resolver learned to walk files it used to skip, so records written by v0.1.0 no longer describe what it would produce.

## Fixed — installs that were silently wrong {#fixed-installs-that-were-silently-wrong}

- **A root dependency could be linked at the wrong version.** With `shared@^1` declared alongside a dependency on `shared@^2`, the tree got 2.x at the top level and the version you asked for was downloaded and never placed.
- **`--production` overwrote `opal.lock`**, and `--production --frozen-lockfile` — the ordinary CI invocation — failed against a perfectly valid lockfile every time.
- **Platform variants of native optionals all installed.** `esbuild` declares 25, totalling 256 MB; one 9.8 MB binary is what belongs on a host. They stay recorded in `opal.lock`, so one committed file still installs the right binary on every platform.
- **A dependency could write lines into your lockfile.** `Range::parse(">=1\npkg …")` kept the newline and the lockfile is one fact per line, so a dependency's own `package.json` could append entries — including package entries with attacker-controlled tarball URLs — to the lockfile of every project installing it. Found by fuzzing.

## Faster {#faster}

- **Registry metadata is cached across runs.** Re-resolving a 74-package tree against a warm store went from **7.6s to 0.36s**, making no round trips at all. `--offline` and `--prefer-offline` are new.
- Packuments are fetched in npm's abbreviated form, roughly half the bytes.
- The CAS hashes before writing, so content it already holds costs a hash and a `stat` instead of a write, an fsync, a read-back and a rename.

## New {#new}

- **`npm:` alias specifiers.** `"string-width-cjs": "npm:string-width@^4.2.0"` installs one package under another's name — how a package depends on two majors of one dependency at once. Unblocks `glob@10`, `rimraf@5`, `node-gyp@10` and `sucrase`, which could not be installed at all.
- **Progress output.** A spinner while resolving and linking, a bar advancing per package while fetching, and one warning per deprecated package. Plain lines when stderr is not a terminal, so CI logs stay readable.
- **`opal cache gc` prunes what it can no longer use** — graph records for deleted projects, and registry metadata past 30 days.
- Bounded retries and explicit timeouts on registry requests; ceilings on what a tarball may unpack to.

## Install {#install}

```
curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash
```

macOS and Linux, x64 and arm64. WSL2 is covered by the Linux builds. Native Windows remains a v2 target.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.1.0...v0.2.0](https://github.com/saintparish4/opal/compare/v0.1.0...v0.2.0)
