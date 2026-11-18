---
title: "Opal v0.4.2"
description: "A security release. opal install no longer accepts a dependency whose name is a path, and a package's bin entry can no longer point outside the package. Upgrade from every earlier release."
date: 2026-11-18T01:21:11Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.4.2"
lede: "A security release. `opal install` no longer accepts a dependency whose name is a path, and a package's `bin` entry can no longer point outside the package."
summary: "A `package.json` or `opal.lock` naming a dependency `../../x` made Opal delete that directory and write a package there. Every release up to 0.4.1 is affected: upgrade with `opal upgrade`."
commands:
  - title: "To install Opal"
    id: post-install
    command: "curl -fsSL https://opalx.dev/install.sh | bash"
  - title: "To upgrade Opal"
    id: post-upgrade
    command: "opal upgrade"
---

## Security {#security}

- `opal install` no longer accepts a dependency whose name is a path. A `package.json` or `opal.lock` naming a dependency `../../x` made Opal delete that directory and write a package there. Names are now checked in the resolver, the lockfile parser and the linker, and the install fails with the name.
- A package's `bin` entry can no longer point outside the package. Such an entry is skipped.

Affected: every release up to 0.4.1. Upgrade with `opal upgrade`.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.4.1...v0.4.2](https://github.com/saintparish4/opal/compare/v0.4.1...v0.4.2)
