<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/opal-horizontal-paper.svg">
    <img src=".github/assets/opal-horizontal-ink.svg" alt="Opal" width="400">
  </picture>
</p>

## What is Opal?

Opal is an all-in-one toolkit for JavaScript and TypeScript projects, built to cover package management, running code, bundling, and testing. It ships as a single executable called `opal`.

At its core is `opal-core`, a module graph engine written in Rust. It works out what every file imports and where each import resolves, then caches that answer by content hash, so the next run over unchanged files is a cache hit. Every tool Opal adds is meant to share that one graph instead of carrying its own resolver.

The first of those tools is the package manager, and it works today. It picks the versions npm would, with a few [documented differences](#limitations), lays out `node_modules` the way npm does, and links every file in from a single content-addressed store shared by all the projects on your machine.

```bash
opal install                     # install the dependencies in package.json
opal add express                 # add a dependency (also: opal install express)
opal add -D typescript@^5        # add a devDependency at a range you choose
opal remove express              # remove one
```

The `opal` command-line tool also lets you inspect a project's module graph and the shared store. Installs are crash-safe: if one is killed partway through, running `opal install` again finishes the job.

```bash
opal graph index.js --root .     # resolve everything index.js imports
opal cache verify                # re-hash the shared store and check it for corruption
opal cache gc                    # delete store files no project uses anymore
```

> **Status**: Beta. The package manager works today — `opal install` resolves against the real npm registry and produces a `node_modules` tree Node runs against, validated on real projects (a Next.js scaffold at 365 packages, express, webpack, and a curated compatibility suite), and it is under active development, so expect rough edges and breaking releases. The runtime (`opal run`), bundler (`opal build`), and test runner (`opal test`) are **not implemented**; their directories under `crates/` hold placeholder files only.

## Install

Opal supports Linux (x64 & arm64) and macOS (x64 & Apple Silicon). On Windows, it runs inside WSL2.

> **Linux users**: the prebuilt binaries need glibc 2.34 or newer (Ubuntu 22.04+, Debian 12+, Fedora 35+, RHEL 9+). Check yours with `ldd --version`. v0.3.1 is the exception: it needs glibc 2.39 (Ubuntu 24.04+, Debian 13+, Fedora 40+). On an older system, the install script stops without installing anything. Alpine and other musl-based distributions aren't supported.

> **Windows users**: install and run Opal inside WSL2. Native Windows support is planned.

```sh
# with install script (recommended)
curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash

# a specific version
curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.3.0 bash

# from source (prerequisites are in CONTRIBUTING.md)
git clone https://github.com/saintparish4/opal && cd opal
cargo build --release   # binary at ./target/release/opal
```

Installing with npm or Homebrew is planned. Until then, use the install script.

The script picks the binary for your OS and CPU from the [GitHub Release](https://github.com/saintparish4/opal/releases), verifies its SHA256 checksum, and installs it to `~/.opal/bin/opal`. It puts that directory on your `PATH` by adding one line, marked `# added by opal's install.sh`, to `~/.zshrc`, `~/.bashrc`, or `~/.profile`. To uninstall, delete the directory `opal cache path` prints, then `~/.opal` and that line.

Opal is pre-1.0 beta software, and the lockfile format can change between minor versions. Don't point it at a project you can't reinstall.

### Upgrade

To upgrade to the latest version of Opal, run:

```sh
opal upgrade
```

To switch to a specific version, older or newer, name it:

```sh
opal upgrade 0.3.1
```

It downloads the release for your platform from GitHub, checks it against the release's `SHA256SUMS`, and runs it once to make sure it starts before replacing the binary, so a failed upgrade leaves the old one untouched. Running the install script again works too.

If the new version changed the lockfile format, the next `opal install` in each project re-resolves `opal.lock` and says so. Commit the rewritten file, because `opal install --frozen-lockfile` refuses to rewrite it and CI fails until you do. An older Opal can't read a lockfile written by a newer one.

There's no canary channel; every release is a tagged [GitHub Release](https://github.com/saintparish4/opal/releases), and what changed in each is in [CHANGELOG.md](./CHANGELOG.md). To run unreleased changes from `master`, build from source.

## Usage

Opal has six commands today: `opal install`, `opal add`, `opal remove`, `opal graph`, `opal cache`, and `opal upgrade`. Run `opal <command> --help` to see their flags.

Commit `opal.lock`. In CI, run `opal install --frozen-lockfile`, which installs exactly what `opal.lock` records and fails instead of changing it.

### Adding and removing dependencies

`opal add <package>` writes the dependency to `package.json`, updates `opal.lock`, and installs it. `opal install <package>` does the same thing. `opal remove <package>` (or `opal rm`) takes it out of every dependency group and out of `node_modules`.

```bash
opal add express                 # the latest release, saved as a ^ range on it
opal add express@4.21.2          # a version you typed is saved as typed: 4.21.2
opal add 'express@^4.0.0'        # and so is a range: ^4.0.0
opal add -D vitest               # devDependencies (-O for optionalDependencies)
opal add -E zod                  # the exact version instead of a ^ range
opal add old-ms@npm:ms@^2.0.0    # an alias
```

- **Only what you name moves.** Every other package keeps the version in `opal.lock`, even when the registry has something newer. The same holds after you edit `package.json` by hand and run `opal install`.
- **A package you name gets what the registry has now**, and anything else that depends on it moves to that same version when its range allows, so you don't end up with two copies.
- **`package.json` keeps its formatting.** Indentation, line endings, key order, and everything outside the dependency group being edited are written back as they were. The edited group is sorted by name.
- **Nothing is written unless the change resolves.** A mistyped package name or a range nothing satisfies leaves `package.json` and `opal.lock` untouched. So does `opal remove` of a name that isn't a dependency, which is an error and not a silent success.
- **To re-resolve everything from scratch**, delete `opal.lock` and run `opal install`.

Not supported yet, and planned for v0.5.0: `--peer`, `--global`, `git:` and `file:` specifiers, and `opal add` in a directory that has no `package.json`.

`update`, `why`, `outdated`, `audit`, and `publish` aren't implemented yet, so the binary doesn't have them. A command that exists and does nothing is worse than one that doesn't exist.

## Benchmarks

Opal 0.4.0 against npm, pnpm, yarn, and bun, each installing the same `package.json`. Measured 2026-10-06 with [`benchmarks/compare-pms.py`](./benchmarks/compare-pms.py) and the release binary. Four of its six scenarios are shown here:

- **cold**: no lockfile, cache, or `node_modules` (a first install)
- **ci**: a lockfile, but no cache or `node_modules` (a fresh CI runner)
- **warm**: a lockfile and a cache, but no `node_modules` (a reinstall on your machine)
- **noop**: everything already installed

Each number is the median of 3 runs (cold, ci) or 5 (warm, noop); fastest in bold. Every tool gets its own copy of the project and its own empty cache, the tools take turns so a network swing hits all of them, and install scripts are off for all five. Machine: AMD Ryzen 5 5625U (WSL limited to 8 of 12 threads), 16 GB RAM, Linux under WSL2, Node 24.19.0.

**express** (68 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.4.0 | **913ms** | 491ms | 92ms | 16ms | 33 MB |
| npm 12.0.2 | 1.75s | 1.04s | 643ms | 389ms | 160 MB |
| pnpm 11.21.0 | 1.36s | 1.23s | 765ms | 470ms | 329 MB |
| yarn 1.22.22 | 1.55s | 1.22s | 552ms | 266ms | 161 MB |
| bun 1.4.2 | 1.35s | **175ms** | **67ms** | **6ms** | **26 MB** |

**Next.js 16.3.2**, the `create-next-app` defaults (about 360 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.4.0 | 24.06s | 18.84s | **907ms** | 72ms | 491 MB |
| npm 12.0.2 | 27.58s | 13.43s | 10.85s | 633ms | 464 MB |
| pnpm 11.21.0 | 19.29s | 16.07s | 2.25s | 490ms | 1,794 MB |
| yarn 1.22.22 | 53.40s | 47.05s | 5.41s | 352ms | 659 MB |
| bun 1.4.2 | **17.27s** | **11.99s** | 1.09s | **16ms** | **216 MB** |

Across both machines (the second is an Intel Core i9-9900K, 16 threads):

- **Reinstalls are where it wins.** A warm install is 7× faster than npm on express and 12–19× on Next.js, where it is ahead of bun on one machine and level with it on the other. A no-op install is 20–24× (express) and 9× (Next.js) faster than npm; bun is faster still.
- **A first install of a large app is faster than npm's and slower than pnpm's and bun's.** On Next.js, Opal's cold install is 1.1–1.2× faster than npm's, and their ranges don't overlap on either machine; pnpm is 1.2–1.3× faster than Opal and bun 1.4–1.5×. On express, Opal is ahead of npm, pnpm, and yarn, and three runs don't separate it from bun.
- **CI on a large app is where it loses.** With a lockfile and an empty cache, Opal is 1.3–1.4× slower than npm on Next.js, 1.2–1.3× slower than pnpm, and 1.6× slower than bun. Nearly all of that install is the download. v0.3.0, which downloaded one package at a time, was 6–13× slower than npm on this install.
- **Memory:** bun uses the least on both projects. Opal is second on express and close to npm on Next.js.
- On Next.js, opal, yarn, and bun also download six musl builds that npm and pnpm skip (see [Limitations](#limitations)), which adds to their cold and CI times.

Absolute times vary between sessions and machines, so compare tools within one table rather than across tables.

The full method, min–max ranges, the second machine's tables, the other two scenarios (CI with a restored cache, adding a package), disk usage, earlier releases' tables, and how to reproduce every number are in [benchmarks/BENCHMARKS.md](./benchmarks/BENCHMARKS.md).

## Limitations

Worth knowing before you point Opal at a project:

- **CI installs of a large app are slower than npm's**: 1.3–1.4× on a Next.js app with a lockfile and an empty cache in the [benchmarks](#benchmarks). Downloads run 16 at a time since v0.4.0, and the download is still nearly all of that install.
- **`opal add` and `opal remove` ask the registry about every package in the tree**, not only the one being changed. On a Next.js app (418 package names) an add takes about half a second when that metadata was fetched in the last five minutes, about 2s when it is older and has to be rechecked, and 5–7s when it isn't cached at all, as on a machine that installed from a lockfile. npm takes under a second in each case.
- **Lifecycle scripts (`preinstall`/`install`/`postinstall`) do not run.** Packages shipping prebuilt binaries (`esbuild`, `sharp`, `@next/swc`) work; a package that needs `node-gyp` to compile at install time installs but does not build. `opal install` says so on every run: it names each dependency whose install scripts were skipped (including native addons that declare none and rely on npm running `node-gyp rebuild` for their `binding.gyp`), and the project's own lifecycle scripts, `prepare` included.
- **`libc` isn't checked.** On Linux with glibc (most distributions), Opal also installs the musl builds of native packages, which npm and pnpm skip. On a Next.js app that's six extra packages and 124 MB, 91 MB of it `@next/swc-linux-x64-musl`. A glibc system doesn't use them, but they cost download time and disk.
- **Peers are recorded and classified, never auto-installed.**
- **Versions can differ slightly from npm's.** Opal reuses a version already in the tree whenever it satisfies a range, where npm sometimes adds a newer copy: on a Next.js app, that's one package (`postcss` 8.5.23, where npm also installs 8.5.28). Opal doesn't prefer versions whose `engines` match your Node, which npm does. And it doesn't honor `bundleDependencies`: packages a dependency ships inside its own tarball are also resolved and downloaded from the registry, though Node still loads the bundled copy.
- **`package-lock.json` is ignored.** In a project npm already installed, the first `opal install` resolves every version and downloads every package again.
- **`git:` and `file:` specifiers are unsupported** and reported as such; dependencies come from a registry only.
- **If your project and cache sit on different filesystems** (a project on `/mnt/c` under WSL2 with the default cache, for instance), every file is copied instead of hardlinked and the install warns. Keep both on the same filesystem, or set `OPAL_CACHE_DIR` to a directory on the project's filesystem to move the shared store there.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) to build Opal from source, run the tests, and check a change before opening a pull request.

## License

MIT — see [LICENSE](./LICENSE).
