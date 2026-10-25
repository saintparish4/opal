// Engine-floor prototype B, the JavaScript half: runs inside the Node child
// that engine-node spawns.
//
//   node driver.cjs MODE FILE...                       plain start
//   node --snapshot-blob B MODE FILE...                from a user-land snapshot
//                                                      built from this file
//
// MODE is `shared` (one context, globals leak between files) or `context` (a
// fresh vm context per file with the harness evaluated in it: Jest's model).
// Test files are read and evaluated with a function wrapper rather than
// `require`d, the same way the deno_core prototype evaluates them, and the way
// a runner that transforms sources has to.
"use strict";
const fs = require("fs");
const path = require("path");
const vm = require("vm");
const v8 = require("v8");

const HARNESS = fs.readFileSync(path.join(__dirname, "..", "harness.js"), "utf8");
const print = (s) => fs.writeSync(1, s + "\n");
globalThis.__opal_print = print;
vm.runInThisContext(HARNESS, { filename: "harness.js" });
const wrap = (src) => `(function () {\n${src}\n})();`;

async function main(argv) {
  const [mode, ...files] = argv;
  let failed = 0;
  for (const f of files) {
    const src = wrap(fs.readFileSync(f, "utf8"));
    if (mode === "context") {
      const ctx = vm.createContext({ __opal_print: print });
      vm.runInContext(HARNESS, ctx, { filename: "harness.js" });
      vm.runInContext(src, ctx, { filename: f });
      failed += await ctx.__opal_run(f);
    } else if (mode === "shared") {
      globalThis.__opal_reset();
      vm.runInThisContext(src, { filename: f });
      failed += await globalThis.__opal_run(f);
    } else {
      throw new Error(`unknown mode ${mode}`);
    }
  }
  process.exitCode = failed > 0 ? 1 : 0;
}

const start = (argv) =>
  main(argv).catch((e) => {
    print(String((e && e.stack) || e));
    process.exitCode = 1;
  });

if (v8.startupSnapshot.isBuildingSnapshot()) {
  v8.startupSnapshot.setDeserializeMainFunction(() => start(process.argv.slice(1)));
} else {
  start(process.argv.slice(2));
}
