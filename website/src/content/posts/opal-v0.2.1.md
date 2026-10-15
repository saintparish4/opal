---
title: "Opal v0.2.1"
description: "A performance fix for one finding: a 367-package Next.js install spent most of its time parsing package metadata it never used, and almost none of it on the network."
date: 2026-08-29T20:23:02Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.2.1"
lede: "A performance fix for one finding: a 367-package Next.js install spent most of its time parsing package metadata it never used, and almost none of it on the network."
summary: "A performance fix for one finding: a 367-package Next.js install spent most of its time parsing package metadata it never used. Version bodies are now parsed on demand, which took an offline install from 128.6s to 37.7s."
commands:
  - title: "To install Opal v0.2.1"
    id: post-install
    command: "curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.2.1 bash"
---

Every package on the registry publishes a record of every version it has ever released. Opal was building the full dependency detail for all of them before selecting one. On that project it parsed **130,246 published versions to choose 367**, then held the rest in memory and freed it again.

Version bodies are now kept as raw slices and parsed on demand.

## Measured {#measured}

On a 367-package Next.js tree with a warm store and warm metadata:

<div class="table-scroll"><table><thead><tr><th></th><th>before</th><th>after</th></tr></thead><tbody><tr><td>whole install, offline</td><td>128.6s</td><td><strong>37.7s</strong></td></tr><tr><td>the resolve phase alone</td><td>74.3s</td><td><strong>8.4s</strong></td></tr><tr><td>whole install, with network</td><td>188.9s</td><td><strong>60.0s</strong></td></tr><tr><td><code>--prefer-offline</code></td><td>—</td><td>26.0s</td></tr><tr><td>lockfile and tree already present</td><td>—</td><td>5.9s</td></tr></tbody></table></div>

## Also in this release {#also-in-this-release}

- **Per-phase timings.** `365 packages resolved in 587.1s` described the whole install, not resolution. It now reads `(resolve 43.2s, fetch 0.5s, link 13.5s)`, because those three phases are slow for unrelated reasons and one total hides which to fix.
- **Fewer syscalls when linking.** A 21,000 file tree made 21,000 `create_dir_all` calls where a few hundred would do. Invisible on ext4, and not on a DrvFS mount under WSL2.
- **The hardlink warning now names the remedy.** If your project and cache sit on different filesystems, every file is copied instead of linked, which can be most of an install's time. Set `OPAL_CACHE_DIR` to the same filesystem, or move the project off `/mnt/c` if you are on WSL2.

## Note {#note}

A version a registry lists but cannot be installed from — no tarball, or no usable integrity — is now skipped in favour of the next best match, rather than being filtered out when the packument was first read.

No lockfile format change. v0.2.0 lockfiles are read as-is.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.2.0...v0.2.1](https://github.com/saintparish4/opal/compare/v0.2.0...v0.2.1)
