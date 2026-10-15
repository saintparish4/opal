---
title: "Opal v0.4.1"
description: "A faster opal add over old metadata, release notes taken from the changelog, and new checks that what Opal installs builds and runs. No opal.lock format change."
date: 2026-10-15T21:03:44Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.4.1"
lede: "A faster `opal add` over old metadata, release notes taken from the changelog, and new checks that what Opal installs builds and runs. No `opal.lock` format change."
summary: "`opal add` over day-old metadata drops from 3.0s to 0.56s on a Next.js app, `opal --revision` names the commit a build came from, and seven framework starters are installed, built, and served by Opal every night."
commands:
  - title: "To install Opal"
    id: post-install
    command: "curl -fsSL https://opalx.dev/install.sh | bash"
  - title: "To upgrade Opal"
    id: post-upgrade
    command: "opal upgrade"
---

## What's new {#added}

- **`opal --revision`** prints the version and the commit a build came from, as in `opal 0.4.1 (1a2b3c4)`. `opal --version` still prints the version alone, because `opal upgrade` in 0.3.1 and 0.4.0 checks for exactly that.
- A framework compatibility matrix, [compat/COMPATIBILITY.md](https://github.com/saintparish4/opal/blob/master/compat/COMPATIBILITY.md): the Next.js, Vite (React and Vue), Astro, SvelteKit, Nest, and Express starters, each installed by Opal, then built, tested, and served. It runs every night against `master`.
- Benchmarks of several installs sharing one store at once, and of how memory grows with package count, in [benchmarks/BENCHMARKS.md](https://github.com/saintparish4/opal/blob/master/benchmarks/BENCHMARKS.md).

## Changed {#changed}

- **`opal add` and `opal remove` no longer re-check packages the lockfile already holds.** Their cached metadata is used however old, since a published version never changes; anything the change needs a version chosen for is still checked against the registry. On a Next.js app with metadata a day old, an add took 3.0s with 0.4.0 and takes 0.56s now (median of five rounds each, one machine), about what it takes with fresh metadata. When no metadata is cached at all, as on a machine that installed from a lockfile, an add still takes 5–7s on that app.
- A version withdrawn from the registry after its metadata was cached is noticed when that metadata is next refreshed, not on the next add.

## Fixed {#fixed}

- The files in the release archives are owned by root, not by uid 1001. `install.sh` run as root, as in a Dockerfile, kept that owner on `~/.opal/bin/opal`.
- A GitHub release's notes are this changelog's section for that version. 0.3.1's and 0.4.0's were a list of Dependabot pull requests.

## Known issues {#known-issues}

- A damaged entry in the package metadata cache makes every `opal add` that keeps that package fail, and rechecking it with the registry doesn't repair it. Deleting the `packuments` folder inside the cache (`opal cache path` shows where) does.
- `opal cache verify` doesn't notice a file deleted from the shared store, and the next project that needs that file fails to link. Deleting the cache folder clears it.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.4.0...v0.4.1](https://github.com/saintparish4/opal/compare/v0.4.0...v0.4.1)
