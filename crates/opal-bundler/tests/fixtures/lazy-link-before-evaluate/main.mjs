// Found by the generated-graph fuzzer (seeds 2034477 and others): ECMA-262
// links every module an import() reaches before any of them runs, so a
// module that has not run yet already has its functions, and its lets are
// in their TDZ. first.mjs reads both through entry's namespace, before
// second.mjs runs.
import("./entry.mjs").then((ns) => console.log("done", Object.keys(ns).join(",")));
