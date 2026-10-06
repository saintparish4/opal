# Contributing to Opal

Opal is a Cargo workspace. You can work on it from macOS, Linux, or WSL2; native Windows isn't supported yet.

## Install dependencies

You need three things:

- **Rust**, installed with [rustup](https://rustup.rs) rather than your distro's `rust`/`cargo` packages. [`rust-toolchain.toml`](./rust-toolchain.toml) pins the `stable` channel along with `rustfmt` and `clippy`, and rustup picks it up when you run `cargo` in the repo.
- **A C compiler.** `blake3` and `ring` compile some C during the build. On Debian/Ubuntu, install `build-essential`; on macOS, run `xcode-select --install`.
- **git.**

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

`node` and `npm` are only needed for the registry-backed test suites below.

## Build

```bash
git clone https://github.com/saintparish4/opal
cd opal
cargo build --workspace
```

To build and run the `opal` binary in one step, pass its arguments after `--`:

```bash
cargo run -p opal-cli -- install --root path/to/project
```

For a release build, run `cargo build --release`. The binary is `./target/release/opal`.

## Where things live

- `crates/opal-core`: the module graph, resolver, and content-addressed store. It knows nothing about any one tool, and import resolution lives here only; tools use it rather than resolving imports themselves.
- `crates/opal-pm`: the package manager as a library: registry client, semver, resolution, `opal.lock`, and the `node_modules` linker.
- `crates/opal-cli`: the `opal` binary and everything that draws to the terminal.
- `fuzz/`: fuzz targets, in a workspace of their own.

`opal-runtime`, `opal-bundler`, and `opal-test` hold placeholder files only and aren't workspace members yet.

## Before you open a pull request

Run all three. CI runs the same checks on Ubuntu and macOS.

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

`--all-features` matters: it turns on `opal-pm`'s `fixtures` feature, a file-backed registry the `install-pipeline` and `install-crash-safety` suites use so they run offline. Without it those suites don't compile, and clippy never sees them.

One of those tests kills installs at random moments. If it fails, it prints an `OPAL_CHAOS_SEED`: set that variable to replay the same run, and set `OPAL_CHAOS_TRIALS` to run more trials. Run it with `--nocapture` to see where the kills landed. A kill that arrives after the install has finished tests nothing, and about 44% of them do, so quote a long run by the number of kills that interrupted a running install, not by its trial count.

If you change how the cache decides what's stale (CAS key derivation, integrity checks, or invalidation), add or update the tests in `crates/opal-core/tests/cache-invalidation.rs`. A bug there doesn't crash; it silently serves stale output.

CI also checks the dependency tree against [`deny.toml`](./deny.toml): permissive licenses only, crates.io as the only source, and no async runtime or OpenSSL. If you add or change a dependency, run it yourself with `cargo deny check bans licenses sources` (install it with `cargo install --locked cargo-deny`).

Commit messages use `<type>: <subject>`, where the type is `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, or `chore`, and the subject is imperative with no trailing period. A `perf` commit cites the benchmark that justifies it.

A change someone using Opal would notice gets a line in [`CHANGELOG.md`](./CHANGELOG.md), in the section at the top for the release it will ship in. A change to behaviour people may depend on, or to the `opal.lock` format, goes under **Breaking**. Each release's notes are taken from that file.

## More tests

These need extra setup, so `cargo test` skips them. Each has its own CI job.

```bash
# real packages from the public registry; test_execute also needs node
cargo test -p opal-cli --test npm-compatibility -- --ignored test_install
cargo test -p opal-cli --test npm-compatibility -- --ignored test_execute

# opal's resolution checked against npm's on the same package.json; needs node and npm
cargo test -p opal-pm --test npm-cross-check -- --ignored --nocapture

# the install benchmark: CI records its numbers but never fails on them
cargo bench -p opal-pm --bench install-pipeline -- --rtt-ms 25 --scenario cold
```

Fuzzing needs nightly Rust and `cargo-fuzz`; see [`fuzz/README.md`](./fuzz/README.md).
