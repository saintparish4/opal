# Benchmarks

Opal against npm, pnpm, yarn, and bun, each installing the same `package.json`. The [README](../README.md#benchmarks) has the headline numbers. This file has the full method, min–max ranges, both machines, and how to reproduce every table.

- [Method](#method)
- [Preliminary: 2026-10-03, unreleased master build](#preliminary-2026-10-03-unreleased-master-build)
- [Results: 2026-09-26, opal 0.3.0](#results-2026-09-26-opal-030)
- [Reproducing](#reproducing)
- [The internal install benchmark](#the-internal-install-benchmark)

## Method

The harness is [`compare-pms.py`](./compare-pms.py). It runs four scenarios, each asking a different question:

- **cold**: no lockfile, cache, or `node_modules` (a first install)
- **ci**: a lockfile, but no cache or `node_modules` (a fresh CI runner)
- **warm**: a lockfile and a cache, but no `node_modules` (a reinstall on your machine)
- **noop**: everything already installed

The harness keeps the comparison fair in these ways:

- **Isolation.** Each tool gets its own copy of the project and its own empty cache next to it. Everything sits on one filesystem, so every tool that hardlinks can.
- **Interleaving.** Runs take turns round by round, starting from a different tool each round. A swing in network speed or page-cache state then hits every tool, not just whichever happened to be running.
- **Same work.** Install scripts are off for all five tools, because Opal doesn't run them. npm's and yarn's update checks are off too, since they're a network request that isn't part of installing.
- **Measurement.** Wall time and peak memory (RSS) come from `wait4` on the tool's process. Each number is the median of 3 rounds (cold, ci) or 5 (warm, noop).
- **Correctness.** After every run, the installed tree has to pass a `require` check, or the run fails.
- **Launchers.** pnpm is launched as `node <pnpm.mjs>`. pnpm 11 is a single 12.8 MB bundle that Node compiles on every start (about 1.2s here), and the standalone `@pnpm/exe` started even slower. yarn is 1.22.22 (classic) through corepack.

Absolute times move by 2–3× between sessions on the same machine, mostly with the network. Compare tools within one table, not across tables.

The two projects:

- **express**: `{"dependencies": {"express": "^5"}}`, 68 packages.
- **Next.js 16.3.2**: the `package.json` that `create-next-app@16.3.2` writes, inlined in the harness verbatim. Its caret ranges still float, so a later run can resolve newer versions.

## Preliminary: 2026-10-03, unreleased master build

These tables are for a build of `master` that adds parallel downloads and stores one package's files on several threads. It is not a release: v0.3.1 is still what the install script gives you, and its numbers are the 2026-09-26 tables below. The build reports `opal 0.3.1`.

Both machines have run it, but the build is unreleased, so treat these as preliminary. They will be replaced by a run against the v0.4.0 release binary on both machines.

Tool versions as in the 2026-09-26 run. Each machine made 160 runs, and all of them passed.

### Laptop: AMD Ryzen 5 5625U (WSL limited to 8 of 12 threads), 16 GB RAM, Linux under WSL2

Built at [`2ba79e2`](https://github.com/saintparish4/opal/commit/2ba79e2). The SHA-256 of the binary that ran is `322e439b446b20fee317defc146773e1ac085226d95adabc13c579dc7f642ab2`. Raw samples: [`results/express-20261003-095838.json`](./results/express-20261003-095838.json) and [`results/next-20261003-101640.json`](./results/next-20261003-101640.json).

**express** (68 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal master | 946ms (940ms–1.00s) | 499ms (474–535ms) | 93ms (88–99ms) | 16ms (16–21ms) | **31 MB** |
| npm 11.17.0 | 1.69s (1.57–1.77s) | 1.01s (1.01–1.12s) | 648ms (625–705ms) | 357ms (355–378ms) | 154 MB |
| pnpm 11.17.0 | 1.35s (1.33–1.41s) | 1.27s (1.26–1.32s) | 818ms (778–895ms) | 502ms (497–508ms) | 329 MB |
| yarn 1.22.22 | 1.59s (1.56–1.71s) | 1.23s (1.22–1.24s) | 585ms (570–724ms) | 295ms (278–369ms) | 163 MB |
| bun 1.3.14 | **605ms** (417ms–1.32s) | **327ms** (230–418ms) | **78ms** (78–82ms) | **11ms** (11–11ms) | 41 MB |

**Next.js 16.3.2**

| | cold | ci | warm | noop | Peak memory (cold) | Packages installed |
|---|---|---|---|---|---|---|
| opal master | 30.35s (29.88–34.51s) | 24.02s (23.23–24.75s) | **951ms** (930ms–1.03s) | 83ms (73–88ms) | 499 MB | 364 |
| npm 11.17.0 | 33.03s (32.35–33.29s) | 15.27s (15.06–15.32s) | 11.87s (11.64–12.60s) | 628ms (618–657ms) | **432 MB** | 359 |
| pnpm 11.17.0 | 25.18s (24.94–27.37s) | 19.19s (18.65–19.37s) | 2.47s (2.41–2.53s) | 519ms (504–576ms) | 1,851 MB | 354 |
| yarn 1.22.22 | 57.70s (55.94–59.82s) | 48.04s (47.97–51.51s) | 5.63s (5.34–6.12s) | 363ms (358–373ms) | 650 MB | 365 |
| bun 1.3.14 | **20.09s** (19.71–21.72s) | **14.85s** (14.55–15.00s) | 1.17s (1.15–1.29s) | **21ms** (21–22ms) | 574 MB | 365 |

### Desktop: Intel Core i9-9900K (16 threads), 16 GB RAM, Linux under WSL2

Measured the same day. Built on that machine at [`ea72765`](https://github.com/saintparish4/opal/commit/ea72765), which is the same code: the one commit after `2ba79e2` changes only these notes and the site. It is a separate build, so its SHA-256 differs: `a797cbd9c5459cbd7e69b7b48cd10938ed37cf015c0235a81bdff7dc83c684c8`. Raw samples: [`results/express-20261003-182856.json`](./results/express-20261003-182856.json) and [`results/next-20261003-184524.json`](./results/next-20261003-184524.json).

**express** (68 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal master | 877ms (864–962ms) | **509ms** (488–522ms) | 103ms (103–150ms) | 21ms (21–37ms) | **36 MB** |
| npm 11.17.0 | 1.55s (1.46–1.77s) | 942ms (902ms–1.00s) | 600ms (589–631ms) | 313ms (303–318ms) | 158 MB |
| pnpm 11.17.0 | 1.24s (1.18–1.29s) | 1.10s (1.07–1.10s) | 785ms (744–928ms) | 436ms (421–446ms) | 416 MB |
| yarn 1.22.22 | 1.51s (1.48–1.53s) | 1.24s (1.24–1.30s) | 549ms (527–621ms) | 241ms (236–247ms) | 163 MB |
| bun 1.3.14 | **385ms** (381ms–1.27s) | 612ms (452–638ms) | **42ms** (41–42ms) | **6ms** (6–6ms) | 39 MB |

**Next.js 16.3.2**

| | cold | ci | warm | noop | Peak memory (cold) | Packages installed |
|---|---|---|---|---|---|---|
| opal master | 26.10s (24.76–26.45s) | 20.74s (20.26–30.91s) | **652ms** (647–929ms) | 67ms (67–77ms) | 495 MB | 364 |
| npm 11.17.0 | 28.83s (27.62–29.03s) | 15.62s (14.99–15.74s) | 13.37s (13.22–14.19s) | 621ms (606–669ms) | **441 MB** | 359 |
| pnpm 11.17.0 | 19.07s (18.71–19.49s) | 15.19s (14.52–16.03s) | 1.81s (1.79–2.93s) | 468ms (447–478ms) | 1,950 MB | 354 |
| yarn 1.22.22 | 56.72s (56.14–60.07s) | 50.61s (48.51–54.80s) | 6.55s (6.39–9.66s) | 339ms (329–360ms) | 630 MB | 365 |
| bun 1.3.14 | **17.81s** (16.64–18.18s) | **12.89s** (12.22–13.12s) | 668ms (641–677ms) | **16ms** (16–16ms) | 712 MB | 365 |

### What the preliminary numbers show

- **express:** on both machines Opal is ahead of npm, pnpm, and yarn in every scenario, and behind bun on warm and noop. On a cold install bun's median is lower, but its range spans Opal's on both machines. On ci bun is ahead on the laptop; on the desktop Opal's median is lower (509ms against 612ms) and bun's range spans Opal's.
- **Next.js cold:** ahead of npm on the desktop (26.10s against 28.83s, and the ranges don't overlap), level with it on the laptop (the ranges overlap). Behind pnpm by 1.2–1.4× and bun by 1.5× on both.
- **Next.js ci:** behind npm, pnpm, and bun on both machines: 1.3–1.6× slower than npm, 1.25–1.4× slower than pnpm, 1.6× slower than bun. One of the desktop's three Opal runs took 30.91s, nearly all of the extra in the download; the median is unaffected.
- **Next.js warm:** the fastest of the five on the laptop. On the desktop it is level with bun (652ms against 668ms, and the ranges overlap) and ahead of the other three.
- **Memory:** the least of the five on express, second to npm on Next.js, on both machines.

## Results: 2026-09-26, opal 0.3.0

Tool versions: opal 0.3.0 (the `opal-linux-x64` asset from the GitHub Release, checked against `SHA256SUMS`), npm 11.17.0, pnpm 11.17.0, yarn 1.22.22, bun 1.3.14, Node 24.19.0. Each cell is the median, with min–max in parentheses; the fastest median is in bold. Each machine made 160 runs (5 tools × 16 rounds × 2 projects), and all of them passed.

### Laptop: AMD Ryzen 5 5625U (WSL limited to 8 of 12 threads), 16 GB RAM, Linux under WSL2

These are the numbers the README quotes. Every cell here was recomputed from the run's raw samples.

**express** (68 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.3.0 | 8.24s (7.72–13.99s) | 5.55s (5.41–5.74s) | 101ms (96–107ms) | 21ms (16–22ms) | **15 MB** |
| npm 11.17.0 | 1.88s (1.79–2.18s) | 1.04s (1.01–1.11s) | 636ms (628–650ms) | 355ms (351–363ms) | 153 MB |
| pnpm 11.17.0 | 1.45s (1.41–1.48s) | 1.25s (1.21–1.25s) | 788ms (783–804ms) | 505ms (496–515ms) | 329 MB |
| yarn 1.22.22 | 1.71s (1.68–1.72s) | 1.32s (1.30–1.32s) | 598ms (587–612ms) | 290ms (282–294ms) | 160 MB |
| bun 1.3.14 | **519ms** (442ms–1.57s) | **328ms** (264–511ms) | **77ms** (76–82ms) | **11ms** (11–12ms) | 40 MB |

**Next.js 16.3.2**

| | cold | ci | warm | noop | Peak memory (cold) | Packages installed |
|---|---|---|---|---|---|---|
| opal 0.3.0 | 124.59s (121.76–128.27s) | 100.81s (98.34–101.28s) | **1.08s** (1.04–1.10s) | 96ms (94–103ms) | **285 MB** | 364 |
| npm 11.17.0 | 29.44s (29.21–31.95s) | 16.25s (15.92–16.64s) | 13.46s (13.43–13.70s) | 734ms (700–747ms) | 416 MB | 359 |
| pnpm 11.17.0 | 22.42s (22.36–22.54s) | 17.15s (16.96–17.23s) | 2.70s (2.30–2.77s) | 595ms (576–610ms) | 1,884 MB | 354 |
| yarn 1.22.22 | 59.98s (58.66–61.19s) | 53.90s (53.69–54.99s) | 6.70s (6.65–6.89s) | 414ms (411–422ms) | 633 MB | 365 |
| bun 1.3.14 | **19.27s** (18.84–19.54s) | **13.52s** (13.51–14.32s) | 1.32s (1.29–1.36s) | **22ms** (20–23ms) | 565 MB | 365 |

### Desktop: Intel Core i9-9900K (16 threads), 16 GB RAM, Linux under WSL2

Measured the same day on a different network. The same release asset was installed with `install.sh` and `OPAL_VERSION=v0.3.0`.

This machine's raw samples were not kept. The tables below were copied from the harness's printed summary, so they can't be recomputed, and the upper ends of the README's ranges (7× on a cold install, 13× on CI) come from them.

**express** (68 packages)

| | cold | ci | warm | noop | Peak memory (cold) |
|---|---|---|---|---|---|
| opal 0.3.0 | 7.53s (7.36–7.61s) | 6.07s (5.63–6.57s) | 93ms (93–98ms) | 16ms (16–21ms) | **15 MB** |
| npm 11.17.0 | 1.61s (1.58–1.91s) | 934ms (893–972ms) | 590ms (575–621ms) | 291ms (286–329ms) | 158 MB |
| pnpm 11.17.0 | 1.32s (1.31–1.35s) | 1.14s (1.13–1.17s) | 739ms (731–799ms) | 418ms (382–442ms) | 470 MB |
| yarn 1.22.22 | 1.61s (1.42–1.63s) | 1.25s (1.16–1.33s) | 560ms (534–575ms) | 225ms (219–242ms) | 162 MB |
| bun 1.3.14 | **480ms** (334ms–1.27s) | **557ms** (413ms–1.14s) | **42ms** (41–42ms) | **6ms** (6–6ms) | 40 MB |

**Next.js 16.3.2**

| | cold | ci | warm | noop | Peak memory (cold) | Packages installed |
|---|---|---|---|---|---|---|
| opal 0.3.0 | 180.29s (170.38–190.45s) | 177.91s (165.67–202.74s) | 673ms (600–868ms) | 77ms (72–104ms) | **277 MB** | 364 |
| npm 11.17.0 | 27.88s (27.44–27.91s) | 14.03s (13.82–14.26s) | 12.37s (12.00–12.97s) | 738ms (698–750ms) | 420 MB | 359 |
| pnpm 11.17.0 | 18.60s (18.28–20.10s) | 15.78s (15.21–16.54s) | 1.55s (1.44–1.60s) | 447ms (418–478ms) | 1,789 MB | 354 |
| yarn 1.22.22 | 56.11s (53.08–56.35s) | 48.00s (46.74–48.20s) | 6.36s (5.96–6.71s) | 319ms (301–329ms) | 649 MB | 365 |
| bun 1.3.14 | **16.87s** (15.56–18.39s) | **11.97s** (11.63–14.70s) | **641ms** (590–667ms) | **16ms** (11–16ms) | 718 MB | 365 |

### What the numbers show

- **Cold and CI installs are slow because Opal fetches one package at a time.** Every other tool downloads in parallel. A ci install is the fetch alone, which is why it tracks cold so closely. Parallel downloads are the next thing being built.
- **The desktop's Next.js cold and ci times are much worse for Opal**: 180s against the laptop's 125s. npm, pnpm, and bun were about the same speed or faster there. Opal's express fetch took about as long on both machines, so the difference comes from Next.js's large tarballs. The likely cause is slower single-stream downloads on that network, which only a one-at-a-time fetcher feels, but this wasn't measured.
- **Warm installs are where Opal wins.** On Next.js it's the fastest of the five on the laptop, and tied with bun on the desktop, where the ranges overlap.
- **Opal uses the least memory of the five** on every cold install, on both machines.
- **Package counts differ for two reasons.** Opal, yarn, and bun also install six musl builds of native packages that npm and pnpm skip (see [Limitations](../README.md#limitations)). pnpm stores each version once. Separately, Opal installs only `postcss` 8.5.23, where the others add 8.5.28 as well. Apart from that, all five install the same package versions.

## Reproducing

You need Python 3.9+, network access, and Node plus every tool being compared on `PATH`. yarn must be Yarn 1 (classic). A run takes about 3 minutes for express and about 36 minutes for Next.js.

```sh
python3 benchmarks/compare-pms.py express --opal "$(command -v opal)" \
  --pnpm "node $HOME/.cache/node/corepack/v1/pnpm/11.17.0/bin/pnpm.mjs"

python3 benchmarks/compare-pms.py next --opal "$(command -v opal)" \
  --pnpm "node $HOME/.cache/node/corepack/v1/pnpm/11.17.0/bin/pnpm.mjs"
```

- `--tools opal,npm` compares a subset of the tools.
- `--rounds cold=3,ci=3,warm=5,noop=5` changes how many rounds each scenario gets.
- `--work DIR` sets the scratch directory (default `/tmp/opal-compare`).
- `--results DIR` sets where the raw samples go (default `benchmarks/results/`). The samples record the path of the Opal binary that ran, so with the default the harness refuses a binary that sits under the system's temporary folder. Keep the binary somewhere you're happy to publish, such as `~/opal-bench/`.

Raw samples for every run are written to `benchmarks/results/` as JSON, one file per run. A table published here from now on is committed together with the file it was computed from, so anyone can recompute it. The tables above predate that: the laptop's samples were kept outside the repository and the desktop's were not kept.

For numbers comparable with the tables above, use a release binary rather than a local build. `opal --version` can't tell the two apart, so the harness also records the SHA-256 of the binary it ran (`opal_binary` in the JSON); for a release it equals `sha256sum` of the `opal` inside the release archive. The 2026-09-26 runs predate that field.

## The internal install benchmark

`cargo bench -p opal-pm --bench install-pipeline` measures Opal alone against a synthetic `file://` registry with a simulated round-trip time, so it doesn't depend on the public registry or the network:

```sh
cargo bench -p opal-pm --bench install-pipeline -- --rtt-ms 25 --scenario cold
```

Its scenarios are `cold`, `resolve`, `link`, and `noop`. CI runs it on every push and posts the numbers to the job summary (the "Install benchmark (tracked, not gated)" job). It never fails a build, because benchmarks are noisier than tests.

It has two workloads, and they answer different questions:

- **The default** is 64 small packages whose files compress to almost nothing, so a download costs one round trip and no transfer time. It shows time spent waiting on round trips and local work, and nothing about download size. A change that overlaps requests looks several times better here than it is on a real network: parallel fetching measured 6.8× on this workload and about 2.4× on the real Next.js tree.
- **`--workload scaffold`** has the Next.js scaffold's totals, measured on 2026-10-01: 364 packages, about 20,000 files, about 540 MB unpacked, and about 153 MiB of tarballs. Pair it with `--bandwidth-mbit`, which charges every response's bytes against one link that all requests share, so overlapping requests can't shorten the transfer:

  ```sh
  cargo bench -p opal-pm --bench install-pipeline -- --workload scaffold \
    --rtt-ms 70 --bandwidth-mbit 37 --scenario cold --iterations 2
  ```

  37 Mbit/s and 70 ms are what one network measured on one afternoon (157 MB in 34s over 16 connections), not constants; set them to the network you care about. At those values the benchmark's fetch took 41s where the real tree's took 43–44s the same afternoon. Generating the fixture takes one to two minutes.

  **Read this workload as a floor on transfer time, not a prediction of a real install.** It models one shared link and nothing else: no limit on what a single connection carries, and a bandwidth that never changes. So it credits parallel downloading with more than a real network gives: it measured parallel fetching at about 5× where the real tree, in the same machine state, showed 2.7×. Use it to see whether a change adds or removes transfer and round trips, and measure the real tree before quoting a speedup.
