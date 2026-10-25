# Node's tests

Node's own tests (`test/parallel` from Node v24.19.0), run unmodified by `opal run`, by [`node-tests.py`](./node-tests.py) on 2026-10-25 (opal 0.4.1). A file passes when opal exits 0 within 60 s, as Node's own runner requires. Every file first runs under node as a control; one that fails there is listed but not counted.

The files are those for the areas the runtime implements (the selection below). `test/common` runs as Node ships it except for what it needs as it loads that Opal doesn't have yet: globals it lists by bare name are read through `globalThis`, and its requires of modules Opal lacks go to a stand-in that gives it what it reads from them while loading and makes any other use an error naming the module. The edits, all of them:

- common/index.js: known global AbortController -> globalThis.AbortController
- common/index.js: known global structuredClone -> globalThis.structuredClone
- common/index.js: known global fetch -> globalThis.fetch
- common/assertSnapshot.js: require('url') -> the stand-in
- common/benchmark.js: require('child_process') -> the stand-in
- common/child_process.js: require('child_process') -> the stand-in
- common/crypto.js: require('child_process') -> the stand-in
- common/debugger.js: require('child_process') -> the stand-in
- common/dns.js: require('net') -> the stand-in
- common/fixtures.js: require('url') -> the stand-in
- common/index.js: require('child_process') -> the stand-in
- common/index.js: require('net') -> the stand-in
- common/index.js: require('worker_threads') -> the stand-in
- common/inspector-helper.js: require('child_process') -> the stand-in
- common/inspector-helper.js: require('url') -> the stand-in
- common/net.js: require('net') -> the stand-in
- common/proxy-server.js: require('child_process') -> the stand-in
- common/proxy-server.js: require('net') -> the stand-in
- common/report.js: require('net') -> the stand-in
- common/require-as.js: require('child_process') -> the stand-in
- common/require-as.js: require('worker_threads') -> the stand-in
- common/snapshot.js: require('child_process') -> the stand-in
- common/tls.js: require('net') -> the stand-in
- common/tmpdir.js: require('child_process') -> the stand-in
- common/tmpdir.js: require('url') -> the stand-in
- common/tmpdir.js: require('worker_threads') -> the stand-in
- common/v8-max-heap-size-option.js: require('child_process') -> the stand-in
- common/watch.js: require('child_process') -> the stand-in
- common/wpt.js: require('worker_threads') -> the stand-in

## By area

| Area | Pass | Fail | Time out | Pass rate |
|---|---|---|---|---|
| `test-assert*.js` | 4 | 15 | 0 | 21% |
| `test-buffer-*.js` | 53 | 16 | 0 | 77% |
| `test-console-*.js` | 5 | 16 | 0 | 24% |
| `test-event-emitter-*.js` | 21 | 5 | 0 | 81% |
| `test-events-*.js` | 3 | 5 | 0 | 38% |
| `test-fs-*.js` | 38 | 218 | 0 | 15% |
| `test-module-*.js` | 8 | 23 | 0 | 26% |
| `test-next-tick*.js` | 8 | 1 | 0 | 89% |
| `test-path*.js` | 15 | 2 | 0 | 88% |
| `test-process-*.js` | 17 | 70 | 0 | 20% |
| `test-require-*.js` | 12 | 9 | 0 | 57% |
| `test-timers*.js` | 31 | 27 | 0 | 53% |
| `test-util-format*.js` | 1 | 0 | 0 | 100% |
| `test-util-inspect*.js` | 2 | 3 | 0 | 40% |
| `test-util-types*.js` | 1 | 1 | 0 | 50% |
| `test-util.js` | 0 | 1 | 0 | 0% |
| **all** | **219** | **412** | **0** | **35%** |

10 more files fail under node on the machine that ran them, so they say nothing about Opal and aren't counted.

## By file

| File | Opal | What failed |
|---|---|---|
| `test-assert-async.js` | pass |  |
| `test-assert-calltracker-calls.js` | pass |  |
| `test-assert-calltracker-getCalls.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-calltracker-report.js` | pass |  |
| `test-assert-calltracker-verify.js` | pass |  |
| `test-assert-checktag.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-class-destructuring.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-class.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-deep-with-error.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-deep.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-esm-cjs-message-verify.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-fail-deprecation.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-fail.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-first-line.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-if-error.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert-myers-diff.js` | **fail** | Error: Cannot find module 'internal/assert/myers_diff' |
| `test-assert-partial-deep-equal.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-assert-typedarray-deepequal.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-assert.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-buffer-alloc-unsafe-is-initialized-with-zero-fill-flag.js` | pass |  |
| `test-buffer-alloc-unsafe-is-uninitialized.js` | pass |  |
| `test-buffer-alloc.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-buffer-arraybuffer.js` | pass |  |
| `test-buffer-ascii.js` | pass |  |
| `test-buffer-backing-arraybuffer.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-buffer-badhex.js` | pass |  |
| `test-buffer-bigint64.js` | pass |  |
| `test-buffer-bytelength.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-buffer-compare-offset.js` | pass |  |
| `test-buffer-compare.js` | pass |  |
| `test-buffer-concat.js` | pass |  |
| `test-buffer-constants.js` | pass |  |
| `test-buffer-constructor-deprecation-error.js` | pass |  |
| `test-buffer-constructor-node-modules-paths.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-buffer-constructor-node-modules.js` | **fail** | Error: child_process.spawnSync: node:child_process is not in Opal yet |
| `test-buffer-constructor-outside-node-modules.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-buffer-copy.js` | pass |  |
| `test-buffer-equals.js` | pass |  |
| `test-buffer-failed-alloc-typed-arrays.js` | pass |  |
| `test-buffer-fakes.js` | pass |  |
| `test-buffer-fill.js` | **fail** | Error: Cannot find module 'internal/errors' |
| `test-buffer-from.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-buffer-generic-methods.js` | pass |  |
| `test-buffer-includes.js` | pass |  |
| `test-buffer-indexof.js` | pass |  |
| `test-buffer-inheritance.js` | pass |  |
| `test-buffer-inspect.js` | pass |  |
| `test-buffer-isascii.js` | **fail** | TypeError: TextEncoder is not a constructor |
| `test-buffer-isencoding.js` | pass |  |
| `test-buffer-isutf8-isascii-fast.js` | **fail** | SyntaxError: Unexpected token '%' |
| `test-buffer-isutf8.js` | **fail** | TypeError: TextEncoder is not a constructor |
| `test-buffer-iterator.js` | pass |  |
| `test-buffer-new.js` | pass |  |
| `test-buffer-no-negative-allocation.js` | pass |  |
| `test-buffer-nopendingdep-map.js` | pass |  |
| `test-buffer-of-no-deprecation.js` | pass |  |
| `test-buffer-over-max-length.js` | pass |  |
| `test-buffer-parent-property.js` | pass |  |
| `test-buffer-pending-deprecation.js` | pass |  |
| `test-buffer-pool-untransferable.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-buffer-prototype-inspect.js` | pass |  |
| `test-buffer-read.js` | pass |  |
| `test-buffer-readdouble.js` | pass |  |
| `test-buffer-readfloat.js` | pass |  |
| `test-buffer-readint.js` | pass |  |
| `test-buffer-readuint.js` | pass |  |
| `test-buffer-resizable.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-buffer-safe-unsafe.js` | pass |  |
| `test-buffer-set-inspect-max-bytes.js` | pass |  |
| `test-buffer-sharedarraybuffer.js` | pass |  |
| `test-buffer-slice.js` | pass |  |
| `test-buffer-slow.js` | pass |  |
| `test-buffer-swap-fast.js` | **fail** | SyntaxError: Unexpected token '%' |
| `test-buffer-swap.js` | pass |  |
| `test-buffer-tojson.js` | pass |  |
| `test-buffer-tostring-4gb.js` | pass |  |
| `test-buffer-tostring-range.js` | pass |  |
| `test-buffer-tostring-rangeerror.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-buffer-tostring.js` | pass |  |
| `test-buffer-write-fast.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-buffer-write.js` | pass |  |
| `test-buffer-writedouble.js` | pass |  |
| `test-buffer-writefloat.js` | pass |  |
| `test-buffer-writeint.js` | pass |  |
| `test-buffer-writeuint.js` | pass |  |
| `test-buffer-zero-fill-cli.js` | pass |  |
| `test-buffer-zero-fill-reset.js` | pass |  |
| `test-buffer-zero-fill.js` | pass |  |
| `test-console-assign-undefined.js` | pass |  |
| `test-console-async-write-error.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-clear.js` | **fail** | TypeError: Cannot set property isTTY of #<WriteStream> which has only a getter |
| `test-console-count.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-console-diagnostics-channels.js` | **fail** | Error: Opal does not implement the built-in module node:diagnostics_channel yet |
| `test-console-formatTime.js` | **fail** | Error: Cannot find module 'internal/util/debuglog' |
| `test-console-group.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-instance.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-issue-43095.js` | pass |  |
| `test-console-log-stdio-broken-dest.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-log-throw-primitive.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-methods.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-no-swallow-stack-overflow.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-not-call-toString.js` | pass |  |
| `test-console-self-assign.js` | pass |  |
| `test-console-stdio-setters.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-sync-write-error.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-table.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-tty-colors-per-stream.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-tty-colors.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-console-with-frozen-intrinsics.js` | pass |  |
| `test-event-emitter-add-listeners.js` | pass |  |
| `test-event-emitter-check-listener-leaks.js` | pass |  |
| `test-event-emitter-emit-context.js` | pass |  |
| `test-event-emitter-error-monitor.js` | **fail** | Error: MyError |
| `test-event-emitter-errors.js` | pass |  |
| `test-event-emitter-get-max-listeners.js` | pass |  |
| `test-event-emitter-invalid-listener.js` | pass |  |
| `test-event-emitter-listener-count.js` | pass |  |
| `test-event-emitter-listeners-side-effects.js` | pass |  |
| `test-event-emitter-listeners.js` | pass |  |
| `test-event-emitter-max-listeners-warning-for-null.js` | pass |  |
| `test-event-emitter-max-listeners-warning-for-symbol.js` | pass |  |
| `test-event-emitter-max-listeners-warning.js` | pass |  |
| `test-event-emitter-max-listeners.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-event-emitter-method-names.js` | pass |  |
| `test-event-emitter-modify-in-emit.js` | pass |  |
| `test-event-emitter-no-error-provided-to-error-event.js` | **fail** | Error: Opal does not implement the built-in module node:domain yet |
| `test-event-emitter-num-args.js` | pass |  |
| `test-event-emitter-once.js` | pass |  |
| `test-event-emitter-prepend.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-event-emitter-remove-all-listeners.js` | pass |  |
| `test-event-emitter-remove-listeners.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-event-emitter-set-max-listeners-side-effects.js` | pass |  |
| `test-event-emitter-special-event-names.js` | pass |  |
| `test-event-emitter-subclass.js` | pass |  |
| `test-event-emitter-symbols.js` | pass |  |
| `test-events-customevent.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-events-getmaxlisteners.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/event_target, which Opal doesn't have |
| `test-events-list.js` | pass |  |
| `test-events-listener-count-with-listener.js` | pass |  |
| `test-events-on-async-iterator.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-events-once.js` | **fail** | Error: kaboom |
| `test-events-static-geteventlisteners.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-events-uncaught-exception-stack.js` | pass |  |
| `test-fs-access.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-append-file-flush.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-append-file-sync.js` | pass |  |
| `test-fs-append-file.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-assert-encoding-error.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-fs-buffer.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-buffertype-writesync.js` | pass |  |
| `test-fs-chmod-mask.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-chmod.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-chown-negative-one.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-chown-type-check.js` | pass |  |
| `test-fs-close-errors.js` | pass |  |
| `test-fs-close.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-constants.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-fs-copyfile-respect-permissions.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-copyfile.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-cp-sync-dereference.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/cp/cp, which Opal doesn't have |
| `test-fs-empty-readStream.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-error-messages.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-exists.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-fs-existssync-false.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-existssync-memleak-longpath.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-fchmod.js` | pass |  |
| `test-fs-fchown-negative-one.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-fchown.js` | pass |  |
| `test-fs-filehandle-use-after-close.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-filehandle.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-fmap.js` | pass |  |
| `test-fs-fsync.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-internal-assertencoding.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/dir, which Opal doesn't have |
| `test-fs-lchmod.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-lchown-negative-one.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-lchown.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-link.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-long-path.js` | pass |  |
| `test-fs-make-callback.js` | pass |  |
| `test-fs-makeStatsCallback.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-mkdir-mode-mask.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-mkdir-recursive-eaccess.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-mkdir-rmdir.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-mkdir.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-mkdtemp-prefix-check.js` | pass |  |
| `test-fs-mkdtemp.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-mkdtempDisposableSync.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-negative-zero.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-non-number-arguments-throw.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-null-bytes.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-fs-open-flags.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-open-mode-mask.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-open-no-close.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-open-numeric-flags.js` | pass |  |
| `test-fs-open.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-opendir.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/dir, which Opal doesn't have |
| `test-fs-operations-with-surrogate-pairs.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-options-immutable.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback |
| `test-fs-promises-exists.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-promises-file-handle-aggregate-errors.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-promises-file-handle-append-file.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-chmod.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-close-errors.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-promises-file-handle-close.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-dispose.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/dir, which Opal doesn't have |
| `test-fs-promises-file-handle-op-errors.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-promises-file-handle-read-worker.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-promises-file-handle-read.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-readFile.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-stat.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-stream.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-sync.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-truncate.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-write.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-file-handle-writeFile.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-mkdtempDisposable.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-promises-readfile-buffer-option.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-readfile-empty.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-readfile-with-fd.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-readfile.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-statfs-validate-path.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-watch-iterator.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-promises-watch.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-promises-write-optional-params.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-writefile-typedarray.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-writefile-with-fd.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises-writefile.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promises.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-promisified.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-read-empty-buffer.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-read-file-assert-encoding.js` | pass |  |
| `test-fs-read-file-sync-hostname.js` | pass |  |
| `test-fs-read-file-sync.js` | pass |  |
| `test-fs-read-offset-null.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-read-optional-params.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-read-promises-optional-params.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-read-stream-autoClose.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-concurrent-reads.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-double-close.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-encoding.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-fs-read-stream-err.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-fd-leak.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-fd.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-file-handle.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-read-stream-inherit.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-patch-open.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-pos.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-resume.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream-throw-type-error.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-read-stream.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-read-type.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-read-zero-length.js` | pass |  |
| `test-fs-read.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-readSync-optional-params.js` | pass |  |
| `test-fs-readdir-buffer.js` | pass |  |
| `test-fs-readdir-pipe.js` | pass |  |
| `test-fs-readdir-recursive.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-readdir-stack-overflow.js` | pass |  |
| `test-fs-readdir-types-symlinks.js` | pass |  |
| `test-fs-readdir-types.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-readdir-ucs2.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-readdir.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-readfile-buffer-option.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-readfile-empty.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback |
| `test-fs-readfile-eof.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-readfile-error.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-readfile-fd.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-readfile-flags.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback |
| `test-fs-readfile-pipe-large.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-readfile-pipe.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-readfile-unlink.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback |
| `test-fs-readfile-zero-byte-liar.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback |
| `test-fs-readfile.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback |
| `test-fs-readfilesync-enoent.js` | pass |  |
| `test-fs-readfilesync-pipe-large.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-readlink-type-check.js` | pass |  |
| `test-fs-readv-promises.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-readv-promisify.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-readv-sync.js` | pass |  |
| `test-fs-readv.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-ready-event-stream.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-realpath-buffer-encoding.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-realpath-native.js` | (fails under node) |  |
| `test-fs-realpath-on-substed-drive.js` | pass |  |
| `test-fs-realpath-pipe.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-realpath.js` | (fails under node) |  |
| `test-fs-rename-type-check.js` | pass |  |
| `test-fs-rm.js` | **fail** | Error: Opal does not implement the built-in module node:url yet |
| `test-fs-rmSync-special-char.js` | pass |  |
| `test-fs-rmdir-recursive-sync-warns-not-found.js` | pass |  |
| `test-fs-rmdir-recursive-sync-warns-on-file.js` | pass |  |
| `test-fs-rmdir-recursive-throws-not-found.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-rmdir-recursive-throws-on-file.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-rmdir-recursive-warns-not-found.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-rmdir-recursive-warns-on-file.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-rmdir-recursive.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-rmdir-type-check.js` | pass |  |
| `test-fs-sir-writes-alot.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-stat-abort-test.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-stat-bigint.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-stat.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-statfs.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-stream-construct-compat-error-read.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-construct-compat-error-write.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-construct-compat-graceful-fs.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-construct-compat-old-node.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-destroy-emit-error.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-double-close.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-fs-options.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-stream-options.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-fs-symlink-buffer-path.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-symlink-dir-junction-relative.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-symlink-dir-junction.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-symlink-dir.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-symlink-longpath.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-symlink.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-sync-fd-leak.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-syncwritestream.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-timestamp-parsing-error.js` | pass |  |
| `test-fs-truncate-clear-file-zero.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-truncate-sync.js` | pass |  |
| `test-fs-truncate.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-unlink-type-check.js` | pass |  |
| `test-fs-util-validateoffsetlength.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-utils-get-dirents.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-utimes-y2K38.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-utimes.js` | **fail** | Error: Opal does not implement the built-in module node:url yet |
| `test-fs-watch-abort-signal.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-close-when-destroyed.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-encoding.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-enoent.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-file-enoent-after-deletion.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/watchers, which Opal doesn't have |
| `test-fs-watch-ignore-function.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-glob.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-invalid.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-mixed.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-recursive-glob-subdirectories.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-recursive-glob.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-recursive-mixed.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-recursive-regexp.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-ignore-regexp.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-recursive-add-file-to-existing-subfolder.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-add-file-to-new-folder.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-add-file-with-url.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-recursive-add-file.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-add-folder.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-recursive-assert-leaks.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-recursive-delete-race.js` | **fail** | Error: Cannot find module 'internal/fs/watchers' |
| `test-fs-watch-recursive-delete.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-linux-parallel-remove.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-watch-recursive-prefix-sibling.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-promise.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-symlink.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-fs-watch-recursive-sync-write.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-update-file.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-validate.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-recursive-watch-file.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-ref-unref.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watch-stop-async.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/watchers, which Opal doesn't have |
| `test-fs-watch-stop-sync.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/watchers, which Opal doesn't have |
| `test-fs-watch.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-fs-watchfile-bigint.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-watchfile-ref-unref.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/watchers, which Opal doesn't have |
| `test-fs-watchfile.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/watchers, which Opal doesn't have |
| `test-fs-whatwg-url.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-write-buffer-large.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-buffer.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-file-buffer.js` | pass |  |
| `test-fs-write-file-flush.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-write-file-invalid-path.js` | pass |  |
| `test-fs-write-file-sync.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-write-file-typedarrays.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-file.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-negativeoffset.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-no-fd.js` | pass |  |
| `test-fs-write-optional-params.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-reuse-callback.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-write-sigxfsz.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-write-stream-autoclose-option.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-change-open.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-close-without-callback.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-double-close.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-encoding.js` | **fail** | Error: Opal does not implement the built-in module node:stream yet |
| `test-fs-write-stream-end.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-err.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-file-handle-2.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-write-stream-file-handle.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-write-stream-flush.js` | **fail** | Error: Opal does not implement the built-in module node:fs/promises yet |
| `test-fs-write-stream-fs.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream-patch-open.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-write-stream-throw-type-error.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-stream.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-write-sync-optional-params.js` | pass |  |
| `test-fs-write-sync.js` | pass |  |
| `test-fs-write.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected "actual" to be strictly unequal to: |
| `test-fs-writefile-with-fd.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-fs-writestream-open-write.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/streams, which Opal doesn't have |
| `test-fs-writesync-crash.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-fs-writev-promises.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/promises, which Opal doesn't have |
| `test-fs-writev-sync.js` | pass |  |
| `test-fs-writev.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-module-builtin.js` | pass |  |
| `test-module-cache.js` | **fail** | Error: Cannot find module '/tmp/opal-node-tests-5xhfx31k/opal/test/.tmp.test-module-cache.js/test-module-cache.json' |
| `test-module-children.js` | **fail** | Error: Opal does not implement the built-in module node:sys yet |
| `test-module-circular-dependency-warning.js` | **fail** |  |
| `test-module-circular-symlinks.js` | pass |  |
| `test-module-create-require-multibyte.js` | **fail** | Error: url.pathToFileURL: node:url is not in Opal yet |
| `test-module-create-require.js` | **fail** | Error: url.pathToFileURL: node:url is not in Opal yet |
| `test-module-globalpaths-nodepath.js` | **fail** | TypeError: mod._initPaths is not a function |
| `test-module-isBuiltin.js` | pass |  |
| `test-module-loading-deprecated.js` | pass |  |
| `test-module-loading-error.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-loading-globalpaths.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-main-extension-lookup.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-main-fail.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-main-preserve-symlinks-fail.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-multi-extensions.js` | **fail** | Error: Cannot find module '/tmp/opal-node-tests-5xhfx31k/opal/test/.tmp.test-module-multi-extensions.js/test-extensions' |
| `test-module-nodemodulepaths.js` | pass |  |
| `test-module-parent-deprecation.js` | **fail** |  |
| `test-module-parent-setter-deprecation.js` | **fail** |  |
| `test-module-prototype-mutation.js` | **fail** | Error: Opal does not implement the built-in module node:diagnostics_channel yet |
| `test-module-readonly.js` | pass |  |
| `test-module-relative-lookup.js` | **fail** | TypeError: _module._resolveLookupPaths is not a function |
| `test-module-run-main-monkey-patch.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-setsourcemapssupport.js` | **fail** | AssertionError [ERR_ASSERTION]: The input did not match the regular expression /ERR_INVALID_ARG_TYPE/. Input: |
| `test-module-stat.js` | **fail** | TypeError: Module._stat is not a function |
| `test-module-strip-types.js` | pass |  |
| `test-module-subpath-import-long-path.js` | pass |  |
| `test-module-symlinked-peer-modules.js` | **fail** | Error: Cannot find module 'moduleB' |
| `test-module-version.js` | **fail** | AssertionError [ERR_ASSERTION]: The expression evaluated to a falsy value: |
| `test-module-wrap.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-module-wrapper.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-next-tick-doesnt-hang.js` | pass |  |
| `test-next-tick-domain.js` | **fail** | Error: Opal does not implement the built-in module node:domain yet |
| `test-next-tick-errors.js` | pass |  |
| `test-next-tick-fixed-queue-regression.js` | pass |  |
| `test-next-tick-intentional-starvation.js` | pass |  |
| `test-next-tick-ordering.js` | pass |  |
| `test-next-tick-ordering2.js` | pass |  |
| `test-next-tick-when-exiting.js` | pass |  |
| `test-next-tick.js` | pass |  |
| `test-path-basename.js` | pass |  |
| `test-path-dirname.js` | pass |  |
| `test-path-extname.js` | pass |  |
| `test-path-glob.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/glob, which Opal doesn't have |
| `test-path-isabsolute.js` | pass |  |
| `test-path-join.js` | pass |  |
| `test-path-makelong.js` | pass |  |
| `test-path-normalize.js` | pass |  |
| `test-path-parse-format.js` | pass |  |
| `test-path-posix-exists.js` | pass |  |
| `test-path-posix-relative-on-windows.js` | pass |  |
| `test-path-relative.js` | pass |  |
| `test-path-resolve.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-path-win32-exists.js` | pass |  |
| `test-path-win32-normalize-device-names.js` | pass |  |
| `test-path-zero-length-strings.js` | pass |  |
| `test-path.js` | pass |  |
| `test-process-abort.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-argv-0.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-available-memory.js` | **fail** | TypeError: process.availableMemory is not a function |
| `test-process-beforeexit-throw-exit.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-beforeexit.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-process-binding-internalbinding-allowlist.js` | **fail** | TypeError: process.binding is not a function |
| `test-process-binding-util.js` | **fail** | TypeError: process.binding is not a function |
| `test-process-binding.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-process-chdir-errormessage.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-chdir.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-config.js` | **fail** | AssertionError [ERR_ASSERTION]: Missing expected exception (TypeError). |
| `test-process-constants-noatime.js` | pass |  |
| `test-process-constrained-memory.js` | **fail** | TypeError: process.constrainedMemory is not a function |
| `test-process-cpuUsage.js` | **fail** | TypeError: process.cpuUsage is not a function |
| `test-process-default.js` | pass |  |
| `test-process-dlopen-error-message-crash.js` | (fails under node) |  |
| `test-process-dlopen-undefined-exports.js` | pass |  |
| `test-process-domain-segfault.js` | pass |  |
| `test-process-emit.js` | pass |  |
| `test-process-emitwarning.js` | **fail** | Error: invalid toString |
| `test-process-env-allowed-flags-are-documented.js` | (fails under node) |  |
| `test-process-env-allowed-flags.js` | **fail** | TypeError: Cannot read properties of undefined (reading 'has') |
| `test-process-env-delete.js` | pass |  |
| `test-process-env-deprecation.js` | **fail** |  |
| `test-process-env-ignore-getter-setter.js` | **fail** | AssertionError [ERR_ASSERTION]: Missing expected exception (TypeError). |
| `test-process-env-sideeffects.js` | pass |  |
| `test-process-env-symbols.js` | **fail** | AssertionError [ERR_ASSERTION]: Missing expected exception (TypeError). |
| `test-process-env-tz.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-env-windows-error-reset.js` | pass |  |
| `test-process-env.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-euid-egid.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-exception-capture-errors.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-process-exception-capture-should-abort-on-uncaught-setflagsfromstring.js` | **fail** | Error: Opal does not implement the built-in module node:v8 yet |
| `test-process-exception-capture-should-abort-on-uncaught.js` | **fail** | TypeError: process.hasUncaughtExceptionCaptureCallback is not a function |
| `test-process-exception-capture.js` | **fail** | TypeError: process.hasUncaughtExceptionCaptureCallback is not a function |
| `test-process-exec-argv.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-execpath.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-execve-abort.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-no-args.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-on-exit.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-permission-fail.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-permission-granted.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-socket.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-validation.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve-worker-threads.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-execve.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-exit-code-validation.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-exit-code.js` | **fail** | TypeError: require(...).debuglog is not a function |
| `test-process-exit-from-before-exit.js` | pass |  |
| `test-process-exit-handler.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-exit-recursive.js` | pass |  |
| `test-process-exit.js` | pass |  |
| `test-process-external-stdio-close-spawn.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-external-stdio-close.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-features.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-process-getactivehandles.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-process-getactiverequests.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs functions with a callback or promise |
| `test-process-getactiveresources-track-active-handles.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-process-getactiveresources-track-active-requests.js` | (fails under node) |  |
| `test-process-getactiveresources-track-interval-lifetime.js` | **fail** | TypeError: process.getActiveResourcesInfo is not a function |
| `test-process-getactiveresources-track-multiple-timers.js` | **fail** | TypeError: process.getActiveResourcesInfo is not a function |
| `test-process-getactiveresources-track-timer-lifetime.js` | **fail** | TypeError: process.getActiveResourcesInfo is not a function |
| `test-process-getactiveresources.js` | (fails under node) |  |
| `test-process-getgroups.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-hrtime-bigint.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-process-hrtime.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-process-initgroups.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-kill-null.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-kill-pid.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-process-load-env-file.js` | (fails under node) |  |
| `test-process-next-tick.js` | pass |  |
| `test-process-no-deprecation.js` | pass |  |
| `test-process-ppid.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-prototype.js` | pass |  |
| `test-process-raw-debug.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-really-exit.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-redirect-warnings-env.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-redirect-warnings.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-ref-unref.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-process-release.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-process-remove-all-signal-listeners.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-setgroups.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-setsourcemapsenabled.js` | **fail** | AssertionError [ERR_ASSERTION]: The input did not match the regular expression /ERR_INVALID_ARG_TYPE/. Input: |
| `test-process-threadCpuUsage-main-thread.js` | **fail** | TypeError: process.threadCpuUsage is not a function |
| `test-process-threadCpuUsage-worker-threads.js` | pass |  |
| `test-process-title-cli.js` | **fail** | Error: Opal does not implement the built-in module node:os yet |
| `test-process-uid-gid.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-umask-mask.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-umask.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-uncaught-exception-monitor.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-uptime.js` | pass |  |
| `test-process-versions.js` | (fails under node) |  |
| `test-process-warning.js` | pass |  |
| `test-require-cache.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected "actual" to be reference-equal to "expected": |
| `test-require-delete-array-iterator.js` | **fail** | Error: url.pathToFileURL: node:url is not in Opal yet |
| `test-require-dot.js` | **fail** | TypeError: m._initPaths is not a function |
| `test-require-empty-main.js` | (fails under node) |  |
| `test-require-enoent-dir.js` | pass |  |
| `test-require-exceptions.js` | pass |  |
| `test-require-extension-over-directory.js` | pass |  |
| `test-require-extensions-main.js` | pass |  |
| `test-require-extensions-same-filename-as-dir-trailing-slash.js` | pass |  |
| `test-require-extensions-same-filename-as-dir.js` | pass |  |
| `test-require-invalid-main-no-exports.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-require-invalid-package.js` | pass |  |
| `test-require-json.js` | pass |  |
| `test-require-long-path.js` | pass |  |
| `test-require-mjs.js` | **fail** | AssertionError [ERR_ASSERTION]: Missing expected exception. |
| `test-require-node-prefix.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected "actual" to be reference-equal to "expected": |
| `test-require-nul.js` | pass |  |
| `test-require-process.js` | pass |  |
| `test-require-resolve-invalid-paths.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-require-resolve-opts-paths-relative.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-require-resolve.js` | (fails under node) |  |
| `test-require-symlink.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-require-unicode.js` | pass |  |
| `test-timers-api-refs.js` | pass |  |
| `test-timers-args.js` | pass |  |
| `test-timers-clear-null-does-not-throw-error.js` | pass |  |
| `test-timers-clear-object-does-not-throw-error.js` | pass |  |
| `test-timers-clear-timeout-interval-equivalent.js` | pass |  |
| `test-timers-clearImmediate-als.js` | **fail** | Error: Opal does not implement the built-in module node:async_hooks yet |
| `test-timers-clearImmediate.js` | pass |  |
| `test-timers-destroyed.js` | pass |  |
| `test-timers-dispose.js` | pass |  |
| `test-timers-fast-calls.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-timers-immediate-promisified.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-timers-immediate-queue-throw.js` | **fail** | Error: Opal does not implement the built-in module node:domain yet |
| `test-timers-immediate-queue.js` | pass |  |
| `test-timers-immediate-unref-nested-once.js` | pass |  |
| `test-timers-immediate-unref-simple.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-timers-immediate-unref.js` | pass |  |
| `test-timers-immediate.js` | pass |  |
| `test-timers-interval-promisified.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-timers-interval-throw.js` | pass |  |
| `test-timers-invalid-clear.js` | pass |  |
| `test-timers-linked-list.js` | **fail** | Error: Cannot find module 'internal/linkedlist' |
| `test-timers-max-duration-warning.js` | pass |  |
| `test-timers-nan-duration-emit-once-per-process.js` | **fail** |  |
| `test-timers-nan-duration-warning-promises.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-timers-nan-duration-warning.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-timers-negative-duration-warning-emit-once-per-process.js` | **fail** |  |
| `test-timers-negative-duration-warning.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-timers-nested.js` | **fail** | Error: Cannot find module 'internal/util' |
| `test-timers-next-tick.js` | **fail** | Error: Cannot find module 'internal/util' |
| `test-timers-non-integer-delay.js` | **fail** | AssertionError [ERR_ASSERTION]: Non-integer delay ordering should be 1,2,3,4, but got 1,4,3,2 |
| `test-timers-not-emit-duration-zero.js` | pass |  |
| `test-timers-now.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-timers-ordering.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-timers-process-tampering.js` | pass |  |
| `test-timers-promises-scheduler.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-timers-promises.js` | **fail** | Error: Opal does not implement the built-in module node:timers/promises yet |
| `test-timers-refresh-in-callback.js` | **fail** |  |
| `test-timers-refresh.js` | **fail** | Error: Cannot find module 'internal/timers' |
| `test-timers-reset-process-domain-on-throw.js` | **fail** | Error: Opal does not implement the built-in module node:domain yet |
| `test-timers-same-timeout-wrong-list-deleted.js` | pass |  |
| `test-timers-setimmediate-infinite-loop.js` | pass |  |
| `test-timers-socket-timeout-removes-other-socket-unref-timer.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-timers-this.js` | pass |  |
| `test-timers-throw-when-cb-not-function.js` | pass |  |
| `test-timers-timeout-promisified.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-timers-timeout-to-interval.js` | **fail** |  |
| `test-timers-timeout-with-non-integer.js` | pass |  |
| `test-timers-to-primitive.js` | pass |  |
| `test-timers-uncaught-exception.js` | pass |  |
| `test-timers-unenroll-unref-interval.js` | **fail** | TypeError: Function.prototype.apply was called on null, which is null and not a function |
| `test-timers-unref-throw-then-ref.js` | pass |  |
| `test-timers-unref.js` | pass |  |
| `test-timers-unrefd-interval-still-fires.js` | pass |  |
| `test-timers-unrefed-in-beforeexit.js` | pass |  |
| `test-timers-unrefed-in-callback.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-timers-user-call.js` | pass |  |
| `test-timers-zero-timeout.js` | pass |  |
| `test-timers.js` | pass |  |
| `test-util-format.js` | pass |  |
| `test-util-inspect-getters-accessing-this.js` | pass |  |
| `test-util-inspect-long-running.js` | pass |  |
| `test-util-inspect-namespace.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-util-inspect-proxy.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-util-inspect.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-util-types-exists.js` | pass |  |
| `test-util-types.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-util.js` | **fail** | Error: Cannot find module 'internal/errors' |
