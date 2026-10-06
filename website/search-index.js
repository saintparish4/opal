// What the header's search looks through: the home page's sections and the
// blog's posts. Rebuilt by hand when either changes.
window.OPAL_SEARCH = [
 {
  "title": "Install Opal",
  "url": "#top",
  "kind": "Home",
  "text": "Beta Opal v0.3.1 released → Opal is a crash‑safe JavaScript toolkit. One graph. A package manager today, with a runtime, bundler and test runner planned, in a single binary. Use opal install in an existing Node.js project: it installs what npm would, from one store shared by every project. Install Opal v0.3.1 curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash macOS & Linux Windows View install script ↗ Linux x64 and arm64 (glibc 2.39 for v0.3.1), macOS x64 and Apple Silicon. Checked against the release's SHA256 checksums. On Windows, Opal runs inside WSL2: open a WSL terminal and run the same command. Native Windows isn't supported yet. Then follow the quickstart → opal install express Installing a Next.js app Preliminary Next.js 16.3.2 defaults · about 360 packages · Ryzen 5 5625U, Linux under WSL2 · install scripts off for every tool · median of 5 · measured 2026-10-05 with a pre-release build of v0.4.0 · these numbers may change · v0.3.1, the release you can install today, is slower on a first install and in CI method"
 },
 {
  "title": "Four tools, one graph. One works today.",
  "url": "#tools",
  "kind": "Home",
  "text": "Four tools, one graph. One works today. opal install drops into an existing Node.js project. No runtime switch required. Works today Package manager alongside npm · pnpm · yarn npm-compatible resolution, a reviewable lockfile, and a hoisted node_modules hardlinked from one shared store. $ opal install Planned Runtime will replace node JavaScript and TypeScript on V8, loading modules through the same resolver the package manager uses. opal run · not built yet Planned Bundler will replace esbuild · webpack Tree-shaking and minification over the shared graph, with outputs cached by the hash of their inputs. opal build · not built yet Planned Test runner will replace jest · vitest Test discovery through the shared graph, with assertions and a reporter, running on Opal's own runtime. opal test · not built yet"
 },
 {
  "title": "One binary, four commands.",
  "url": "#minute",
  "kind": "Home",
  "text": "A minute with Opal One binary, four commands. Install, re-run, lock it down for CI, check the store, and upgrade. Every step below is a real command with real output — click one. 01 Install dependencies $ opal install Resolves package.json , writes opal.lock before it downloads anything, then links the tree. 02 Run it again $ opal install The linker compares the lockfile with what's on disk and applies only the difference. 03 Lock it down in CI $ opal install --frozen-lockfile Installs exactly what opal.lock records, and fails instead of rewriting it. 04 Check the shared store $ opal cache verify Every file is stored once under the BLAKE3 hash of its contents. Verify re-hashes all of it. 05 Stay current $ opal upgrade Checks the new binary against the release's checksums and runs it once before replacing the old one. ● ● ● ~/my-app — opal 01 / 05 Preliminary Real output from an unreleased build of master on a 364-package Next.js app, abridged and sped up. Hover to pause. ↻ replay"
 },
 {
  "title": "Cleaner install output. opal upgrade. Honest errors.",
  "url": "#release",
  "kind": "Home",
  "text": "v0.3.1 Latest release · September 2026 Cleaner install output. opal upgrade. Honest errors. v0.3.1 reworks what an install prints, adds a command that upgrades the binary in place, and says plainly when opal.lock is missing. The install path itself is unchanged from v0.3.0. Read the release notes → opal upgrade A thin download bar in opal colours, and a spinner with a timer for resolving and linking A one-line summary, with one detail line only when it matters Packages skipped for other platforms are reported as one line, not sixty-six Warnings print last, so they aren't scrolled away opal upgrade installs the latest or a named version, checked against SHA256SUMS and run once before it replaces anything --frozen-lockfile with no opal.lock now says how to create one All releases →"
 },
 {
  "title": "Built to be trusted first.",
  "url": "#trust",
  "kind": "Home",
  "text": "Tested, not claimed Built to be trusted first. A package manager that is fast and occasionally wrong is not fast. These are the properties Opal tests for, with the numbers from the test runs. 432 /433 The versions npm picks On a 364-package Next.js app, 432 of npm's 433 package versions are identical. The differences are documented, and a CI job fails if a new one appears. 332 kills Kill it anywhere Every write is atomic and the linker is a reconciler. In one soak, 332 SIGKILLs interrupted a running install; re-running converged on a clean tree every time. Against the real registry, 102 more did the same across express and a Next.js app. 33 MB Small while it works Peak memory installing express was 33 MB, against 162 MB for npm and 26 MB for bun: second lowest of the five. On a Next.js app it was third, 475 MB against bun's 241 MB and npm's 437 MB. Same preliminary run as the charts below. 1 store One copy of every file Packages are verified against the registry's integrity hash, stored once per machine, and hardlinked into each project. A second copy of a Next.js app added 10.5 MB on disk, against 463 MB with npm; bun and pnpm share files too, and added 8.3 and 13.7 MB."
 },
 {
  "title": "Benchmarks: Fastest where you reinstall. Not yet where you download.",
  "url": "#install-bench",
  "kind": "Home",
  "text": "opal install Preliminary Fastest where you reinstall. Not yet where you download. A first install, CI with and without its cache, a reinstall, a no-op, adding one package, and what a second copy costs on disk, against npm, pnpm, yarn and bun at their defaults. Opal's losses are on the same page as its wins. opal install → Method & raw numbers Preliminary Measured 2026-10-05 on an AMD Ryzen 5 5625U (WSL limited to 8 of 12 threads, 16 GB, Linux under WSL2) with a pre-release build of Opal v0.4.0, npm 12.0.2, pnpm 11.21.0, yarn 1.22.22 and bun 1.4.2. One machine and one session, and the numbers may change before v0.4.0 is released. Each tool added the package with its own add command; one of Opal's three Next.js rounds took 7.1s, which the median hides. v0.3.1, the release you can install today, downloads one package at a time and is 4–7× slower than npm on a first install; its numbers and the raw samples for these charts are in the benchmark notes."
 },
 {
  "title": "Limits: What it doesn't do yet.",
  "url": "#limits",
  "kind": "Home",
  "text": "Read before you switch What it doesn't do yet. Worth knowing before you point Opal at a project. The first three are asserted by tests, so the suite says what doesn't work as plainly as what does. Install scripts don't run. Packages that ship prebuilt binaries work. A package that compiles with node-gyp installs but isn't built, and opal install names it every time. Peer dependencies aren't installed. They are recorded, not added to the tree the way npm 7 and later add them. git: and file: dependencies are refused. Dependencies come from a registry only. Both are planned for v0.5.0. No add , remove or update in v0.3.1. Edit package.json and run opal install . opal add and opal remove are coming in v0.4.0; --peer , --global , and adding to a folder with no package.json are planned for v0.5.0. Commands that aren't built aren't in the binary. First installs are slow in v0.3.1. It downloads one package at a time, 4–7× slower than npm on a cold install. The fix is on master and ships in v0.4.0. Linux needs a recent glibc. v0.3.1's binaries need glibc 2.39 (Ubuntu 24.04 or newer); the next release goes back to 2.34. Alpine and other musl distributions aren't supported. The full list of limitations →"
 },
 {
  "title": "Opal v0.3.1",
  "url": "blog/opal-v0.3.1/",
  "kind": "Post",
  "text": "The launch release: cleaner terminal output, a new download bar, and opal upgrade . Resolution, downloads, and linking are unchanged, so the benchmarks measured on v0.3.0 still hold: warm and no-op installs timed the same on both builds. To install Opal curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash To upgrade Opal opal upgrade What's new A thin download bar in opal colours. No line fills the terminal's last column, the suspected cause of a duplicate bar seen once in Windows Terminal. opal upgrade installs the latest release, or a named one, in place. The download is checked against SHA256SUMS and run once before it replaces anything. A one-line summary , plus one more line only when it says something: what a re-run kept, or that the shared store supplied everything. A Next.js install now ends in five lines instead of about seventy-five. No stale lines between stages. A terminal install no longer leaves \"Resolving dependencies\" and \"Installing N packages\" on screen. Packages built for other platforms print as one line instead of one each: 66 on a Next.js app. Warnings print after the result , so they are the last thing on screen. --frozen-lockfile without an opal.lock says so , and how to create one. --cache-dir is described in --help . Full Changelog : https://github.com/saintparish4/opal/compare/v0.3.0...v0.3.1"
 },
 {
  "title": "Opal v0.3.1: What's new",
  "url": "blog/opal-v0.3.1/#what-s-new",
  "kind": "Post",
  "text": "A thin download bar in opal colours. No line fills the terminal's last column, the suspected cause of a duplicate bar seen once in Windows Terminal. opal upgrade installs the latest release, or a named one, in place. The download is checked against SHA256SUMS and run once before it replaces anything. A one-line summary , plus one more line only when it says something: what a re-run kept, or that the shared store supplied everything. A Next.js install now ends in five lines instead of about seventy-five. No stale lines between stages. A terminal install no longer leaves \"Resolving dependencies\" and \"Installing N packages\" on screen. Packages built for other platforms print as one line instead of one each: 66 on a Next.js app. Warnings print after the result , so they are the last thing on screen. --frozen-lockfile without an opal.lock says so , and how to create one. --cache-dir is descri"
 },
 {
  "title": "Opal v0.3.0",
  "url": "blog/opal-v0.3.0/",
  "kind": "Post",
  "text": "Opal now installs the versions npm would install. On six edge-case fixtures the trees match npm's package for package, and on a 365-package Next.js scaffold 432 of npm's 433 package versions match. To install Opal v0.3.0 curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.3.0 bash Matching npm Version preference agrees with npm-pick-manifest itself on all 12,032 cases of a generated registry. Faster linking Linking runs in parallel, one node_modules depth at a time, about 2.7× faster than v0.2.1 on the same tree and machine. Install scripts are named The install names every package whose install scripts it didn't run, the project's own included. A contained exports A package's exports can no longer resolve to a file outside the package. The suites behind it Randomized SIGKILLs during install that must converge. A cross-check of resolution against npm's. A fuzz target and a proptest for the resolver. A benchmark job that records numbers on every push. Full Changelog : https://github.com/saintparish4/opal/compare/v0.2.1...v0.3.0"
 },
 {
  "title": "Opal v0.3.0: Matching npm",
  "url": "blog/opal-v0.3.0/#matching-npm",
  "kind": "Post",
  "text": "Version preference agrees with npm-pick-manifest itself on all 12,032 cases of a generated registry."
 },
 {
  "title": "Opal v0.3.0: Faster linking",
  "url": "blog/opal-v0.3.0/#faster-linking",
  "kind": "Post",
  "text": "Linking runs in parallel, one node_modules depth at a time, about 2.7× faster than v0.2.1 on the same tree and machine."
 },
 {
  "title": "Opal v0.3.0: Install scripts are named",
  "url": "blog/opal-v0.3.0/#install-scripts-are-named",
  "kind": "Post",
  "text": "The install names every package whose install scripts it didn't run, the project's own included."
 },
 {
  "title": "Opal v0.3.0: A contained exports",
  "url": "blog/opal-v0.3.0/#a-contained-exports",
  "kind": "Post",
  "text": "A package's exports can no longer resolve to a file outside the package."
 },
 {
  "title": "Opal v0.3.0: The suites behind it",
  "url": "blog/opal-v0.3.0/#the-suites-behind-it",
  "kind": "Post",
  "text": "Randomized SIGKILLs during install that must converge. A cross-check of resolution against npm's. A fuzz target and a proptest for the resolver. A benchmark job that records numbers on every push. Full Changelog : https://github.com/saintparish4/opal/compare/v0.2.1...v0.3.0"
 },
 {
  "title": "Opal v0.2.1",
  "url": "blog/opal-v0.2.1/",
  "kind": "Post",
  "text": "A performance fix for one finding: a 367-package Next.js install spent most of its time parsing package metadata it never used, and almost none of it on the network. To install Opal v0.2.1 curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.2.1 bash Every package on the registry publishes a record of every version it has ever released. Opal was building the full dependency detail for all of them before selecting one. On that project it parsed 130,246 published versions to choose 367 , then held the rest in memory and freed it again. Version bodies are now kept as raw slices and parsed on demand. Measured On a 367-package Next.js tree with a warm store and warm metadata: before after whole install, offline 128.6s 37.7s the resolve phase alone 74.3s 8.4s whole install, with network 188.9s 60.0s --prefer-offline — 26.0s lockfile and tree already present — 5.9s Also in this release Per-phase timings. 365 packages resolved in 587.1s described the whole install, not resolution. It now reads (resolve 43.2s, fetch 0.5s, link 13.5s) , because those three phases are slow for unrelated reasons and one total hides which to fix. Fewer syscalls when linking. A 21,000 file tree made 21,000 create_dir_all calls where a few hundred would do. Invisible on ext4, and not on a DrvFS mount under WSL2. The hardlink warning now names the remedy. If your project and cache sit on different filesystems, every file is copied instead of linked, which can be most of an install's time. Set OPAL_CACHE_DIR to the same filesystem, or move the project off /mnt/c if you are on WSL2. Note A version a registry lists but cannot be installed from — no tarball, or no usable integrity — is now skipped in favour of the next best match, rather than being filtered out"
 },
 {
  "title": "Opal v0.2.1: Measured",
  "url": "blog/opal-v0.2.1/#measured",
  "kind": "Post",
  "text": "On a 367-package Next.js tree with a warm store and warm metadata: before after whole install, offline 128.6s 37.7s the resolve phase alone 74.3s 8.4s whole install, with network 188.9s 60.0s --prefer-offline — 26.0s lockfile and tree already present — 5.9s"
 },
 {
  "title": "Opal v0.2.1: Also in this release",
  "url": "blog/opal-v0.2.1/#also-in-this-release",
  "kind": "Post",
  "text": "Per-phase timings. 365 packages resolved in 587.1s described the whole install, not resolution. It now reads (resolve 43.2s, fetch 0.5s, link 13.5s) , because those three phases are slow for unrelated reasons and one total hides which to fix. Fewer syscalls when linking. A 21,000 file tree made 21,000 create_dir_all calls where a few hundred would do. Invisible on ext4, and not on a DrvFS mount under WSL2. The hardlink warning now names the remedy. If your project and cache sit on different filesystems, every file is copied instead of linked, which can be most of an install's time. Set OPAL_CACHE_DIR to the same filesystem, or move the project off /mnt/c if you are on WSL2."
 },
 {
  "title": "Opal v0.2.1: Note",
  "url": "blog/opal-v0.2.1/#note",
  "kind": "Post",
  "text": "A version a registry lists but cannot be installed from — no tarball, or no usable integrity — is now skipped in favour of the next best match, rather than being filtered out when the packument was first read. No lockfile format change. v0.2.0 lockfiles are read as-is. Full Changelog : https://github.com/saintparish4/opal/compare/v0.2.0...v0.2.1"
 },
 {
  "title": "Opal v0.2.0",
  "url": "blog/opal-v0.2.0/",
  "kind": "Post",
  "text": "Correctness, speed, and visibility across the package manager. Four bugs in this release produced a wrong node_modules without ever failing an install. To install Opal v0.2.0 curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.2.0 bash Breaking opal.lock is now v3. A v0.1.0 binary refuses a v3 lockfile rather than misreading it, so upgrade before sharing a lockfile with one. Going the other way, v0.2.0 replaces a v1 or v2 lockfile by re-resolving — except under --frozen-lockfile , where it is an error, because a build that promised not to rewrite the lockfile must not rewrite it to upgrade it either. A required dependency this platform cannot run is now EBADPLATFORM , matching npm. Previously it installed and produced a tree that could not run. A package declared in optionalDependencies is still skipped, which is how jest , vite and anything else depending on fsevents keeps working. The graph cache misses once after upgrading: the resolver learned to walk files it used to skip, so records written by v0.1.0 no longer describe what it would produce. Fixed — installs that were silently wrong A root dependency could be linked at the wrong version. With shared@^1 declared alongside a dependency on shared@^2 , the tree got 2.x at the top level and the version you asked for was downloaded and never placed. --production overwrote opal.lock , and --production --frozen-lockfile — the ordinary CI invocation — failed against a perfectly valid lockfile every time. Platform variants of native optionals all installed. esbuild declares 25, totalling 256 MB; one 9.8 MB binary is what belongs on a host. They stay recorded in opal.lock , so one committed file still installs the right binary on every platform. A dependency could write lines in"
 },
 {
  "title": "Opal v0.2.0: Breaking",
  "url": "blog/opal-v0.2.0/#breaking",
  "kind": "Post",
  "text": "opal.lock is now v3. A v0.1.0 binary refuses a v3 lockfile rather than misreading it, so upgrade before sharing a lockfile with one. Going the other way, v0.2.0 replaces a v1 or v2 lockfile by re-resolving — except under --frozen-lockfile , where it is an error, because a build that promised not to rewrite the lockfile must not rewrite it to upgrade it either. A required dependency this platform cannot run is now EBADPLATFORM , matching npm. Previously it installed and produced a tree that could not run. A package declared in optionalDependencies is still skipped, which is how jest , vite and anything else depending on fsevents keeps working. The graph cache misses once after upgrading: the resolver learned to walk files it used to skip, so records written by v0.1.0 no longer describe what it would produce."
 },
 {
  "title": "Opal v0.2.0: Fixed — installs that were silently wrong",
  "url": "blog/opal-v0.2.0/#fixed-installs-that-were-silently-wrong",
  "kind": "Post",
  "text": "A root dependency could be linked at the wrong version. With shared@^1 declared alongside a dependency on shared@^2 , the tree got 2.x at the top level and the version you asked for was downloaded and never placed. --production overwrote opal.lock , and --production --frozen-lockfile — the ordinary CI invocation — failed against a perfectly valid lockfile every time. Platform variants of native optionals all installed. esbuild declares 25, totalling 256 MB; one 9.8 MB binary is what belongs on a host. They stay recorded in opal.lock , so one committed file still installs the right binary on every platform. A dependency could write lines into your lockfile. Range::parse(\">=1\\npkg …\") kept the newline and the lockfile is one fact per line, so a dependency's own package.json could append entries — including package entries with attacker-controlled tarball URLs — to the lockfile of every pro"
 },
 {
  "title": "Opal v0.2.0: Faster",
  "url": "blog/opal-v0.2.0/#faster",
  "kind": "Post",
  "text": "Registry metadata is cached across runs. Re-resolving a 74-package tree against a warm store went from 7.6s to 0.36s , making no round trips at all. --offline and --prefer-offline are new. Packuments are fetched in npm's abbreviated form, roughly half the bytes. The CAS hashes before writing, so content it already holds costs a hash and a stat instead of a write, an fsync, a read-back and a rename."
 },
 {
  "title": "Opal v0.2.0: New",
  "url": "blog/opal-v0.2.0/#new",
  "kind": "Post",
  "text": "npm: alias specifiers. \"string-width-cjs\": \"npm:string-width@^4.2.0\" installs one package under another's name — how a package depends on two majors of one dependency at once. Unblocks glob@10 , rimraf@5 , node-gyp@10 and sucrase , which could not be installed at all. Progress output. A spinner while resolving and linking, a bar advancing per package while fetching, and one warning per deprecated package. Plain lines when stderr is not a terminal, so CI logs stay readable. opal cache gc prunes what it can no longer use — graph records for deleted projects, and registry metadata past 30 days. Bounded retries and explicit timeouts on registry requests; ceilings on what a tarball may unpack to."
 },
 {
  "title": "Opal v0.2.0: Install",
  "url": "blog/opal-v0.2.0/#install",
  "kind": "Post",
  "text": "curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | bash macOS and Linux, x64 and arm64. WSL2 is covered by the Linux builds. Native Windows remains a v2 target. Full Changelog : https://github.com/saintparish4/opal/compare/v0.1.0...v0.2.0"
 },
 {
  "title": "Opal v0.1.0",
  "url": "blog/opal-v0.1.0/",
  "kind": "Post",
  "text": "The first release: opal install , with prebuilt binaries for Linux and macOS and an install script. To install Opal v0.1.0 curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.1.0 bash Full Changelog : https://github.com/saintparish4/opal/commits/v0.1.0"
 },
 {
  "title": "Blog",
  "url": "blog/",
  "kind": "Page",
  "text": "Release notes and writing about Opal. RSS feed."
 }
];
