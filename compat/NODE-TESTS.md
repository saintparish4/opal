# Node's tests

Node's own tests (`test/parallel` from Node v24.19.0), run unmodified by `opal run`, by [`node-tests.py`](./node-tests.py) on 2026-10-26 (opal 0.4.1). A file passes when opal exits 0 within 60 s, as Node's own runner requires. Every file first runs under node as a control; one that fails there is listed but not counted.

The files are those for the areas the runtime implements (the selection below). `test/common` runs as Node ships it except for what it needs as it loads that Opal doesn't have yet: globals it lists by bare name are read through `globalThis`, and its requires of modules Opal lacks go to a stand-in that gives it what it reads from them while loading and makes any other use an error naming the module. The edits, all of them:

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
| `test-abort*.js` | 0 | 5 | 0 | 0% |
| `test-assert*.js` | 4 | 15 | 0 | 21% |
| `test-async-local-storage*.js` | 6 | 5 | 0 | 55% |
| `test-buffer-*.js` | 54 | 15 | 0 | 78% |
| `test-console-*.js` | 5 | 16 | 0 | 24% |
| `test-event-emitter-*.js` | 25 | 1 | 0 | 96% |
| `test-event-target*.js` | 1 | 0 | 0 | 100% |
| `test-eventtarget*.js` | 2 | 3 | 0 | 40% |
| `test-events-*.js` | 5 | 3 | 0 | 62% |
| `test-fs-*.js` | 162 | 94 | 0 | 63% |
| `test-module-*.js` | 8 | 23 | 0 | 26% |
| `test-next-tick*.js` | 8 | 1 | 0 | 89% |
| `test-os*.js` | 3 | 5 | 0 | 38% |
| `test-path*.js` | 15 | 2 | 0 | 88% |
| `test-process-*.js` | 17 | 70 | 0 | 20% |
| `test-require-*.js` | 12 | 9 | 0 | 57% |
| `test-stream*.js` | 178 | 26 | 0 | 87% |
| `test-string-decoder*.js` | 3 | 0 | 0 | 100% |
| `test-timers*.js` | 40 | 18 | 0 | 69% |
| `test-util-format*.js` | 1 | 0 | 0 | 100% |
| `test-util-inspect*.js` | 2 | 3 | 0 | 40% |
| `test-util-types*.js` | 1 | 1 | 0 | 50% |
| `test-util.js` | 0 | 1 | 0 | 0% |
| `test-whatwg-events*.js` | 5 | 0 | 0 | 100% |
| **all** | **557** | **316** | **0** | **64%** |

10 more files fail under node on the machine that ran them, so they say nothing about Opal and aren't counted.

## By file

| File | Opal | What failed |
|---|---|---|
| `test-abort-controller-any-timeout.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-abortcontroller-internal.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-abortcontroller.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-aborted-util.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-abortsignal-cloneable.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
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
| `test-async-local-storage-bind.js` | pass |  |
| `test-async-local-storage-contexts.js` | **fail** | Error: Opal does not implement the built-in module node:vm yet |
| `test-async-local-storage-deep-stack.js` | pass |  |
| `test-async-local-storage-enter-with.js` | pass |  |
| `test-async-local-storage-exit-does-not-leak.js` | pass |  |
| `test-async-local-storage-http-agent.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-async-local-storage-http-multiclients.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-async-local-storage-http-parser-leak.js` | **fail** | Error: Cannot find module '_http_common' |
| `test-async-local-storage-isolation.js` | pass |  |
| `test-async-local-storage-snapshot.js` | pass |  |
| `test-async-local-storage-weak-asyncwrap-leak.js` | **fail** | Error: Opal does not implement the built-in module node:zlib yet |
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
| `test-buffer-tostring-rangeerror.js` | pass |  |
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
| `test-console-async-write-error.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-clear.js` | **fail** | TypeError: Cannot set property isTTY of #<WriteStream> which has only a getter |
| `test-console-count.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-console-diagnostics-channels.js` | **fail** | Error: Opal does not implement the built-in module node:diagnostics_channel yet |
| `test-console-formatTime.js` | **fail** | Error: Cannot find module 'internal/util/debuglog' |
| `test-console-group.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-instance.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-console-issue-43095.js` | pass |  |
| `test-console-log-stdio-broken-dest.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-log-throw-primitive.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-methods.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-no-swallow-stack-overflow.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-console-not-call-toString.js` | pass |  |
| `test-console-self-assign.js` | pass |  |
| `test-console-stdio-setters.js` | **fail** | fhqwhgads |
| `test-console-sync-write-error.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-table.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-tty-colors-per-stream.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-tty-colors.js` | **fail** | TypeError: Console is not a constructor |
| `test-console-with-frozen-intrinsics.js` | pass |  |
| `test-event-emitter-add-listeners.js` | pass |  |
| `test-event-emitter-check-listener-leaks.js` | pass |  |
| `test-event-emitter-emit-context.js` | pass |  |
| `test-event-emitter-error-monitor.js` | pass |  |
| `test-event-emitter-errors.js` | pass |  |
| `test-event-emitter-get-max-listeners.js` | pass |  |
| `test-event-emitter-invalid-listener.js` | pass |  |
| `test-event-emitter-listener-count.js` | pass |  |
| `test-event-emitter-listeners-side-effects.js` | pass |  |
| `test-event-emitter-listeners.js` | pass |  |
| `test-event-emitter-max-listeners-warning-for-null.js` | pass |  |
| `test-event-emitter-max-listeners-warning-for-symbol.js` | pass |  |
| `test-event-emitter-max-listeners-warning.js` | pass |  |
| `test-event-emitter-max-listeners.js` | pass |  |
| `test-event-emitter-method-names.js` | pass |  |
| `test-event-emitter-modify-in-emit.js` | pass |  |
| `test-event-emitter-no-error-provided-to-error-event.js` | **fail** | Error: Opal does not implement the built-in module node:domain yet |
| `test-event-emitter-num-args.js` | pass |  |
| `test-event-emitter-once.js` | pass |  |
| `test-event-emitter-prepend.js` | pass |  |
| `test-event-emitter-remove-all-listeners.js` | pass |  |
| `test-event-emitter-remove-listeners.js` | pass |  |
| `test-event-emitter-set-max-listeners-side-effects.js` | pass |  |
| `test-event-emitter-special-event-names.js` | pass |  |
| `test-event-emitter-subclass.js` | pass |  |
| `test-event-emitter-symbols.js` | pass |  |
| `test-event-target.js` | pass |  |
| `test-events-customevent.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-events-getmaxlisteners.js` | pass |  |
| `test-events-list.js` | pass |  |
| `test-events-listener-count-with-listener.js` | pass |  |
| `test-events-on-async-iterator.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-events-once.js` | pass |  |
| `test-events-static-geteventlisteners.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-events-uncaught-exception-stack.js` | pass |  |
| `test-eventtarget-brandcheck.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-eventtarget-custom-inspect-does-not-throw.js` | pass |  |
| `test-eventtarget-memoryleakwarning.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-eventtarget-once-twice.js` | pass |  |
| `test-eventtarget.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-fs-access.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-append-file-flush.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-append-file-sync.js` | pass |  |
| `test-fs-append-file.js` | pass |  |
| `test-fs-assert-encoding-error.js` | pass |  |
| `test-fs-buffer.js` | pass |  |
| `test-fs-buffertype-writesync.js` | pass |  |
| `test-fs-chmod-mask.js` | pass |  |
| `test-fs-chmod.js` | pass |  |
| `test-fs-chown-negative-one.js` | pass |  |
| `test-fs-chown-type-check.js` | pass |  |
| `test-fs-close-errors.js` | pass |  |
| `test-fs-close.js` | pass |  |
| `test-fs-constants.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
| `test-fs-copyfile-respect-permissions.js` | pass |  |
| `test-fs-copyfile.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-cp-sync-dereference.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/fs/cp/cp, which Opal doesn't have |
| `test-fs-empty-readStream.js` | pass |  |
| `test-fs-error-messages.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-exists.js` | **fail** | ReferenceError: URL is not defined |
| `test-fs-existssync-false.js` | pass |  |
| `test-fs-existssync-memleak-longpath.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-fchmod.js` | pass |  |
| `test-fs-fchown-negative-one.js` | pass |  |
| `test-fs-fchown.js` | pass |  |
| `test-fs-filehandle-use-after-close.js` | pass |  |
| `test-fs-filehandle.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-fmap.js` | pass |  |
| `test-fs-fsync.js` | pass |  |
| `test-fs-internal-assertencoding.js` | pass |  |
| `test-fs-lchmod.js` | pass |  |
| `test-fs-lchown-negative-one.js` | pass |  |
| `test-fs-lchown.js` | pass |  |
| `test-fs-link.js` | pass |  |
| `test-fs-long-path.js` | pass |  |
| `test-fs-make-callback.js` | pass |  |
| `test-fs-makeStatsCallback.js` | pass |  |
| `test-fs-mkdir-mode-mask.js` | pass |  |
| `test-fs-mkdir-recursive-eaccess.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-mkdir-rmdir.js` | pass |  |
| `test-fs-mkdir.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-mkdtemp-prefix-check.js` | pass |  |
| `test-fs-mkdtemp.js` | **fail** | Error: url.pathToFileURL: node:url is not in Opal yet |
| `test-fs-mkdtempDisposableSync.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-negative-zero.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs.watchFile |
| `test-fs-non-number-arguments-throw.js` | pass |  |
| `test-fs-null-bytes.js` | **fail** | ReferenceError: URL is not defined |
| `test-fs-open-flags.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-open-mode-mask.js` | pass |  |
| `test-fs-open-no-close.js` | pass |  |
| `test-fs-open-numeric-flags.js` | pass |  |
| `test-fs-open.js` | pass |  |
| `test-fs-opendir.js` | pass |  |
| `test-fs-operations-with-surrogate-pairs.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-options-immutable.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-promises-exists.js` | pass |  |
| `test-fs-promises-file-handle-aggregate-errors.js` | **fail** | Error: Cannot find module 'internal/fs/promises' |
| `test-fs-promises-file-handle-append-file.js` | pass |  |
| `test-fs-promises-file-handle-chmod.js` | pass |  |
| `test-fs-promises-file-handle-close-errors.js` | **fail** | Error: Cannot find module 'internal/fs/promises' |
| `test-fs-promises-file-handle-close.js` | **fail** | TypeError: globalThis.gc is not a function |
| `test-fs-promises-file-handle-dispose.js` | pass |  |
| `test-fs-promises-file-handle-op-errors.js` | **fail** | Error: Cannot find module 'internal/fs/promises' |
| `test-fs-promises-file-handle-read-worker.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-promises-file-handle-read.js` | pass |  |
| `test-fs-promises-file-handle-readFile.js` | pass |  |
| `test-fs-promises-file-handle-stat.js` | pass |  |
| `test-fs-promises-file-handle-stream.js` | **fail** | Error: Opal does not implement the built-in module node:stream/consumers yet |
| `test-fs-promises-file-handle-sync.js` | pass |  |
| `test-fs-promises-file-handle-truncate.js` | pass |  |
| `test-fs-promises-file-handle-write.js` | pass |  |
| `test-fs-promises-file-handle-writeFile.js` | pass |  |
| `test-fs-promises-mkdtempDisposable.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-promises-readfile-buffer-option.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-promises-readfile-empty.js` | pass |  |
| `test-fs-promises-readfile-with-fd.js` | pass |  |
| `test-fs-promises-readfile.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-promises-statfs-validate-path.js` | pass |  |
| `test-fs-promises-watch-iterator.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-promises-watch.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-promises-write-optional-params.js` | pass |  |
| `test-fs-promises-writefile-typedarray.js` | pass |  |
| `test-fs-promises-writefile-with-fd.js` | pass |  |
| `test-fs-promises-writefile.js` | pass |  |
| `test-fs-promises.js` | pass |  |
| `test-fs-promisified.js` | pass |  |
| `test-fs-read-empty-buffer.js` | pass |  |
| `test-fs-read-file-assert-encoding.js` | pass |  |
| `test-fs-read-file-sync-hostname.js` | pass |  |
| `test-fs-read-file-sync.js` | pass |  |
| `test-fs-read-offset-null.js` | pass |  |
| `test-fs-read-optional-params.js` | pass |  |
| `test-fs-read-promises-optional-params.js` | pass |  |
| `test-fs-read-stream-autoClose.js` | pass |  |
| `test-fs-read-stream-concurrent-reads.js` | pass |  |
| `test-fs-read-stream-double-close.js` | pass |  |
| `test-fs-read-stream-encoding.js` | pass |  |
| `test-fs-read-stream-err.js` | pass |  |
| `test-fs-read-stream-fd-leak.js` | pass |  |
| `test-fs-read-stream-fd.js` | pass |  |
| `test-fs-read-stream-file-handle.js` | pass |  |
| `test-fs-read-stream-inherit.js` | pass |  |
| `test-fs-read-stream-patch-open.js` | pass |  |
| `test-fs-read-stream-pos.js` | pass |  |
| `test-fs-read-stream-resume.js` | pass |  |
| `test-fs-read-stream-throw-type-error.js` | pass |  |
| `test-fs-read-stream.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-read-type.js` | pass |  |
| `test-fs-read-zero-length.js` | pass |  |
| `test-fs-read.js` | pass |  |
| `test-fs-readSync-optional-params.js` | pass |  |
| `test-fs-readdir-buffer.js` | pass |  |
| `test-fs-readdir-pipe.js` | pass |  |
| `test-fs-readdir-recursive.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-readdir-stack-overflow.js` | pass |  |
| `test-fs-readdir-types-symlinks.js` | pass |  |
| `test-fs-readdir-types.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-readdir-ucs2.js` | pass |  |
| `test-fs-readdir.js` | pass |  |
| `test-fs-readfile-buffer-option.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-readfile-empty.js` | pass |  |
| `test-fs-readfile-eof.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-readfile-error.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-readfile-fd.js` | pass |  |
| `test-fs-readfile-flags.js` | pass |  |
| `test-fs-readfile-pipe-large.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-readfile-pipe.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-readfile-unlink.js` | pass |  |
| `test-fs-readfile-zero-byte-liar.js` | pass |  |
| `test-fs-readfile.js` | pass |  |
| `test-fs-readfilesync-enoent.js` | pass |  |
| `test-fs-readfilesync-pipe-large.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-readlink-type-check.js` | pass |  |
| `test-fs-readv-promises.js` | pass |  |
| `test-fs-readv-promisify.js` | pass |  |
| `test-fs-readv-sync.js` | pass |  |
| `test-fs-readv.js` | pass |  |
| `test-fs-ready-event-stream.js` | pass |  |
| `test-fs-realpath-buffer-encoding.js` | pass |  |
| `test-fs-realpath-native.js` | (fails under node) |  |
| `test-fs-realpath-on-substed-drive.js` | pass |  |
| `test-fs-realpath-pipe.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-realpath.js` | (fails under node) |  |
| `test-fs-rename-type-check.js` | pass |  |
| `test-fs-rm.js` | **fail** | Error: Opal does not implement the built-in module node:url yet |
| `test-fs-rmSync-special-char.js` | pass |  |
| `test-fs-rmdir-recursive-sync-warns-not-found.js` | pass |  |
| `test-fs-rmdir-recursive-sync-warns-on-file.js` | pass |  |
| `test-fs-rmdir-recursive-throws-not-found.js` | pass |  |
| `test-fs-rmdir-recursive-throws-on-file.js` | pass |  |
| `test-fs-rmdir-recursive-warns-not-found.js` | pass |  |
| `test-fs-rmdir-recursive-warns-on-file.js` | pass |  |
| `test-fs-rmdir-recursive.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-rmdir-type-check.js` | pass |  |
| `test-fs-sir-writes-alot.js` | pass |  |
| `test-fs-stat-abort-test.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-stat-bigint.js` | pass |  |
| `test-fs-stat.js` | pass |  |
| `test-fs-statfs.js` | pass |  |
| `test-fs-stream-construct-compat-error-read.js` | pass |  |
| `test-fs-stream-construct-compat-error-write.js` | pass |  |
| `test-fs-stream-construct-compat-graceful-fs.js` | pass |  |
| `test-fs-stream-construct-compat-old-node.js` | pass |  |
| `test-fs-stream-destroy-emit-error.js` | pass |  |
| `test-fs-stream-double-close.js` | pass |  |
| `test-fs-stream-fs-options.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/streams/fast-utf8-stream, which Opal doesn't have |
| `test-fs-stream-options.js` | pass |  |
| `test-fs-symlink-buffer-path.js` | pass |  |
| `test-fs-symlink-dir-junction-relative.js` | pass |  |
| `test-fs-symlink-dir-junction.js` | pass |  |
| `test-fs-symlink-dir.js` | pass |  |
| `test-fs-symlink-longpath.js` | pass |  |
| `test-fs-symlink.js` | pass |  |
| `test-fs-sync-fd-leak.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-syncwritestream.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-timestamp-parsing-error.js` | pass |  |
| `test-fs-truncate-clear-file-zero.js` | pass |  |
| `test-fs-truncate-sync.js` | pass |  |
| `test-fs-truncate.js` | pass |  |
| `test-fs-unlink-type-check.js` | pass |  |
| `test-fs-util-validateoffsetlength.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-utils-get-dirents.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-utimes-y2K38.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-utimes.js` | **fail** | Error: Opal does not implement the built-in module node:url yet |
| `test-fs-watch-abort-signal.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-close-when-destroyed.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-encoding.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-enoent.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-fs-watch-file-enoent-after-deletion.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs.watchFile |
| `test-fs-watch-ignore-function.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-ignore-glob.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-ignore-invalid.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-fs-watch-ignore-mixed.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-ignore-recursive-glob-subdirectories.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/deps/minimatch/index, which Opal doesn't have |
| `test-fs-watch-ignore-recursive-glob.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/deps/minimatch/index, which Opal doesn't have |
| `test-fs-watch-ignore-recursive-mixed.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/deps/minimatch/index, which Opal doesn't have |
| `test-fs-watch-ignore-recursive-regexp.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-ignore-regexp.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-add-file-to-existing-subfolder.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-add-file-to-new-folder.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-add-file-with-url.js` | **fail** | Error: Opal does not implement the built-in module node:url yet |
| `test-fs-watch-recursive-add-file.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-add-folder.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-assert-leaks.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-delete-race.js` | **fail** | Error: Cannot find module 'internal/fs/watchers' |
| `test-fs-watch-recursive-delete.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-linux-parallel-remove.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-watch-recursive-prefix-sibling.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-promise.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-symlink.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-sync-write.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-update-file.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-recursive-validate.js` | pass |  |
| `test-fs-watch-recursive-watch-file.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-ref-unref.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watch-stop-async.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs.watchFile |
| `test-fs-watch-stop-sync.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs.watchFile |
| `test-fs-watch.js` | **fail** | Error: Opal does not implement fs.watch yet |
| `test-fs-watchfile-bigint.js` | **fail** | Error: Cannot find module 'internal/fs/utils' |
| `test-fs-watchfile-ref-unref.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs.watchFile |
| `test-fs-watchfile.js` | **fail** | Error: Opal implements only the synchronous fs functions yet, not fs.watchFile |
| `test-fs-whatwg-url.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-write-buffer-large.js` | pass |  |
| `test-fs-write-buffer.js` | pass |  |
| `test-fs-write-file-buffer.js` | pass |  |
| `test-fs-write-file-flush.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-write-file-invalid-path.js` | pass |  |
| `test-fs-write-file-sync.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-fs-write-file-typedarrays.js` | pass |  |
| `test-fs-write-file.js` | pass |  |
| `test-fs-write-negativeoffset.js` | pass |  |
| `test-fs-write-no-fd.js` | pass |  |
| `test-fs-write-optional-params.js` | pass |  |
| `test-fs-write-reuse-callback.js` | **fail** | TypeError: globalThis.gc is not a function |
| `test-fs-write-sigxfsz.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-write-stream-autoclose-option.js` | pass |  |
| `test-fs-write-stream-change-open.js` | pass |  |
| `test-fs-write-stream-close-without-callback.js` | pass |  |
| `test-fs-write-stream-double-close.js` | pass |  |
| `test-fs-write-stream-encoding.js` | pass |  |
| `test-fs-write-stream-end.js` | pass |  |
| `test-fs-write-stream-err.js` | pass |  |
| `test-fs-write-stream-file-handle-2.js` | pass |  |
| `test-fs-write-stream-file-handle.js` | pass |  |
| `test-fs-write-stream-flush.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-fs-write-stream-fs.js` | pass |  |
| `test-fs-write-stream-patch-open.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-fs-write-stream-throw-type-error.js` | pass |  |
| `test-fs-write-stream.js` | pass |  |
| `test-fs-write-sync-optional-params.js` | pass |  |
| `test-fs-write-sync.js` | pass |  |
| `test-fs-write.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected "actual" to be strictly unequal to: |
| `test-fs-writefile-with-fd.js` | pass |  |
| `test-fs-writestream-open-write.js` | pass |  |
| `test-fs-writesync-crash.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-fs-writev-promises.js` | pass |  |
| `test-fs-writev-sync.js` | pass |  |
| `test-fs-writev.js` | pass |  |
| `test-module-builtin.js` | pass |  |
| `test-module-cache.js` | **fail** | Error: Cannot find module '/tmp/opal-node-tests-3i_l9clk/opal/test/.tmp.test-module-cache.js/test-module-cache.json' |
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
| `test-module-multi-extensions.js` | **fail** | Error: Cannot find module '/tmp/opal-node-tests-3i_l9clk/opal/test/.tmp.test-module-multi-extensions.js/test-extensions' |
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
| `test-os-checked-function.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-os-constants-signals.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-os-eol.js` | pass |  |
| `test-os-fast.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-os-homedir-no-envvar.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-os-process-priority.js` | pass |  |
| `test-os-userinfo-handles-getter-errors.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-os.js` | pass |  |
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
| `test-process-execve-abort.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-process-execve-no-args.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-execve-on-exit.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-execve-permission-fail.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-execve-permission-granted.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-execve-socket.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-process-execve-validation.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-execve-worker-threads.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
| `test-process-execve.js` | **fail** | Error: Opal does not implement the built-in module node:worker_threads yet |
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
| `test-process-getactiverequests.js` | **fail** | TypeError: process._getActiveRequests is not a function |
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
| `test-process-raw-debug.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
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
| `test-process-title-cli.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly equal: |
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
| `test-stream-add-abort-signal.js` | **fail** | Error: Cannot find module 'internal/streams/add-abort-signal' |
| `test-stream-aliases-legacy.js` | **fail** | Error: Cannot find module '_stream_readable' |
| `test-stream-auto-destroy.js` | pass |  |
| `test-stream-await-drain-writers-in-synchronously-recursion-write.js` | pass |  |
| `test-stream-backpressure.js` | pass |  |
| `test-stream-base-prototype-accessors-enumerability.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-stream-base-typechecking.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-stream-big-packet.js` | pass |  |
| `test-stream-big-push.js` | pass |  |
| `test-stream-catch-rejections.js` | pass |  |
| `test-stream-compose.js` | **fail** | Error: internal/blob.isBlob is not implemented by Opal yet |
| `test-stream-construct.js` | pass |  |
| `test-stream-consumers.js` | **fail** | Error: Opal does not implement the built-in module node:stream/consumers yet |
| `test-stream-decoder-objectmode.js` | pass |  |
| `test-stream-destroy-event-order.js` | pass |  |
| `test-stream-destroy.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-stream-drop-take.js` | pass |  |
| `test-stream-duplex-destroy.js` | pass |  |
| `test-stream-duplex-end.js` | pass |  |
| `test-stream-duplex-from.js` | **fail** | Error: Opal does not implement the built-in module node:stream/web yet |
| `test-stream-duplex-props.js` | pass |  |
| `test-stream-duplex-readable-end.js` | pass |  |
| `test-stream-duplex-readable-writable.js` | pass |  |
| `test-stream-duplex-writable-finished.js` | pass |  |
| `test-stream-duplex.js` | **fail** | Error: Opal does not implement the built-in module node:stream/web yet |
| `test-stream-duplexpair.js` | pass |  |
| `test-stream-end-of-streams.js` | pass |  |
| `test-stream-end-paused.js` | pass |  |
| `test-stream-err-multiple-callback-construction.js` | pass |  |
| `test-stream-error-once.js` | pass |  |
| `test-stream-event-names.js` | pass |  |
| `test-stream-events-prepend.js` | pass |  |
| `test-stream-filter.js` | pass |  |
| `test-stream-finished.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-stream-flatMap.js` | pass |  |
| `test-stream-forEach.js` | pass |  |
| `test-stream-inheritance.js` | pass |  |
| `test-stream-ispaused.js` | pass |  |
| `test-stream-map.js` | pass |  |
| `test-stream-objectmode-undefined.js` | pass |  |
| `test-stream-once-readable-pipe.js` | pass |  |
| `test-stream-passthrough-drain.js` | pass |  |
| `test-stream-pipe-after-end.js` | pass |  |
| `test-stream-pipe-await-drain-manual-resume.js` | pass |  |
| `test-stream-pipe-await-drain-push-while-write.js` | pass |  |
| `test-stream-pipe-await-drain.js` | pass |  |
| `test-stream-pipe-cleanup-pause.js` | pass |  |
| `test-stream-pipe-cleanup.js` | pass |  |
| `test-stream-pipe-deadlock.js` | pass |  |
| `test-stream-pipe-error-handling.js` | pass |  |
| `test-stream-pipe-error-unhandled.js` | pass |  |
| `test-stream-pipe-event.js` | pass |  |
| `test-stream-pipe-flow-after-unpipe.js` | pass |  |
| `test-stream-pipe-flow.js` | pass |  |
| `test-stream-pipe-manual-resume.js` | pass |  |
| `test-stream-pipe-multiple-pipes.js` | pass |  |
| `test-stream-pipe-needDrain.js` | pass |  |
| `test-stream-pipe-objectmode-to-non-objectmode.js` | pass |  |
| `test-stream-pipe-same-destination-twice.js` | pass |  |
| `test-stream-pipe-unpipe-streams.js` | pass |  |
| `test-stream-pipe-without-listenerCount.js` | pass |  |
| `test-stream-pipeline-async-iterator.js` | pass |  |
| `test-stream-pipeline-duplex.js` | pass |  |
| `test-stream-pipeline-http2.js` | pass |  |
| `test-stream-pipeline-listeners.js` | pass |  |
| `test-stream-pipeline-process.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-stream-pipeline-queued-end-in-destroy.js` | pass |  |
| `test-stream-pipeline-uncaught.js` | pass |  |
| `test-stream-pipeline-with-empty-string.js` | pass |  |
| `test-stream-pipeline.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-stream-preprocess.js` | **fail** | Error: Opal does not implement the built-in module node:readline yet |
| `test-stream-promises.js` | pass |  |
| `test-stream-push-order.js` | pass |  |
| `test-stream-push-strings.js` | pass |  |
| `test-stream-readable-aborted.js` | pass |  |
| `test-stream-readable-add-chunk-during-data.js` | pass |  |
| `test-stream-readable-async-iter-half-open-duplex.js` | pass |  |
| `test-stream-readable-async-iterators.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-stream-readable-compose.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-stream-readable-constructor-set-methods.js` | pass |  |
| `test-stream-readable-data.js` | pass |  |
| `test-stream-readable-default-encoding.js` | pass |  |
| `test-stream-readable-destroy.js` | pass |  |
| `test-stream-readable-didRead.js` | pass |  |
| `test-stream-readable-dispose.js` | pass |  |
| `test-stream-readable-emit-readable-short-stream.js` | pass |  |
| `test-stream-readable-emittedReadable.js` | pass |  |
| `test-stream-readable-end-destroyed.js` | pass |  |
| `test-stream-readable-ended.js` | pass |  |
| `test-stream-readable-error-end.js` | pass |  |
| `test-stream-readable-event.js` | pass |  |
| `test-stream-readable-flow-recursion.js` | pass |  |
| `test-stream-readable-from-web-termination.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/webstreams/adapters, which Opal doesn't have |
| `test-stream-readable-hwm-0-async.js` | pass |  |
| `test-stream-readable-hwm-0-no-flow-data.js` | pass |  |
| `test-stream-readable-hwm-0.js` | pass |  |
| `test-stream-readable-infinite-read.js` | pass |  |
| `test-stream-readable-invalid-chunk.js` | pass |  |
| `test-stream-readable-needReadable.js` | pass |  |
| `test-stream-readable-next-no-null.js` | pass |  |
| `test-stream-readable-no-unneeded-readable.js` | pass |  |
| `test-stream-readable-object-multi-push-async.js` | pass |  |
| `test-stream-readable-pause-and-resume.js` | pass |  |
| `test-stream-readable-readable-then-resume.js` | pass |  |
| `test-stream-readable-readable.js` | pass |  |
| `test-stream-readable-reading-readingMore.js` | pass |  |
| `test-stream-readable-resume-hwm.js` | pass |  |
| `test-stream-readable-resumeScheduled.js` | pass |  |
| `test-stream-readable-setEncoding-existing-buffers.js` | pass |  |
| `test-stream-readable-setEncoding-null.js` | pass |  |
| `test-stream-readable-strategy-option.js` | **fail** | ReferenceError: ByteLengthQueuingStrategy is not defined |
| `test-stream-readable-to-web-byob.js` | **fail** | AssertionError [ERR_ASSERTION]: Expected values to be strictly deep-equal: |
| `test-stream-readable-to-web-termination-byob.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/webstreams/adapters, which Opal doesn't have |
| `test-stream-readable-to-web-termination.js` | **fail** | Error: Opal does not implement this yet: it needs node:internal/webstreams/adapters, which Opal doesn't have |
| `test-stream-readable-unpipe-resume.js` | pass |  |
| `test-stream-readable-unshift.js` | pass |  |
| `test-stream-readable-with-unimplemented-_read.js` | pass |  |
| `test-stream-readableListening-state.js` | pass |  |
| `test-stream-reduce.js` | pass |  |
| `test-stream-set-default-hwm.js` | pass |  |
| `test-stream-toArray.js` | pass |  |
| `test-stream-toWeb-allows-server-response.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-stream-transform-callback-twice.js` | pass |  |
| `test-stream-transform-constructor-set-methods.js` | pass |  |
| `test-stream-transform-destroy.js` | pass |  |
| `test-stream-transform-final-sync.js` | pass |  |
| `test-stream-transform-final.js` | pass |  |
| `test-stream-transform-flush-data.js` | pass |  |
| `test-stream-transform-hwm0.js` | pass |  |
| `test-stream-transform-objectmode-falsey-value.js` | pass |  |
| `test-stream-transform-split-highwatermark.js` | pass |  |
| `test-stream-transform-split-objectmode.js` | pass |  |
| `test-stream-typedarray.js` | pass |  |
| `test-stream-uint8array.js` | pass |  |
| `test-stream-unpipe-event.js` | pass |  |
| `test-stream-unshift-empty-chunk.js` | pass |  |
| `test-stream-unshift-read-race.js` | pass |  |
| `test-stream-wrap-drain.js` | **fail** | Error: Cannot find module 'internal/js_stream_socket' |
| `test-stream-wrap-encoding.js` | **fail** | Error: Cannot find module 'internal/js_stream_socket' |
| `test-stream-wrap.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-stream-writable-aborted.js` | pass |  |
| `test-stream-writable-change-default-encoding.js` | pass |  |
| `test-stream-writable-clear-buffer.js` | pass |  |
| `test-stream-writable-constructor-set-methods.js` | pass |  |
| `test-stream-writable-decoded-encoding.js` | pass |  |
| `test-stream-writable-destroy.js` | pass |  |
| `test-stream-writable-end-cb-error.js` | pass |  |
| `test-stream-writable-end-cb-uncaught.js` | pass |  |
| `test-stream-writable-end-multiple.js` | pass |  |
| `test-stream-writable-ended-state.js` | pass |  |
| `test-stream-writable-final-async.js` | pass |  |
| `test-stream-writable-final-destroy.js` | pass |  |
| `test-stream-writable-final-throw.js` | pass |  |
| `test-stream-writable-finish-destroyed.js` | pass |  |
| `test-stream-writable-finished-state.js` | pass |  |
| `test-stream-writable-finished.js` | pass |  |
| `test-stream-writable-invalid-chunk.js` | pass |  |
| `test-stream-writable-needdrain-state.js` | pass |  |
| `test-stream-writable-null.js` | pass |  |
| `test-stream-writable-properties.js` | pass |  |
| `test-stream-writable-samecb-singletick.js` | **fail** | TypeError: Console is not a constructor |
| `test-stream-writable-writable.js` | pass |  |
| `test-stream-writable-write-cb-error.js` | pass |  |
| `test-stream-writable-write-cb-twice.js` | pass |  |
| `test-stream-writable-write-error.js` | pass |  |
| `test-stream-writable-write-writev-finish.js` | pass |  |
| `test-stream-writableState-ending.js` | pass |  |
| `test-stream-writableState-uncorked-bufferedRequestCount.js` | pass |  |
| `test-stream-write-destroy.js` | pass |  |
| `test-stream-write-drain.js` | pass |  |
| `test-stream-write-final.js` | pass |  |
| `test-stream-writev.js` | pass |  |
| `test-stream2-base64-single-char-read-end.js` | pass |  |
| `test-stream2-basic.js` | pass |  |
| `test-stream2-compatibility.js` | pass |  |
| `test-stream2-decode-partial.js` | pass |  |
| `test-stream2-finish-pipe-error.js` | pass |  |
| `test-stream2-finish-pipe.js` | pass |  |
| `test-stream2-httpclient-response-end.js` | **fail** | Error: Opal does not implement the built-in module node:http yet |
| `test-stream2-large-read-stall.js` | pass |  |
| `test-stream2-objects.js` | pass |  |
| `test-stream2-pipe-error-handling.js` | pass |  |
| `test-stream2-pipe-error-once-listener.js` | pass |  |
| `test-stream2-push.js` | pass |  |
| `test-stream2-read-correct-num-bytes-in-utf8.js` | pass |  |
| `test-stream2-read-sync-stack.js` | pass |  |
| `test-stream2-readable-empty-buffer-no-eof.js` | pass |  |
| `test-stream2-readable-legacy-drain.js` | pass |  |
| `test-stream2-readable-non-empty-end.js` | pass |  |
| `test-stream2-readable-wrap-destroy.js` | pass |  |
| `test-stream2-readable-wrap-empty.js` | pass |  |
| `test-stream2-readable-wrap-error.js` | pass |  |
| `test-stream2-readable-wrap-proxy-methods.js` | pass |  |
| `test-stream2-readable-wrap.js` | pass |  |
| `test-stream2-set-encoding.js` | pass |  |
| `test-stream2-transform.js` | pass |  |
| `test-stream2-unpipe-drain.js` | pass |  |
| `test-stream2-unpipe-leak.js` | pass |  |
| `test-stream2-writable.js` | pass |  |
| `test-stream3-cork-end.js` | pass |  |
| `test-stream3-cork-uncork.js` | pass |  |
| `test-stream3-pause-then-read.js` | pass |  |
| `test-stream3-pipeline-async-iterator.js` | pass |  |
| `test-streams-highwatermark.js` | pass |  |
| `test-string-decoder-end.js` | pass |  |
| `test-string-decoder-fuzz.js` | pass |  |
| `test-string-decoder.js` | pass |  |
| `test-timers-api-refs.js` | pass |  |
| `test-timers-args.js` | pass |  |
| `test-timers-clear-null-does-not-throw-error.js` | pass |  |
| `test-timers-clear-object-does-not-throw-error.js` | pass |  |
| `test-timers-clear-timeout-interval-equivalent.js` | pass |  |
| `test-timers-clearImmediate-als.js` | pass |  |
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
| `test-timers-nan-duration-emit-once-per-process.js` | pass |  |
| `test-timers-nan-duration-warning-promises.js` | pass |  |
| `test-timers-nan-duration-warning.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-timers-negative-duration-warning-emit-once-per-process.js` | pass |  |
| `test-timers-negative-duration-warning.js` | **fail** | Error: Opal does not implement the built-in module node:child_process yet |
| `test-timers-nested.js` | **fail** | Error: Cannot find module 'internal/util' |
| `test-timers-next-tick.js` | **fail** | Error: Cannot find module 'internal/util' |
| `test-timers-non-integer-delay.js` | pass |  |
| `test-timers-not-emit-duration-zero.js` | pass |  |
| `test-timers-now.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-timers-ordering.js` | **fail** | Error: Cannot find module 'internal/test/binding' |
| `test-timers-process-tampering.js` | pass |  |
| `test-timers-promises-scheduler.js` | pass |  |
| `test-timers-promises.js` | **fail** | Error: Opal does not implement the built-in module node:test yet |
| `test-timers-refresh-in-callback.js` | pass |  |
| `test-timers-refresh.js` | **fail** | Error: Cannot find module 'internal/timers' |
| `test-timers-reset-process-domain-on-throw.js` | **fail** | Error: Opal does not implement the built-in module node:domain yet |
| `test-timers-same-timeout-wrong-list-deleted.js` | pass |  |
| `test-timers-setimmediate-infinite-loop.js` | pass |  |
| `test-timers-socket-timeout-removes-other-socket-unref-timer.js` | **fail** | Error: Opal does not implement the built-in module node:net yet |
| `test-timers-this.js` | pass |  |
| `test-timers-throw-when-cb-not-function.js` | pass |  |
| `test-timers-timeout-promisified.js` | **fail** | Error: Cannot find module 'internal/event_target' |
| `test-timers-timeout-to-interval.js` | pass |  |
| `test-timers-timeout-with-non-integer.js` | pass |  |
| `test-timers-to-primitive.js` | pass |  |
| `test-timers-uncaught-exception.js` | pass |  |
| `test-timers-unenroll-unref-interval.js` | pass |  |
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
| `test-whatwg-events-add-event-listener-options-passive.js` | pass |  |
| `test-whatwg-events-add-event-listener-options-signal.js` | pass |  |
| `test-whatwg-events-customevent.js` | pass |  |
| `test-whatwg-events-event-constructors.js` | pass |  |
| `test-whatwg-events-eventtarget-this-of-listener.js` | pass |  |
