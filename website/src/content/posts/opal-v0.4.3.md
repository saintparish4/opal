---
title: "Opal v0.4.3"
description: "A security release. An opal.lock can no longer choose where a tarball comes from, the shared store is checked before a package is linked from it, downloads go over https only, and every release archive carries signed build provenance. Upgrade from every earlier release."
date: 2026-11-23T00:15:04Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.4.3"
lede: "A security release. An `opal.lock` can no longer choose where a tarball comes from, the shared store is checked before a package is linked from it, and downloads go over `https://` only."
summary: "An edited `opal.lock` line could install any bytes as any package, under `--frozen-lockfile` too. Every release up to 0.4.2 is affected: upgrade with `opal upgrade`."
commands:
  - title: "To install Opal"
    id: post-install
    command: "curl -fsSL https://opalx.dev/install.sh | bash"
  - title: "To upgrade Opal"
    id: post-upgrade
    command: "opal upgrade"
---

## Security {#security}

- **`opal.lock` can no longer choose where a tarball comes from.** A lockfile line carries a package's tarball URL and the integrity it is checked against, so an edited line could install any bytes as any package, under `--frozen-lockfile` too. Each package is now fetched only from where the registry keeps it, `<registry>/<name>/-/<name>-<version>.tgz` or the URL the registry itself lists for that version, and this is checked before the local store is used.
- **Downloads go over `https://` only** (or `file://` from a registry on this machine), redirects included, and a response stops at 256 MiB for a packument and 512 MiB for a tarball instead of being read without end.
- **The shared store is checked before a package is linked from it.** Its files are hardlinked into projects, so a file changed in place under one project's `node_modules` (as root, or after a `chmod`) was changed for every project installed after it. Every stored file is now sealed (read-only, with npm's fixed 1985 timestamp, which is what files under `node_modules` show), so a changed one is found with one `stat`, and its package is downloaded again with a warning. A store written by an earlier release is re-hashed once, on first use.
- **`opal install` no longer follows a symlink out of `node_modules`.** With `node_modules/<name>` linked to another project, as `npm link` does, packages inside that project were removed as stale; a symlinked scope directory was written through. Symlinks there are now left alone, or replaced when a package belongs at that path.
- **`opal x` asks before installing a package to run it**, when run at a terminal; only `y` goes ahead. `--yes` skips the question, and with no terminal on stdin, in a script or CI, there is none, as with `npx`.
- **`opal upgrade` reads releases from GitHub only.** A release build ignores `OPAL_INTERNAL_RELEASES_URL`, which let anything able to set an environment variable choose the binary an upgrade installs, with a `SHA256SUMS` from the same place.
- **The runtime no longer lets JavaScript free memory a native call is using.** A vectored read or write (`fs.readv`, `fs.writev` and their sync forms) reads every buffer of its array before using any, then holds each one's memory until the call ends, so an array element that is a getter freeing an earlier buffer can't make `readv` write into freed memory. That was a heap use-after-free on `master` before the asynchronous fs rewrite, found under AddressSanitizer, and never in a release; a nightly AddressSanitizer job now runs the attack. `Buffer`'s native operations take their numeric arguments only from primitives, for the same reason.
- **Release archives carry signed build provenance**, checked with `gh attestation verify <archive> --repo saintparish4/opal`, and the release workflow verifies it before publishing. Every action in CI is pinned to a commit hash, and a lint step refuses one that isn't.

## Breaking {#breaking}

- A registry configured as `http://` is refused.
- An `opal.lock` written against one registry is refused when installing from another. Delete it and install again.

No `opal.lock` format change. Affected: 0.4.2 and earlier. Upgrade with `opal upgrade`.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.4.2...v0.4.3](https://github.com/saintparish4/opal/compare/v0.4.2...v0.4.3)
