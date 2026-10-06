# Benchmarks

Opal against npm, pnpm, yarn, and bun, each installing the same `package.json`. The [README](../README.md#benchmarks) has the headline numbers. This file has the full method, min–max ranges, both machines, and how to reproduce every table.

- [Method](#method)
- [Results: 2026-10-06, opal 0.4.0](#results-2026-10-06-opal-040)
- [Pre-release: 2026-10-05, the same source before the tag](#pre-release-2026-10-05-the-same-source-before-the-tag)
- [Preliminary: 2026-10-04, all six scenarios and disk usage](#preliminary-2026-10-04-all-six-scenarios-and-disk-usage)
- [Preliminary: 2026-10-03, unreleased master build](#preliminary-2026-10-03-unreleased-master-build)
- [Results: 2026-09-26, opal 0.3.0](#results-2026-09-26-opal-030)
- [Reproducing](#reproducing)
- [The kill test](#the-kill-test)
- [The internal install benchmark](#the-internal-install-benchmark)

## Method

The harness is [`compare-pms.py`](./compare-pms.py). It runs six scenarios, each asking a different question:

- **cold**: no lockfile, cache, or `node_modules` (a first install)
- **ci**: a lockfile, but no cache or `node_modules` (a fresh CI runner)
- **warm**: a lockfile and a cache, but no `node_modules` (a reinstall on your machine)
- **ci-cached**: the same state as warm, run with each tool's frozen-lockfile command: `opal install --frozen-lockfile`, `npm ci`, and `--frozen-lockfile` for pnpm, yarn, and bun (a CI runner that restored its cache)
- **noop**: everything already installed
- **add**: one new dependency added to an installed project with each tool's own command: `opal add`, `npm install <pkg>`, `pnpm add`, `yarn add`, and `bun add`. Each round adds a different small package with no dependencies of its own, pinned to one version, so every round pays for metadata and a download it hasn't seen. Before 2026-10-05 the harness wrote the dependency into `package.json` and ran each tool's plain install, because Opal had no `add` command; the 2026-10-04 tables were measured that way.

After the scenarios it measures **disk usage**: the project's `node_modules`, the tool's cache, the two together, and what a second copy of the same project adds on top when it is installed from the same cache. Sizes are blocks on disk with every file counted once however many names it has, which is what hardlinking from a shared store saves.

ci-cached, add, and disk usage were added on 2026-10-04. The tables from that date on have them; the earlier ones have the first four scenarios.

The harness keeps the comparison fair in these ways:

- **Isolation.** Each tool gets its own copy of the project and its own empty cache next to it. Everything sits on one filesystem, so every tool that hardlinks can.
- **Interleaving.** Runs take turns round by round, starting from a different tool each round. A swing in network speed or page-cache state then hits every tool, not just whichever happened to be running.
- **Same work.** Install scripts are off for all five tools, because Opal doesn't run them. npm's and yarn's update checks are off too, since they're a network request that isn't part of installing.
- **Measurement.** Wall time and peak memory (RSS) come from `wait4` on the tool's process. Each number is the median of 3 rounds (cold, ci, add) or 5 (warm, ci-cached, noop).
- **Correctness.** After every run, the installed tree has to pass a `require` check, or the run fails.
- **Launchers.** pnpm is launched as `node <pnpm.mjs>`. pnpm 11 is a single bundle of about 13 MB that Node compiles on every start (about 1.2s here), and the standalone `@pnpm/exe` started even slower. yarn is 1.22.22 (classic) through corepack.

Absolute times move by 2–3× between sessions on the same machine, mostly with the network. Compare tools within one table, not across tables.

The two projects:

- **express**: `{"dependencies": {"express": "^5"}}`, 68 packages.
- **Next.js 16.3.2**: the `package.json` that `create-next-app@16.3.2` writes, inlined in the harness verbatim. Its caret ranges still float, so a later run can resolve newer versions.

## Results: 2026-10-06, opal 0.4.0

Tool versions: opal 0.4.0, npm 12.0.2, pnpm 11.21.0, yarn 1.22.22, bun 1.4.2, Node 24.19.0. Opal is the `opal-linux-x64` asset from the [v0.4.0 release](https://github.com/saintparish4/opal/releases/tag/v0.4.0), checked against its `SHA256SUMS`; the SHA-256 of the `opal` inside it is `61595255213c9aeb7ef54a29a33604dc37540bf19a60f0fe8c8ab84c7af94540`, and both machines ran that file. The npm, pnpm, and bun versions are the ones bun.com's own install chart names.

Each cell is the median, with min–max in parentheses. Each machine made 240 runs (5 tools, 24 rounds, 2 projects), and all 480 passed. The machines ran one after the other, not at the same time, so neither took bandwidth from the other.

### Laptop: AMD Ryzen 5 5625U (WSL limited to 8 of 12 threads), 16 GB RAM, Linux under WSL2

These are the numbers the README and the site's charts show. Raw samples: [`results/express-20261006-000421.json`](./results/express-20261006-000421.json), [`results/next-20261006-002335.json`](./results/next-20261006-002335.json).

**express** (68 packages)

| Tool | cold | ci | warm | ci-cached | noop | add | Peak RSS (cold) |
|---|---|---|---|---|---|---|---|
| opal 0.4.0 | 913ms (908ms–926ms) | 491ms (477ms–508ms) | 92ms (87ms–97ms) | 92ms (87ms–98ms) | 16ms (16ms–21ms) | 499ms (158ms–566ms) | 33 MB |
| npm 12.0.2 | 1.75s (1.73s–1.91s) | 1.04s (990ms–1.05s) | 643ms (642ms–673ms) | 639ms (623ms–664ms) | 389ms (363ms–430ms) | 531ms (515ms–719ms) | 160 MB |
| pnpm 11.21.0 | 1.36s (1.33s–1.39s) | 1.23s (1.22s–1.26s) | 765ms (758ms–788ms) | 778ms (734ms–810ms) | 470ms (465ms–485ms) | 901ms (883ms–977ms) | 329 MB |
| yarn 1.22.22 | 1.55s (1.48s–1.58s) | 1.22s (1.21s–1.23s) | 552ms (541ms–571ms) | 556ms (541ms–577ms) | 266ms (260ms–276ms) | 587ms (582ms–1.12s) | 161 MB |
| bun 1.4.2 | 1.35s (450ms–1.49s) | 175ms (174ms–1.10s) | 67ms (66ms–67ms) | 67ms (67ms–72ms) | 6ms (6ms–11ms) | 117ms (108ms–189ms) | 26 MB |

| Tool | node_modules | Cache | One project, with its cache | A second copy adds |
|---|---|---|---|---|
| opal 0.4.0 | 4.3 MB | 7.6 MB | 8.4 MB | 0.8 MB |
| npm 12.0.2 | 4.3 MB | 2.1 MB | 6.5 MB | 4.3 MB |
| pnpm 11.21.0 | 4.7 MB | 6.6 MB | 7.7 MB | 1.1 MB |
| yarn 1.22.22 | 4.2 MB | 6.2 MB | 10.4 MB | 4.2 MB |
| bun 1.4.2 | 4.2 MB | 4.5 MB | 5.0 MB | 0.5 MB |

**Next.js 16.3.2** (364 packages)

| Tool | cold | ci | warm | ci-cached | noop | add | Peak RSS (cold) |
|---|---|---|---|---|---|---|---|
| opal 0.4.0 | 24.06s (23.25s–24.64s) | 18.84s (18.68s–19.18s) | 907ms (863ms–949ms) | 915ms (877ms–937ms) | 72ms (66ms–77ms) | 939ms (585ms–5.98s) | 491 MB |
| npm 12.0.2 | 27.58s (26.39s–27.69s) | 13.43s (12.94s–13.78s) | 10.85s (10.68s–11.07s) | 10.80s (10.76s–11.09s) | 633ms (617ms–654ms) | 839ms (792ms–1.03s) | 464 MB |
| pnpm 11.21.0 | 19.29s (18.88s–19.53s) | 16.07s (15.26s–16.30s) | 2.25s (2.20s–2.29s) | 2.25s (2.21s–2.25s) | 490ms (490ms–518ms) | 3.87s (3.67s–9.06s) | 1794 MB |
| yarn 1.22.22 | 53.40s (52.81s–54.44s) | 47.05s (46.52s–47.78s) | 5.41s (5.20s–5.47s) | 5.45s (5.33s–5.61s) | 352ms (347ms–368ms) | 2.44s (2.34s–2.89s) | 659 MB |
| bun 1.4.2 | 17.27s (16.70s–17.48s) | 11.99s (11.76s–12.83s) | 1.09s (1.06s–1.12s) | 1.09s (1.08s–1.20s) | 16ms (16ms–16ms) | 205ms (134ms–1.17s) | 216 MB |

| Tool | node_modules | Cache | One project, with its cache | A second copy adds |
|---|---|---|---|---|
| opal 0.4.0 | 573.5 MB | 633.8 MB | 644.3 MB | 10.5 MB |
| npm 12.0.2 | 463.3 MB | 110.8 MB | 574.1 MB | 463.3 MB |
| pnpm 11.21.0 | 453.8 MB | 650.3 MB | 664.0 MB | 13.7 MB |
| yarn 1.22.22 | 587.1 MB | 1972.4 MB | 2559.5 MB | 587.1 MB |
| bun 1.4.2 | 586.0 MB | 587.7 MB | 596.0 MB | 8.3 MB |

### Desktop: Intel Core i9-9900K (16 threads), 16 GB RAM, Linux under WSL2

Measured in the half hour before the laptop's run. **Its raw samples are not in this repository yet.** They are on that machine, as `express-20261005-233955.json` (SHA-256 `0f0f7447cfee934dea88d9124427d3dce198667b9d5d974370bc644d207e2044`) and `next-20261005-235934.json` (SHA-256 `26d1a2da2e3d4a8c9d923c09bea2974e9f65052fa5f125c57db2004f78357045`). Until they are added, the two tables below are the harness's printed summary and can't be recomputed from here.

**express** (68 packages)

| Tool | cold | ci | warm | ci-cached | noop | add | Peak RSS (cold) |
|---|---|---|---|---|---|---|---|
| opal 0.4.0 | 887ms (826ms–895ms) | 507ms (460ms–508ms) | 87ms (87ms–92ms) | 87ms (82ms–102ms) | 16ms (16ms–21ms) | 505ms (154ms–526ms) | 37 MB |
| npm 12.0.2 | 1.61s (1.49s–1.76s) | 908ms (903ms–908ms) | 607ms (587ms–633ms) | 628ms (582ms–639ms) | 326ms (316ms–332ms) | 474ms (459ms–689ms) | 164 MB |
| pnpm 11.21.0 | 1.20s (1.18s–1.25s) | 1.06s (1.02s–1.08s) | 749ms (709ms–771ms) | 769ms (708ms–799ms) | 413ms (402ms–419ms) | 860ms (852ms–966ms) | 470 MB |
| yarn 1.22.22 | 1.48s (1.45s–1.51s) | 1.18s (1.18s–1.19s) | 536ms (510ms–553ms) | 557ms (530ms–568ms) | 230ms (224ms–240ms) | 658ms (526ms–1.17s) | 159 MB |
| bun 1.4.2 | 1.21s (496ms–1.32s) | 583ms (547ms–672ms) | 36ms (36ms–41ms) | 41ms (36ms–46ms) | 6ms (6ms–6ms) | 118ms (77ms–133ms) | 26 MB |

| Tool | node_modules | Cache | One project, with its cache | A second copy adds |
|---|---|---|---|---|
| opal 0.4.0 | 4.3 MB | 7.6 MB | 8.4 MB | 0.8 MB |
| npm 12.0.2 | 4.3 MB | 2.1 MB | 6.5 MB | 4.3 MB |
| pnpm 11.21.0 | 4.7 MB | 6.6 MB | 7.7 MB | 1.1 MB |
| yarn 1.22.22 | 4.2 MB | 6.2 MB | 10.4 MB | 4.2 MB |
| bun 1.4.2 | 4.2 MB | 4.5 MB | 5.0 MB | 0.5 MB |

**Next.js 16.3.2** (364 packages)

| Tool | cold | ci | warm | ci-cached | noop | add | Peak RSS (cold) |
|---|---|---|---|---|---|---|---|
| opal 0.4.0 | 23.96s (22.68s–24.56s) | 18.81s (18.69s–20.20s) | 658ms (607ms–698ms) | 618ms (587ms–1.17s) | 67ms (62ms–72ms) | 915ms (506ms–5.58s) | 506 MB |
| npm 12.0.2 | 28.02s (27.62s–29.64s) | 14.96s (14.79s–15.21s) | 12.78s (12.55s–12.95s) | 12.64s (12.56s–12.72s) | 618ms (607ms–627ms) | 751ms (745ms–980ms) | 515 MB |
| pnpm 11.21.0 | 18.29s (18.17s–19.21s) | 14.31s (14.24s–14.60s) | 1.70s (1.68s–1.75s) | 1.71s (1.69s–1.72s) | 439ms (429ms–444ms) | 3.68s (3.55s–7.98s) | 1948 MB |
| yarn 1.22.22 | 54.08s (52.20s–54.26s) | 47.12s (46.80s–47.28s) | 6.42s (6.38s–6.67s) | 6.36s (6.16s–6.46s) | 316ms (311ms–317ms) | 2.94s (2.86s–3.56s) | 665 MB |
| bun 1.4.2 | 16.31s (15.63s–16.53s) | 11.55s (11.48s–14.29s) | 617ms (607ms–632ms) | 617ms (612ms–622ms) | 16ms (11ms–16ms) | 109ms (98ms–148ms) | 224 MB |

| Tool | node_modules | Cache | One project, with its cache | A second copy adds |
|---|---|---|---|---|
| opal 0.4.0 | 573.5 MB | 633.8 MB | 644.3 MB | 10.5 MB |
| npm 12.0.2 | 463.3 MB | 110.8 MB | 574.1 MB | 463.3 MB |
| pnpm 11.21.0 | 453.8 MB | 650.2 MB | 664.0 MB | 13.7 MB |
| yarn 1.22.22 | 587.1 MB | 1972.4 MB | 2559.5 MB | 587.1 MB |
| bun 1.4.2 | 586.0 MB | 587.7 MB | 596.0 MB | 8.3 MB |

### What these show

- **express:** on both machines Opal is ahead of npm, pnpm, and yarn on cold, ci, warm, ci-cached, and noop, with no overlap in their ranges, and level with npm on add. bun is ahead on warm, ci-cached, and noop. On a cold install Opal's median is lower on both machines (913ms against 1.35s, 887ms against 1.21s), but bun's fastest round is under 500ms on both, so three rounds don't settle it. On ci bun is ahead on the laptop (175ms against 491ms) and Opal is ahead on the desktop (507ms against 583ms, ranges not overlapping).
- **Next.js cold:** ahead of npm on both machines, and the ranges don't overlap: 24.06s against 27.58s on the laptop, 23.96s against 28.02s on the desktop, which is 1.1–1.2× faster. Behind pnpm by 1.2–1.3× and bun by 1.4–1.5×.
- **Next.js ci:** behind npm, pnpm, and bun on both machines: 1.3–1.4× slower than npm, 1.2–1.3× slower than pnpm, 1.6× slower than bun. Nearly all of a ci install is the download (17.8s of 18.8s in the laptop's median round).
- **Next.js warm:** 12× faster than npm on the laptop and 19× on the desktop, and 2.5–2.6× faster than pnpm. Against bun it is ahead on the laptop (907ms against 1.09s, ranges not overlapping) and level on the desktop (658ms against 617ms, ranges overlapping).
- **Nothing to do:** a no-op install is 20–24× faster than npm on express and 9× on Next.js. bun is faster still: 6ms and 16ms.
- **Adding a package:** bun is well ahead on both projects and both machines. On Next.js Opal's median is a little behind npm's (939ms against 839ms on the laptop, 915ms against 751ms on the desktop), and their ranges overlap. **Opal's first Next.js round is slow on both machines**, 5.98s on the laptop (5.7s of it resolving) and 5.58s on the desktop; the other rounds take under a second. The median hides that round, so read the range. The slow round is the first because of the state the earlier scenarios leave: ci empties each tool's cache, and an install from a lockfile fetches no registry metadata, so Opal's first add finds none cached and downloads it for all 418 package names in the tree before it resolves. Measured by hand on 2026-10-05 with a build of the same source: an add after a lockfile-only install into an empty cache took 5.4s (5.3s resolving), the next one 0.8s. An add whose cached metadata was more than five minutes old took 2.1s (1.7s resolving), and 0.7s with `--prefer-offline`.
- **Memory:** bun uses the least on both projects and both machines (26 MB on express; 216 MB and 224 MB on Next.js). Opal is second on express (33 MB and 37 MB). On Next.js it is close to npm: 491 MB against 464 MB on the laptop, 506 MB against 515 MB on the desktop.
- **Disk:** the same on both machines to within 0.1 MB. A second copy of the Next.js app costs 10.5 MB with Opal, 8.3 MB with bun, and 13.7 MB with pnpm, against 463 MB with npm and 587 MB with yarn. Sharing files from one store is what the three have in common; it is not an advantage Opal has over bun or pnpm. Opal's `node_modules` is the largest of the hardlinking tools' because it installs both the glibc and musl builds of native packages.
- **Against 0.3.0:** the laptop's Next.js cold install was 124.59s on 2026-09-26 and is 24.06s here; ci was 100.81s and is 18.84s. Those are different sessions and different versions of the other tools, so read them as the size of the change.

## Pre-release: 2026-10-05, the same source before the tag

The evening before the release, the laptop ran a local build of the tree v0.4.0 was cut from, with the same tool versions. It agreed with the results above on everything but one row: on a Next.js cold install Opal was level with npm (29.34s against 26.37s, ranges overlapping), where the release run has it ahead. Absolute times move between sessions, which is why tools are only compared within one run.

Its raw samples are [`results/express-20261005-204133.json`](./results/express-20261005-204133.json) and [`results/next-20261005-210232.json`](./results/next-20261005-210232.json), and its tables are in this file's history at [`d036770`](https://github.com/saintparish4/opal/blob/d036770/benchmarks/BENCHMARKS.md). This is also the first run in which **add** used each tool's own add command, and the first with these tool versions.

## Preliminary: 2026-10-04, all six scenarios and disk usage

Laptop only (AMD Ryzen 5 5625U, WSL limited to 8 of 12 threads, 16 GB RAM, Linux under WSL2), one session, with an unreleased build: `10d96c0` plus the install-output changes that were not yet committed. The SHA-256 of the binary is `8fd770fea77a5124d3d250138a13b1a6cec105ad5e5645eaaff8bf205b928a95`. Raw samples: [`results/express-20261004-174510.json`](./results/express-20261004-174510.json), [`results/next-20261004-180345.json`](./results/next-20261004-180345.json). The tool versions were npm 11.17.0, pnpm 11.17.0, and bun 1.3.14, and every tool was given a hand-edited `package.json` for **add**, so don't compare these with the results above.

**express** (68 packages)

| Tool | cold | ci | warm | ci-cached | noop | add | Peak RSS (cold) |
|---|---|---|---|---|---|---|---|
| opal master | 865ms (830ms–879ms) | 451ms (445ms–458ms) | 87ms (82ms–92ms) | 87ms (82ms–122ms) | 16ms (16ms–21ms) | 458ms (168ms–540ms) | 32 MB |
| npm 11.17.0 | 1.57s (1.53s–1.77s) | 862ms (846ms–867ms) | 571ms (561ms–576ms) | 555ms (519ms–575ms) | 316ms (312ms–321ms) | 495ms (474ms–658ms) | 153 MB |
| pnpm 11.17.0 | 1.31s (1.26s–1.48s) | 1.09s (1.06s–1.15s) | 680ms (669ms–726ms) | 680ms (646ms–690ms) | 444ms (439ms–454ms) | 832ms (809ms–909ms) | 328 MB |
| yarn 1.22.22 | 1.54s (1.44s–1.66s) | 1.15s (1.10s–1.28s) | 530ms (505ms–546ms) | 515ms (509ms–535ms) | 255ms (250ms–256ms) | 580ms (545ms–1.10s) | 161 MB |
| bun 1.3.14 | 1.08s (419ms–1.30s) | 218ms (205ms–282ms) | 67ms (67ms–67ms) | 67ms (67ms–67ms) | 11ms (11ms–11ms) | 143ms (123ms–195ms) | 40 MB |

| Tool | node_modules | Cache | One project, with its cache | A second copy adds |
|---|---|---|---|---|
| opal master | 4.3 MB | 7.6 MB | 8.4 MB | 0.8 MB |
| npm 11.17.0 | 4.3 MB | 2.1 MB | 6.5 MB | 4.3 MB |
| pnpm 11.17.0 | 4.7 MB | 6.6 MB | 7.7 MB | 1.1 MB |
| yarn 1.22.22 | 4.2 MB | 6.2 MB | 10.4 MB | 4.2 MB |
| bun 1.3.14 | 4.2 MB | 4.5 MB | 5.0 MB | 0.5 MB |

**Next.js 16.3.2** (364 packages)

| Tool | cold | ci | warm | ci-cached | noop | add | Peak RSS (cold) |
|---|---|---|---|---|---|---|---|
| opal master | 24.05s (24.03s–26.48s) | 19.23s (17.96s–19.81s) | 876ms (864ms–901ms) | 889ms (856ms–993ms) | 67ms (66ms–77ms) | 757ms (501ms–5.95s) | 489 MB |
| npm 11.17.0 | 25.51s (24.84s–27.73s) | 12.32s (12.00s–13.12s) | 10.73s (10.33s–11.72s) | 10.27s (9.88s–10.51s) | 586ms (578ms–600ms) | 781ms (764ms–927ms) | 424 MB |
| pnpm 11.17.0 | 19.90s (18.44s–21.08s) | 15.59s (15.04s–16.04s) | 2.18s (2.11s–2.32s) | 2.16s (2.10s–2.18s) | 474ms (469ms–492ms) | 4.16s (3.70s–8.70s) | 1811 MB |
| yarn 1.22.22 | 52.35s (51.00s–54.06s) | 44.92s (44.08s–45.85s) | 5.01s (4.54s–5.36s) | 4.95s (4.56s–5.01s) | 338ms (331ms–360ms) | 2.32s (2.31s–2.73s) | 638 MB |
| bun 1.3.14 | 16.58s (16.32s–17.32s) | 12.43s (12.14s–13.45s) | 1.09s (1.08s–1.17s) | 1.09s (1.08s–1.10s) | 17ms (16ms–21ms) | 155ms (115ms–202ms) | 596 MB |

| Tool | node_modules | Cache | One project, with its cache | A second copy adds |
|---|---|---|---|---|
| opal master | 573.2 MB | 633.4 MB | 643.9 MB | 10.5 MB |
| npm 11.17.0 | 463.0 MB | 110.7 MB | 573.7 MB | 463.0 MB |
| pnpm 11.17.0 | 453.8 MB | 652.6 MB | 666.3 MB | 13.7 MB |
| yarn 1.22.22 | 586.7 MB | 1972.0 MB | 2558.7 MB | 586.7 MB |
| bun 1.3.14 | 585.7 MB | 587.4 MB | 595.7 MB | 8.3 MB |

### What these show

- **ci-cached matches warm for every tool.** The frozen-lockfile commands do the same work as a plain install when the lockfile is already current, so this scenario confirms the warm numbers and doesn't change the order.
- **Adding a package:** bun is well ahead on both projects. Opal is level with npm on Next.js (757ms against 781ms) and on express (458ms against 495ms), and ahead of pnpm and yarn. Opal re-resolves the whole tree when one dependency is added, and that is where its time goes. Its Next.js range runs to 5.95s: one of the three rounds was slow.
- **Disk:** a second copy of the Next.js app costs 10.5 MB with Opal, 8.3 MB with bun, and 13.7 MB with pnpm, against 463 MB with npm and 587 MB with yarn. Sharing files from one store is what the three have in common; it is not an advantage Opal has over bun or pnpm. Opal's `node_modules` is the largest of the hardlinking tools' because it installs both the glibc and musl builds of native packages.
- **Download-bound scenarios are unchanged in order:** bun leads cold and ci on Next.js, and Opal is 1.6× slower than npm on ci.

## Preliminary: 2026-10-03, unreleased master build

These tables are for a build of `master` that adds parallel downloads and stores one package's files on several threads. It is not a release: v0.3.1 is still what the install script gives you, and its numbers are the 2026-09-26 tables below. The build reports `opal 0.3.1`.

Both machines have run it, but the build is unreleased, so treat these as preliminary. The run against the v0.4.0 release binary on both machines is [above](#results-2026-10-06-opal-040).

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

You need Python 3.9+, network access, and Node plus every tool being compared on `PATH`. yarn must be Yarn 1 (classic). A run takes about 2 minutes for express and about 21 minutes for Next.js.

```sh
python3 benchmarks/compare-pms.py express --opal "$(command -v opal)" \
  --pnpm "node $HOME/.cache/node/corepack/v1/pnpm/11.21.0/bin/pnpm.mjs"

python3 benchmarks/compare-pms.py next --opal "$(command -v opal)" \
  --pnpm "node $HOME/.cache/node/corepack/v1/pnpm/11.21.0/bin/pnpm.mjs"
```

- `--tools opal,npm` compares a subset of the tools.
- `--rounds cold=3,ci=3,warm=5,ci-cached=5,noop=5,add=3` changes how many rounds each scenario gets. A scenario given 0 rounds is skipped; `add` has five packages to add, so five rounds at most.
- `--no-disk` skips the disk-usage measurement.
- `--work DIR` sets the scratch directory (default `/tmp/opal-compare`).
- `--results DIR` sets where the raw samples go (default `benchmarks/results/`). The samples record the path of the Opal binary that ran, so with the default the harness refuses a binary that sits under the system's temporary folder. Keep the binary somewhere you're happy to publish, such as `~/opal-bench/`.

Raw samples for every run are written to `benchmarks/results/` as JSON, one file per run. A table published here is committed together with the file it was computed from, so anyone can recompute it. There are two exceptions. The 2026-09-26 tables predate the rule: the laptop's samples were kept outside the repository and the desktop's were not kept. And the desktop's 2026-10-06 samples exist but are still on that machine; its section says so.

For numbers comparable with the tables above, use a release binary rather than a local build. `opal --version` can't tell the two apart, so the harness also records the SHA-256 of the binary it ran (`opal_binary` in the JSON); for a release it equals `sha256sum` of the `opal` inside the release archive. The 2026-09-26 runs predate that field. Since 0.4.0, `opal install` also names its commit on its first line.

## The kill test

[`kill-test.py`](./kill-test.py) sends SIGKILL to real `opal install` runs against the public registry, then checks that running it again finishes the job. The crash-safety suite in the repository makes the same claim with small fixture packages served from a local folder; this makes it against a real project, real downloads, and a kill that arrives whenever it arrives.

- One clean install is the reference.
- Each trial starts from an empty project and an empty cache, runs `opal install`, and kills it after a random delay somewhere inside the time the clean install took, and a little past it.
- A trial is killed one to three times in a row, so a kill can land in the recovery from the one before.
- After every kill, `opal cache verify` must find every stored object matching its key.
- Then `opal install` runs to completion, and the result must equal the reference: the same `opal.lock`, and the same `node_modules` file for file, with the same contents, modes, and symlink targets.

A kill that arrives after the install has exited tested nothing, so kills are counted by where they landed, read from what was on disk at that moment: resolving (no `opal.lock` yet), fetching (lockfile written, `node_modules` not started), linking, or finished. **The number to quote is the kills that interrupted a running install, not the number of trials.**

What it doesn't cover: a kill of the machine and not the process (power loss, where what reached the disk depends on fsync), a full disk, and two installs racing. The reference and the trials resolve minutes apart against a registry that can publish in between; a release inside a floating range would show up as a lockfile difference and is reported as a failed trial, not explained away.

### Results: 2026-10-06, opal 0.4.0

Laptop (AMD Ryzen 5 5625U, Linux under WSL2), with the v0.4.0 release binary, straight after the comparison above. Every trial converged.

| Project | Trials converged | Kills | Interrupted a running install | Resolving | Fetching | Linking | After it exited |
|---|---|---|---|---|---|---|---|
| express (68 packages, clean install 0.9s) | 60 of 60 | 114 | 75 | 38 | 30 | 7 | 39 |
| Next.js (364 packages, clean install 24.1s) | 12 of 12 | 25 | 15 | 4 | 11 | 0 | 10 |

Raw results: [`results/kill-express-20261006-002506.json`](./results/kill-express-20261006-002506.json), [`results/kill-next-20261006-003242.json`](./results/kill-next-20261006-003242.json).

That is 90 kills that interrupted a running install, and a clean tree after every one. With the two earlier runs below, 252 kills have interrupted a real install against the public registry, and every one converged. No Next.js kill landed in linking in this run either; the fixture suite reaches linking on purpose.

### Results: 2026-10-05, opal 0.4.0 candidate build

The laptop, with the local build described under [Pre-release](#pre-release-2026-10-05-the-same-source-before-the-tag). Every trial converged.

| Project | Trials converged | Kills | Interrupted a running install | Resolving | Fetching | Linking | After it exited |
|---|---|---|---|---|---|---|---|
| express (68 packages) | 60 of 60 | 121 | 87 | 51 | 28 | 8 | 34 |
| Next.js (364 packages) | 12 of 12 | 29 | 15 | 3 | 12 | 0 | 14 |

Raw results: [`results/kill-express-20261005-210418.json`](./results/kill-express-20261005-210418.json), [`results/kill-next-20261005-211242.json`](./results/kill-next-20261005-211242.json).

That is 102 kills that interrupted a running install, and a clean tree after every one. No Next.js kill landed in linking this time, and 8 express kills did.

### Results: 2026-10-04, unreleased master build

Laptop (AMD Ryzen 5 5625U, Linux under WSL2), with an uncommitted build on top of `10d96c0`; the SHA-256 of the binary is in each results file. Every trial converged.

| Project | Trials converged | Kills | Interrupted a running install | Resolving | Fetching | Linking | After it exited |
|---|---|---|---|---|---|---|---|
| express (68 packages, clean install 1.3s) | 60 of 60 | 112 | 41 | 19 | 15 | 7 | 71 |
| Next.js (364 packages, clean install 25.1s) | 12 of 12 | 27 | 19 | 4 | 14 | 1 | 8 |

Raw results: [`results/kill-express-20261004-173253.json`](./results/kill-express-20261004-173253.json), [`results/kill-next-20261004-174049.json`](./results/kill-next-20261004-174049.json).

That is 60 kills that interrupted a running install, and a clean tree after every one. Linking is the thin part: it is about a second of a 25-second Next.js install, so random delays seldom land in it, and 8 kills did across both projects. The fixture suite reaches it on purpose, through fault points placed mid-link and between packages.

```sh
python3 benchmarks/kill-test.py express --opal ~/opal-bench/opal --trials 60
python3 benchmarks/kill-test.py next --opal ~/opal-bench/opal --trials 12
```

`--seed N` replays a run's delays. They are delays, not positions, so the same seed lands its kills in the same places only on a machine and network of the same speed. A failed trial's project and cache are left in the scratch directory (`--work`, default `/tmp/opal-kill-test`) to look at.

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
