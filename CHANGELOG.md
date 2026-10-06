# Changelog

What changed in each Opal release, newest first.

Opal is pre-1.0, so a release can change behaviour you depend on. Anything that does is under **Breaking**, and every release says whether it changes the `opal.lock` format. Dates are when the release was published. The numbers are measurements made at the time on one machine, and they compare a release with the one before it. Opal against npm, pnpm, yarn, and bun is in [benchmarks/BENCHMARKS.md](./benchmarks/BENCHMARKS.md).

## 0.4.0 (not yet released)

Parallel downloads, `opal add` and `opal remove`, and an install that shows what it is doing. No `opal.lock` format change.

### Added

- **`opal add`, `opal remove` (`rm`, `uninstall`), and `opal install <pkg>`** change `package.json`, `opal.lock`, and `node_modules` in one step. `-D` and `-O` choose the dependency group, and `-E` saves the exact version. A version or range you typed is saved as typed; a bare name or a tag saves a `^` range on what was installed.
- `package.json` keeps its formatting when Opal edits it, and nothing is written unless the change resolves. `opal remove` of a name that isn't a dependency is an error, not a silent success.
- `opal install` opens by naming the build it came from: `opal install v0.4.0 (<commit>)`.
- Resolving shows a live `[settled/known]` package count on a terminal, and prints the final count once when piped.
- Downloads show one bar per tarball in flight, with its name and bytes of its total, under a line counting packages.
- The project's own dependencies that a run added are listed as `+ name@version` above the summary, five at most with the rest counted.

### Changed

- **Packages download 16 at a time** instead of one at a time, and one package's files are stored on several threads. A first install of a 364-package Next.js app took about 29s where v0.3.1 took about 2m on the same machine, in separate sessions. Resolution is still sequential, so the lockfile it writes is the same.
- **A re-resolve keeps what is locked.** When `package.json` changes, Opal starts from the existing `opal.lock`, so adding or removing one package no longer moves the rest of the tree to newer versions. This applies to a hand edit followed by `opal install` too. Delete `opal.lock` to re-resolve from nothing.
- The summary's total is in brackets, `364 packages installed [26.5s]`, and result lines are coloured on a terminal. Piped output has no colour and no stage lines: the header, the resolve count, and the result. A script that reads Opal's output should check it against the new format.
- The Linux binaries are built on Ubuntu 22.04 and need glibc 2.34 again, so they run on Ubuntu 22.04, Debian 12, and RHEL 9.
- Dependencies: rustls 0.23.45 for RUSTSEC-2026-0285, oxc 0.152.0, and patch updates for blake3, flate2, ureq, clap, thiserror, and console.

### Fixed

- `install.sh` no longer installs a binary that can't run on the machine. It runs the new binary once and stops, leaving any existing install in place, if it doesn't start.
- A killed install could leave a `write-<pid>-….tmp` file in the project folder, and nothing removed it. The two files Opal writes into a project now go through `opal.lock.tmp` and `package.json.opal-tmp`, which the next run clears. Files already left by an earlier version are not removed.

### Known issues

- `opal add` and `opal remove` ask the registry about every package in the tree, not only the one being changed. On a Next.js app that is about half a second when the metadata was fetched in the last five minutes, about 2s when it is older, and 5–7s when none is cached, as on a machine that installed from a lockfile. A fix for the second case is planned for 0.4.1.

## [0.3.1] (2026-09-27)

Cleaner terminal output, a new download bar, and `opal upgrade`. Resolution, downloads, and linking are unchanged from 0.3.0. No `opal.lock` format change.

### Added

- **`opal upgrade`** installs the latest release, or a named one, in place. The download is checked against `SHA256SUMS` and run once before it replaces anything.

### Changed

- The download bar is a thin line in opal colours, and no line fills the terminal's last column.
- The summary is one line, plus one more only when it says something: what a re-run kept, or that the shared store supplied everything. A Next.js install ends in five lines instead of about seventy-five.
- A terminal install no longer leaves "Resolving dependencies" and "Installing N packages" on screen between stages.
- Packages built for other platforms print as one line instead of one each (66 on a Next.js app).
- Warnings print after the result, so they are the last thing on screen.
- `--frozen-lockfile` without an `opal.lock` says so, and how to create one.
- `--cache-dir` is described in `--help`.

### Known issues

- The Linux binaries need glibc 2.39, so they don't start on Ubuntu 22.04, Debian 12, or RHEL 9. Fixed in 0.4.0.

## [0.3.0] (2026-09-19)

Opal installs the versions npm would install. No `opal.lock` format change.

### Changed

- **Version choice follows npm's order**: the `latest` dist-tag when it satisfies the range and isn't deprecated, then the newest version that isn't deprecated, then the newest. It agrees with `npm-pick-manifest` on all 12,032 cases of a generated registry. On six edge-case fixtures the trees match npm's package for package, and on a 365-package Next.js scaffold 432 of npm's 433 package versions match.
- **Linking runs in parallel**, one `node_modules` depth at a time: about 2.7× faster than 0.2.1 on the same tree and machine.
- The graph cache misses once after upgrading, because of the `exports` fix below.

### Added

- An install names every package whose install scripts it didn't run, the project's own included.

### Fixed

- **`opal install --root .`, run again over an installed tree, deleted `../node_modules/<name>` outside the project** for every package it had installed. A project inside another project could lose the enclosing project's packages. The bug shipped in 0.2.1. A 0.2.2 with this fix was tagged and never published; 0.3.0 is the first release that carries it.
- A package's `exports` can no longer resolve to a file outside the package.
- The warning for a project and cache on different filesystems prints as one sentence.

### Tests

- Installs killed at random moments must converge when run again.
- Resolution is cross-checked against npm's on the same `package.json`.
- The resolver has a fuzz target and a property test.
- A benchmark job records numbers on every push and never fails the build.

## [0.2.1] (2026-08-29)

A performance fix for one finding: a 367-package Next.js install spent most of its time parsing package metadata it never used. No `opal.lock` format change.

### Changed

- **Version records are parsed on demand.** Opal was building the full dependency detail for every version of every package before selecting one: 130,246 published versions to choose 367. On that tree, with a warm store and warm metadata, an offline install went from 128.6s to 37.7s, the resolve phase from 74.3s to 8.4s, and an install with the network from 188.9s to 60.0s.
- The summary gives per-phase timings, `(resolve 43.2s, fetch 0.5s, link 13.5s)`, where it gave one total.
- Linking makes one `create_dir_all` call per directory, not one per file.
- The warning for a project and cache on different filesystems names the remedy: set `OPAL_CACHE_DIR` to the project's filesystem.
- A version the registry lists but that can't be installed (no tarball, or no usable integrity) is skipped in favour of the next best match.

## [0.2.0] (2026-08-29)

Correctness, speed, and visibility across the package manager. Four bugs fixed here produced a wrong `node_modules` without ever failing an install.

### Breaking

- **`opal.lock` is now v3.** A 0.1.0 binary refuses a v3 lockfile, so upgrade before sharing a lockfile with one. 0.2.0 replaces a v1 or v2 lockfile by re-resolving, except under `--frozen-lockfile`, where an older lockfile is an error.
- A required dependency this platform can't run is now `EBADPLATFORM`, matching npm. It used to install and produce a tree that couldn't run. A package in `optionalDependencies` is still skipped.
- The graph cache misses once after upgrading: the resolver walks files it used to skip.

### Fixed

- A root dependency could be linked at the wrong version. With `shared@^1` declared alongside a dependency on `shared@^2`, the tree got 2.x at the top level, and the version you asked for was downloaded and never placed.
- `--production` overwrote `opal.lock`, and `--production --frozen-lockfile` failed against a valid lockfile every time.
- Every platform variant of a native optional dependency installed. `esbuild` declares 25, totalling 256 MB, where one 9.8 MB binary belongs on a host. They stay recorded in `opal.lock`, so one committed file installs the right binary on every platform.
- **A dependency could write lines into your lockfile.** A range containing a newline was kept as written, and the lockfile is one fact per line, so a dependency's own `package.json` could append entries, including tarball URLs, to the lockfile of every project installing it. Found by fuzzing.

### Changed

- Registry metadata is cached across runs. Re-resolving a 74-package tree against a warm store went from 7.6s to 0.36s, with no round trips.
- Packuments are fetched in npm's abbreviated form, roughly half the bytes.
- The store hashes before writing, so content it already holds costs a hash and a `stat`, not a write.

### Added

- **`npm:` alias specifiers**, such as `"string-width-cjs": "npm:string-width@^4.2.0"`. This unblocks `glob@10`, `rimraf@5`, `node-gyp@10`, and `sucrase`, which could not be installed at all.
- `--offline` and `--prefer-offline`.
- Progress output: a spinner while resolving and linking, a bar while fetching, and one warning per deprecated package. Plain lines when stderr isn't a terminal.
- `opal cache gc` prunes graph records for deleted projects and registry metadata older than 30 days.
- Bounded retries and explicit timeouts on registry requests, and ceilings on what a tarball may unpack to.

## [0.1.0] (2026-08-22)

The first release.

### Added

- **`opal install`** resolves a `package.json` against the npm registry, writes `opal.lock`, and links `node_modules` from a content-addressed store shared by every project. `--production` skips devDependencies, and `--frozen-lockfile` fails when `opal.lock` doesn't match `package.json`.
- `opal graph` resolves a module graph from an entry file.
- `opal cache verify`, `opal cache gc`, and `opal cache path` inspect and prune the shared store.
- Prebuilt binaries for Linux and macOS, x64 and arm64, and an install script.

[0.3.1]: https://github.com/saintparish4/opal/releases/tag/v0.3.1
[0.3.0]: https://github.com/saintparish4/opal/releases/tag/v0.3.0
[0.2.1]: https://github.com/saintparish4/opal/releases/tag/v0.2.1
[0.2.0]: https://github.com/saintparish4/opal/releases/tag/v0.2.0
[0.1.0]: https://github.com/saintparish4/opal/releases/tag/v0.1.0
