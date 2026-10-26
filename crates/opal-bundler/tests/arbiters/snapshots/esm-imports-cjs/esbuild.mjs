var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __commonJS = (cb, mod) => function __require() {
  try {
    return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
  } catch (e) {
    throw mod = 0, e;
  }
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// lib.cjs
var require_lib = __commonJS({
  "lib.cjs"(exports) {
    "use strict";
    exports.counter = 0;
    exports.bump = () => {
      exports.counter++;
    };
    var values = { fromGetter: "getter read once" };
    Object.defineProperty(exports, "fromGetter", { enumerable: true, get: function() {
      return values.fromGetter;
    } });
    Object.defineProperty(exports, "unrecognizedGetter", {
      enumerable: true,
      get() {
        return "only through default";
      }
    });
    Object.defineProperty(exports, "viaDefine", { enumerable: true, value: "defined" });
    exports.later = void 0;
    setTimeout(() => {
      exports.later = "set later";
    }, 0);
  }
});

// transpiled.cjs
var require_transpiled = __commonJS({
  "transpiled.cjs"(exports) {
    "use strict";
    Object.defineProperty(exports, "__esModule", { value: true });
    exports.named = "named";
    exports.default = "the default property";
  }
});

// base.cjs
var require_base = __commonJS({
  "base.cjs"(exports) {
    "use strict";
    exports.fromBase = "base";
    exports.alsoBase = 2;
  }
});

// reexporting.cjs
var require_reexporting = __commonJS({
  "reexporting.cjs"(exports, module) {
    "use strict";
    module.exports = {
      ...require_base(),
      own: 1
    };
  }
});

// main.mjs
var ns = __toESM(require_lib(), 1);
var import_lib = __toESM(require_lib(), 1);
var import_transpiled = __toESM(require_transpiled(), 1);
var reexporting = __toESM(require_reexporting(), 1);
console.log(Object.keys(ns));
console.log(ns.default === ns.default, ns["module.exports"] === ns.default);
console.log("snapshot:", import_lib.counter);
(0, import_lib.bump)();
console.log("after bump:", import_lib.counter, ns.counter, ns.default.counter);
console.log(import_lib.fromGetter, import_lib.viaDefine, "unrecognizedGetter" in ns, ns.default.unrecognizedGetter);
console.log("__esModule default is still module.exports:", typeof import_transpiled.default, import_transpiled.default.default, import_transpiled.named);
console.log("re-exported names:", Object.keys(reexporting));
console.log(Object.getOwnPropertyDescriptor(ns, "counter"));
