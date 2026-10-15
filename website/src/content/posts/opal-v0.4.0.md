---
title: "Opal v0.4.0"
description: "Parallel downloads, opal add and opal remove, and an install that shows what it is doing. No opal.lock format change."
date: 2026-10-06T03:29:33Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.4.0"
lede: "Parallel downloads, `opal add` and `opal remove`, and an install that shows what it is doing. No `opal.lock` format change."
summary: "Downloads 16 packages at a time, adds `opal add` and `opal remove`, and shows what an install is doing while it does it. The Linux binaries run on glibc 2.34 again."
commands:
  - title: "To install Opal v0.4.0"
    id: post-install
    command: "curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.4.0 bash"
---

## What's new {#added}

- **`opal add`, `opal remove` (`rm`, `uninstall`), and `opal install <pkg>`** change `package.json`, `opal.lock`, and `node_modules` in one step. `-D` and `-O` choose the dependency group, and `-E` saves the exact version. A version or range you typed is saved as typed; a bare name or a tag saves a `^` range on what was installed.
- `package.json` keeps its formatting when Opal edits it, and nothing is written unless the change resolves. `opal remove` of a name that isn't a dependency is an error, not a silent success.
- `opal install` opens by naming the build it came from: `opal install v0.4.0 (<commit>)`.
- Resolving shows a live `[settled/known]` package count on a terminal, and prints the final count once when piped.
- Downloads show one bar per tarball in flight, with its name and bytes of its total, under a line counting packages.
- The project's own dependencies that a run added are listed as `+ name@version` above the summary, five at most with the rest counted.

## Changed {#changed}

- **Packages download 16 at a time** instead of one at a time, and one package's files are stored on several threads. A first install of a 364-package Next.js app takes about 24s where v0.3.0 took about 2m on the same machine, in separate sessions. Resolution is still sequential, so the lockfile it writes is the same.
- **A re-resolve keeps what is locked.** When `package.json` changes, Opal starts from the existing `opal.lock`, so adding or removing one package no longer moves the rest of the tree to newer versions. This applies to a hand edit followed by `opal install` too. Delete `opal.lock` to re-resolve from nothing.
- The summary's total is in brackets, `364 packages installed [26.5s]`, and result lines are coloured on a terminal. Piped output has no colour and no stage lines: the header, the resolve count, and the result. A script that reads Opal's output should check it against the new format.
- The Linux binaries are built on Ubuntu 22.04 and need glibc 2.34 again, so they run on Ubuntu 22.04, Debian 12, and RHEL 9.
- Dependencies: rustls 0.23.45 for RUSTSEC-2026-0285, oxc 0.152.0, and patch updates for blake3, flate2, ureq, clap, thiserror, and console.

## Fixed {#fixed}

- `install.sh` no longer installs a binary that can't run on the machine. It runs the new binary once and stops, leaving any existing install in place, if it doesn't start.
- A killed install could leave a `write-<pid>-….tmp` file in the project folder, and nothing removed it. The two files Opal writes into a project now go through `opal.lock.tmp` and `package.json.opal-tmp`, which the next run clears. Files already left by an earlier version are not removed.

## Known issues {#known-issues}

- `opal add` and `opal remove` ask the registry about every package in the tree, not only the one being changed. On a Next.js app that is about half a second when the metadata was fetched in the last five minutes, about 2s when it is older, and 5–7s when none is cached, as on a machine that installed from a lockfile. A fix for the second case is planned for 0.4.1.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.3.1...v0.4.0](https://github.com/saintparish4/opal/compare/v0.3.1...v0.4.0)
