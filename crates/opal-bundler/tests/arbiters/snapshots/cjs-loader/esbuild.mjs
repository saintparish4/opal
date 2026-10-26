var __getOwnPropNames = Object.getOwnPropertyNames;
var __require = /* @__PURE__ */ ((x) => typeof require !== "undefined" ? require : typeof Proxy !== "undefined" ? new Proxy(x, {
  get: (a, b) => (typeof require !== "undefined" ? require : a)[b]
}) : x)(function(x) {
  if (typeof require !== "undefined") return require.apply(this, arguments);
  throw Error('Dynamic require of "' + x + '" is not supported');
});
var __commonJS = (cb, mod) => function __require2() {
  try {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  } catch (e) {
    throw mod = 0, e;
  }
};

// b.cjs
var require_b = __commonJS({
  "b.cjs"(exports, module) {
    "use strict";
    var a = require_a();
    exports.sawName = a.name + ":" + Object.keys(a).join(",");
    exports.loadedAtEnd = false;
    process.nextTick(() => {
      exports.loadedAtEnd = module.loaded;
    });
  }
});

// a.cjs
var require_a = __commonJS({
  "a.cjs"(exports, module) {
    "use strict";
    exports.name = "a";
    var b = require_b();
    exports.fromB = b.sawName;
    exports.parentId = module.parent.id;
  }
});

// flaky.cjs
var require_flaky = __commonJS({
  "flaky.cjs"(exports, module) {
    "use strict";
    globalThis.flakyRuns = (globalThis.flakyRuns || 0) + 1;
    if (globalThis.flakyRuns === 1) {
      throw new Error("first run fails");
    }
    module.exports = "ran " + globalThis.flakyRuns + " times";
  }
});

// data.json
var require_data = __commonJS({
  "data.json"(exports, module) {
    module.exports = { list: [1, 2, 3], ["__proto__"]: { own: true } };
  }
});

// main.cjs
var require_main = __commonJS({
  "main.cjs"(exports, module) {
    var path = __require("node:path");
    var here = (p) => path.relative(process.cwd(), p);
    console.log("main:", __require.main === module, module.id, here(__filename), here(__dirname));
    console.log("this is exports:", exports === module.exports, typeof __require.resolve, arguments.length);
    var a = require_a();
    console.log("a:", a.name, a.fromB, "b loaded:", require_b().loadedAtEnd);
    console.log("same instance:", require_a() === a);
    try {
      __require("./a");
    } catch (error) {
      console.log("no .cjs probing:", error.code);
    }
    console.log("children:", module.children.map((m) => here(m.filename)));
    console.log("a.parent is main:", a.parentId);
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        console.log("flaky:", require_flaky());
      } catch (error) {
        console.log("flaky threw:", error.message);
      }
    }
    var data = require_data();
    console.log("json:", data.list.length, require_data() === data, Object.keys(data));
    var which = "./a.cjs";
    console.log("dynamic require:", __require(which) === a);
    console.log("builtin:", typeof __require("node:os").platform, __require("path") === path);
    try {
      __require("./does-not-exist");
    } catch (error) {
      console.log("missing:", error.code);
    }
    console.log("resolve:", here(__require.resolve("./a.cjs")));
    console.log("module keys:", Object.keys(module));
    console.log("loaded during run:", module.loaded);
    setTimeout(() => console.log("loaded after:", module.loaded), 0);
    module.exports = { done: true };
  }
});
export default require_main();
