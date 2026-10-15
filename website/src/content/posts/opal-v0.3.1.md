---
title: "Opal v0.3.1"
description: "The launch release: cleaner terminal output, a new download bar, and opal upgrade. Resolution, downloads, and linking are unchanged, so the benchmarks measured on v0.3.0 still hold: warm and no-op installs timed the same on both builds."
date: 2026-09-27T02:10:13Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.3.1"
lede: "The launch release: cleaner terminal output, a new download bar, and `opal upgrade`. Resolution, downloads, and linking are unchanged, so the benchmarks measured on v0.3.0 still hold: warm and no-op installs timed the same on both builds."
summary: "Reworks what an install prints, adds `opal upgrade` to replace the binary in place, and says plainly when `opal.lock` is missing. The install path itself is unchanged from v0.3.0."
commands:
  - title: "To install Opal v0.3.1"
    id: post-install
    command: "curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.3.1 bash"
---

## What's new {#what-s-new}

- **A thin download bar in opal colours.** No line fills the terminal's last column, the suspected cause of a duplicate bar seen once in Windows Terminal.
- **`opal upgrade`** installs the latest release, or a named one, in place. The download is checked against `SHA256SUMS` and run once before it replaces anything.
- **A one-line summary**, plus one more line only when it says something: what a re-run kept, or that the shared store supplied everything. A Next.js install now ends in five lines instead of about seventy-five.
- **No stale lines between stages.** A terminal install no longer leaves "Resolving dependencies" and "Installing N packages" on screen.
- **Packages built for other platforms print as one line** instead of one each: 66 on a Next.js app.
- **Warnings print after the result**, so they are the last thing on screen.
- **`--frozen-lockfile` without an `opal.lock` says so**, and how to create one.
- **`--cache-dir` is described in `--help`.**

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.3.0...v0.3.1](https://github.com/saintparish4/opal/compare/v0.3.0...v0.3.1)
