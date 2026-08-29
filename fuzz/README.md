# Fuzzing

`testing_strategy.md` §6: every parser of untrusted input gets a fuzz target,
because a panic on install is the worst failure mode a package manager has.
Four inputs qualify, in rough order of how little they can be trusted:

| Target      | Input                    | Why it is untrusted                       |
| ----------- | ------------------------ | ----------------------------------------- |
| `packument` | registry JSON            | arrives over the network, decides installs |
| `tarball`   | a package's bytes        | gzip, tar, and paths that become directories |
| `manifest`  | `package.json`           | whatever a publisher put there            |
| `lockfile`  | `opal.lock`              | committed to repositories others open     |

Needs nightly, since libFuzzer builds with sanitizer flags:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run lockfile -- -max_total_time=300
cargo +nightly fuzz build          # all four, no running
```

This is its own workspace on purpose: those sanitizer flags should not land on
an ordinary `cargo build` of the main tree.

Three bugs came out of the first run, all in the lockfile round trip, all now
covered by named tests in `lockfile.rs` rather than by checked-in crash
artifacts:

- a value ending in `\r` lost a character on every write, because `str::lines`
  strips one trailing carriage return;
- a specifier containing a newline **wrote another line** into `opal.lock` —
  `Range::parse(">=1\n<2")` succeeds and keeps the newline, so a dependency's
  own `package.json` could inject entries into the lockfile of every project
  installing it, and `--frozen-lockfile` exists to trust that file;
- `render` sorted and `parse` did not, so parsing was not a fixed point of
  rendering and a reordered lockfile did not survive being written back.
