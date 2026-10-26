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

## The bundler

Two targets fuzz `opal build`, and they hold it to different things.

| Target | Input | What must hold |
| ------ | ----- | -------------- |
| `bundle_source` | any text, as an ES module entry and as a CommonJS one | the bundler builds it or refuses it, never panics, and what it builds parses |
| `bundle_graph` | the fuzzer's bytes as a seed for a generated module graph | the bundle, run under Node, is indistinguishable from the program |

`bundle_graph` spawns `node` twice per input, so it runs at tens of inputs a
second, not thousands, and coverage guidance buys little: the program's shape
comes from a seeded generator (`crates/opal-bundler/src/generate.rs`), not
from the bytes' structure. The long runs therefore use the generator's own
driver, which runs fresh seeds on every core and saves each divergent program:

```bash
cargo run --release -p opal-bundler --features oracle --example generated-graphs -- \
    --seconds 3600 --jobs 6 --first-seed 1000000 --failures target/generated-failures
cargo +nightly fuzz run bundle_source corpus/bundle_source seeds/bundle_source -- -timeout=10
```

The generator builds two to eight modules, mixing ES modules with strict and
sloppy CommonJS, importing each other in every form `SEMANTICS.md` covers
(named, default, namespace, side-effect, `export … from`, `export *`,
`export * as`, `require()`, `import()`), with cycles, top-level names drawn
from a small pool so modules collide in the bundle's scope, live bindings
mutated across modules, and reads that may land in the TDZ. Every module logs
as it runs and probes what it imported, so order, values, `.name`s, and errors
reach stdout. A seed the bundler refuses counts as a pass only if Node also
fails to link the program, or the refusal is one the contract lists.

Its runs found eight bundler bugs, all fixed and covered by fixtures. The
first three, in its first minutes:
an `import()` of the entry while its top-level `await` was pending settled at
once, where Node waits for the entry (and so deadlocks, like the program); a
lazily imported module that depends on such an entry ran at once, where Node
makes it wait (now refused, `E-TLA`); and the oracle compared the file and
line in Node's "unsettled top-level await" warning, which name the bundle
instead of the program.

The long runs found two more. A lazy module's namespace getters were
written inside its closure, where a re-exported binding's bundle name could
resolve to one of the module's own (`lazy-namespace-capture`, seed
1002770). And a lazy module was linked only when it was entered, so a
module that ran first read another's re-exported function through a
namespace as `undefined` where ECMA-262 has already made it at link time;
lazy modules are now generators, linked as a graph before any runs
(`lazy-link-before-evaluate`, seeds 2034477, 2040152, 2041121, 2045733).

A sixth was in the first long run's log as well (seed 1015623) and was
found again by the third (seed 3028481): a namespace held a name that two
`export *` reached through two `export * as` declarations of one module's
namespace. ECMA-262 calls that one binding, V8 gives each declaration its
own, and the name is ambiguous under Node. The linker now resolves as V8
does (`esm-star-namespace-cells`), and its test262 counterparts, which the
runner had filed as Node failures while the bundle passed them, now fail
as Node fails them.

The fourth long run found the seventh (seed 4191301): a namespace that
lacked a name Node gave it. The name was ambiguous, through `export *`
declarations that form a cycle, and V8 builds namespaces from export
tables it fills in as it goes, so whichever namespace in the cycle it
builds second has the name. That depends on the order of imports and on
when each `import()` runs, so the bundler now refuses it
(`refuse-star-cycle-ambiguous`).

The fifth found the eighth (seed 6066569), with no cycle involved: a name
one `export *` module found ambiguous, and that a module above it also got
from another branch. ECMA-262 makes the name ambiguous for every module
above the first; V8 leaves it out of that module's export table, so the
module above has it from the other branch. Namespaces are now built by
V8's algorithm (`esm-namespace-nested-ambiguous`).

`bundle_source` found one: a stack overflow on 558 nested divisions. The
parallel build ran on threads with Rust's default 2 MiB stack; they now
get 256 MiB, and a module nested deeper than the bundler's passes can take
is refused (`refuse-deep-nesting`).

### The last runs

On `98badfc`, the last commit to change the bundler, both targets ran for
two hours at once, on the 8-thread laptop in
[`benchmarks/BENCHMARKS.md`](../benchmarks/BENCHMARKS.md): the generator on
four jobs, `bundle_source` on two workers.

| Target | Ran | Result |
| ------ | --- | ------ |
| generated graphs | 157,547 programs, seeds 9,000,000 to 9,157,546, in 7,200 s | 149,884 equivalent, 7,663 refused (7,606 `E-TLA`, 57 `E-LINK`), 0 diverged |
| `bundle_source` | 782,111 inputs (388,923 and 393,188), in 7,201 s | no crash, leak, or timeout; the corpus grew to 13,203 inputs |

Every refused program is one Node runs, refused under a rule the contract
lists. Replaying the same seeds with `--save-refused` gave the same counts
and kept each one: 5,002 have top-level `await` in an entry that is in an
import cycle and 2,604 import a module lazily that depends on an entry that
awaits (rule 11), and all 57 `E-LINK` are a namespace with an ambiguous
name over a cycle of `export *` (rule 4).

A review then changed how the module graph resolves package subpaths,
directory specifiers, and the JSX runtime (`fe6f372`, `4ad9685`). On
`1c4ab54`, after those, both targets ran for 30 minutes more: 43,292
generated programs (seeds 10,000,000 to 10,043,291), 41,215 equivalent,
2,077 refused, 0 diverged; and 127,271 `bundle_source` inputs (63,336
and 63,935) with no crash. A replay
with `--save-refused` gave the same counts, and every refusal is again
one the contract lists: 1,323 top-level `await` in an entry in a cycle,
741 lazy imports of a module that depends on an entry that awaits, and
13 ambiguous names over a cycle of `export *`.

`artifacts/bundle_source` holds no crash. It keeps one slow input from an
earlier run, 18 bytes that take 153 ms on their own and passed libFuzzer's
10-second slow-input mark only while the machine was loaded.

