# Fuzzing

`testing_strategy.md` §6: every parser of untrusted input gets a fuzz target,
because a panic on install is the worst failure mode a package manager has.
Five inputs qualify, in rough order of how little they can be trusted, and a
sixth is here for a different reason:

| Target      | Input                    | Why it is untrusted                       |
| ----------- | ------------------------ | ----------------------------------------- |
| `packument` | registry JSON            | arrives over the network, decides installs |
| `tarball`   | a package's bytes        | gzip, tar, and paths that become directories |
| `manifest`  | `package.json`           | whatever a publisher put there            |
| `lockfile`  | `opal.lock`              | committed to repositories others open     |
| `resolver`  | JS/TS source + a dependency's `exports` | every installed file is parsed, and an `exports` map decides which file a specifier reaches |
| `edit`      | the project's own `package.json`, plus an `opal add` or `opal remove` | trusted, but hand-written, and the one file opal rewrites that it did not create |

Needs nightly, since libFuzzer builds with sanitizer flags:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run lockfile -- -max_total_time=300
cargo +nightly fuzz build          # all six, no running
```

`resolver` checks more than "does not panic": whatever a package's `exports`
resolves a specifier to has to stay inside that package, which is Node's rule.
Run it with its hand-written seeds and its dictionary:

```bash
cargo +nightly fuzz run resolver corpus/resolver seeds/resolver -- -dict=resolver.dict -timeout=10
```

`edit` also checks more than that. An edit that succeeds has to say what it
was for, leave every member outside the dependency groups as the value it
was, change nothing when repeated, and never turn a manifest opal could read
into one it cannot:

```bash
cargo +nightly fuzz run edit corpus/edit seeds/edit -- -dict=edit.dict -max_len=2048
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

Writing the `resolver` target's invariant turned up one more: an `exports`
pattern let a specifier such as `pkg/../../outside` resolve outside the
package. A crafted input tripped the invariant. The fuzzer itself, started
from the seeds against the unfixed resolver, did not rediscover it in 575,000
runs: an escape needs `../` at one exact spot, and coverage never rewards
getting close. So the containment property is also a proptest that builds
those segments directly (`crates/opal-core/tests/exports-properties.rs`),
and the original case is a named test
(`test_exports_never_resolve_outside_the_package` in
`crates/opal-core/tests/graph-resolution.rs`).

The `edit` target's first run stopped on `{"e": 32E2220}`: a number that is
valid JSON text and too large to read into any number. The editor carries
values as the text they were written with, so it accepted a manifest that
reading the manifest refuses. Nothing was written (the install reads the
edited manifest before it writes anything), but the registry had already been
asked about the package by then, so a broken `package.json` could be reported
as a missing package. The manifest is now read before the change is worked
out, held by `test_a_manifest_opal_cannot_read_is_refused_before_the_registry_is_asked`
in `crates/opal-pm/tests/add-remove.rs`.
