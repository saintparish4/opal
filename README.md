<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/assets/opal-logo-horizontal-white.svg">
    <img src=".github/assets/opal-logo-horizontal-ink.svg" alt="Opal" width="400">
  </picture>
</p>

## What is Opal?

Opal is an all-in-one toolkit for JavaScript and TypeScript projects, built to cover package management, running code, bundling, and testing. It ships as a single executable called `opal`.

At its core is `opal-core`, a module graph engine written in Rust. It works out what every file imports and where each import resolves, then caches that answer by content hash, so the next run over unchanged files is a cache hit. Every tool Opal adds is meant to share that one graph instead of carrying its own resolver.

The first of those tools is the package manager, and it works today. It picks the same versions npm would, lays out `node_modules` the way npm does, and links every file in from a single content-addressed store shared by all the projects on your machine.

```bash
opal install                     # install the dependencies in package.json
```

The `opal` command-line tool also lets you inspect a project's module graph and the shared store. Installs are crash-safe: if one is killed partway through, running `opal install` again finishes the job.

```bash
opal graph index.js --root .     # resolve everything index.js imports
opal cache verify                # re-hash the shared store and check it for corruption
opal cache gc                    # delete store files no project uses anymore
```

> **Status**: Beta. The package manager works today — `opal install` resolves against the real npm registry and produces a `node_modules` tree Node runs against, validated on real projects (a Next.js scaffold at 365 packages, express, webpack, and a curated compatibility suite), and it is under active development, so expect rough edges and breaking releases. The runtime (`opal run`), bundler (`opal build`), and test runner (`opal test`) are **not implemented**; their directories under `crates/` hold placeholder files only.

<a name="deployment"></a>

## Install

Opal supports Linux (x64 & arm64) and macOS (x64 & Apple Silicon). On Windows, it runs inside WSL2.

> **Linux users**: the prebuilt binaries need glibc 2.34 or newer (Ubuntu 22.04+, Debian 12+, Fedora 35+, RHEL 9+). Check yours with `ldd --version`. On an older system, Opal fails to start with a `GLIBC_2.34 not found` error. Alpine and other musl-based distributions aren't supported.

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

There's no canary channel; every release is a tagged [GitHub Release](https://github.com/saintparish4/opal/releases). To run unreleased changes from `master`, build from source.

## Usage

Opal has four commands today: `opal install`, `opal graph`, `opal cache`, and `opal upgrade`. Run `opal <command> --help` to see their flags.

Commit `opal.lock`. In CI, run `opal install --frozen-lockfile`, which installs exactly what `opal.lock` records and fails instead of changing it.

`add`, `remove`, `update`, `why`, `outdated`, `audit`, and `publish` aren't implemented yet, so the binary doesn't have them. A command that exists and does nothing is worse than one that doesn't exist.

## Benchmarks

Opal against npm, pnpm, yarn, and bun, each installing the same `package.json`. Measured 2026-09-26 with [`scripts/compare-pms.py`](./scripts/compare-pms.py), in four scenarios:

- **cold**: no lockfile, cache, or `node_modules` (a first install)
- **ci**: a lockfile, but no cache or `node_modules` (a fresh CI runner)
- **warm**: a lockfile and a cache, but no `node_modules` (a reinstall on your machine)
- **noop**: everything already installed

Each number is the median of 3 runs (cold, ci) or 5 (warm, noop); fastest in bold. Every tool gets its own copy of the project and its own empty cache, the tools take turns so a network swing hits all of them, and install scripts are off for all five. Machine: AMD Ryzen 5 5625U (8 threads), 16 GB RAM, Linux under WSL2, Node 24.19.0.

**express** (68 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.3.0 | 8.24s | 5.55s | 101ms | 21ms | **15 MB** |
| npm 11.17.0 | 1.88s | 1.04s | 636ms | 355ms | 153 MB |
| pnpm 11.17.0 | 1.45s | 1.25s | 788ms | 505ms | 329 MB |
| yarn 1.22.22 | 1.71s | 1.32s | 598ms | 290ms | 160 MB |
| bun 1.3.14 | **519ms** | **328ms** | **77ms** | **11ms** | 40 MB |

**Next.js 16.3.2**, the `create-next-app` defaults (about 360 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.3.0 | 124.59s | 100.81s | **1.08s** | 96ms | **285 MB** |
| npm 11.17.0 | 29.44s | 16.25s | 13.46s | 734ms | 416 MB |
| pnpm 11.17.0 | 22.42s | 17.15s | 2.70s | 595ms | 1,884 MB |
| yarn 1.22.22 | 59.98s | 53.90s | 6.70s | 414ms | 633 MB |
| bun 1.3.14 | **19.27s** | **13.52s** | 1.32s | **22ms** | 565 MB |

<details>
<summary>The same benchmark on a second machine: Intel Core i9-9900K (16 threads), 16 GB RAM, Linux under WSL2</summary>

Same tool versions and Node 24.19.0, measured 2026-09-26 on a different network.

**express**

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.3.0 | 7.53s | 6.07s | 93ms | 16ms | **15 MB** |
| npm 11.17.0 | 1.61s | 934ms | 590ms | 291ms | 158 MB |
| pnpm 11.17.0 | 1.32s | 1.14s | 739ms | 418ms | 470 MB |
| yarn 1.22.22 | 1.61s | 1.25s | 560ms | 225ms | 162 MB |
| bun 1.3.14 | **480ms** | **557ms** | **42ms** | **6ms** | 40 MB |

**Next.js 16.3.2**

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.3.0 | 180.29s | 177.91s | 673ms | 77ms | **277 MB** |
| npm 11.17.0 | 27.88s | 14.03s | 12.37s | 738ms | 420 MB |
| pnpm 11.17.0 | 18.60s | 15.78s | 1.55s | 447ms | 1,789 MB |
| yarn 1.22.22 | 56.11s | 48.00s | 6.36s | 319ms | 649 MB |
| bun 1.3.14 | **16.87s** | **11.97s** | **641ms** | **16ms** | 718 MB |

</details>

Across both machines:

- **First installs and CI are where Opal loses.** It's 4–7× slower than npm on a cold install and 5–13× slower on CI, because it downloads packages one at a time. The gap is widest on Next.js's large downloads, and it depends on the network. Parallel downloads are the next thing being built.
- **Reinstalls are where it wins.** A warm install is 6× faster than npm on express and 12–18× on Next.js, where it's roughly tied with bun. A no-op install is 17–18× (express) and 8–10× (Next.js) faster than npm; bun is faster still.
- **It uses the least memory of the five** on every cold install.
- On Next.js, opal, yarn, and bun also download six musl builds that npm and pnpm skip (see [Limitations](#limitations)), which adds to their cold and CI times.

Absolute times vary between sessions and machines, so compare tools within one table rather than across tables.

## Limitations

Worth knowing before you point Opal at a project:

- **First installs and CI installs are slower than npm's**, 4–7× on a cold install and up to 13× on CI in the [benchmarks](#benchmarks), because packages download one at a time. Linking already runs in parallel; parallel downloads are next.
- **Lifecycle scripts (`preinstall`/`install`/`postinstall`) do not run.** Packages shipping prebuilt binaries (`esbuild`, `sharp`, `@next/swc`) work; a package that needs `node-gyp` to compile at install time installs but does not build. `opal install` says so on every run: it names each dependency whose install scripts were skipped (including native addons that declare none and rely on npm running `node-gyp rebuild` for their `binding.gyp`), and the project's own lifecycle scripts, `prepare` included.
- **`libc` isn't checked.** On Linux with glibc (most distributions), Opal also installs the musl builds of native packages, which npm and pnpm skip. On a Next.js app that's six extra packages and 124 MB, 91 MB of it `@next/swc-linux-x64-musl`. A glibc system doesn't use them, but they cost download time and disk.
- **Peers are recorded and classified, never auto-installed.**
- **Versions can differ slightly from npm's.** Opal reuses a version already in the tree whenever it satisfies a range, where npm sometimes adds a newer copy: on a Next.js app, that's one package (`postcss` 8.5.23, where npm also installs 8.5.28). Opal doesn't prefer versions whose `engines` match your Node, which npm does. And it doesn't honor `bundleDependencies`: packages a dependency ships inside its own tarball are also resolved and downloaded from the registry, though Node still loads the bundled copy.
- **Changing `package.json` re-resolves the whole tree.** Adding one dependency can move unrelated packages to newer versions, where npm keeps each locked version that still satisfies its range.
- **`package-lock.json` is ignored.** In a project npm already installed, the first `opal install` resolves every version and downloads every package again.
- **`git:` and `file:` specifiers are unsupported** and reported as such; dependencies come from a registry only.
- **If your project and cache sit on different filesystems** (a project on `/mnt/c` under WSL2 with the default cache, for instance), every file is copied instead of hardlinked and the install warns. Keep both on the same filesystem, or set `OPAL_CACHE_DIR` to a directory on the project's filesystem to move the shared store there.

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) to build Opal from source, run the tests, and check a change before opening a pull request.

## License

MIT — see [LICENSE](./LICENSE).
