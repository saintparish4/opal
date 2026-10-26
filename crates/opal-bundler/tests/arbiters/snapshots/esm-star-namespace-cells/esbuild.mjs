var __defProp = Object.defineProperty;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __esm = (fn, res, err) => function __init() {
  if (err) throw err[0];
  try {
    return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
  } catch (e) {
    throw err = [e], e;
  }
};
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// leaf.mjs
var leaf_exports = {};
__export(leaf_exports, {
  value: () => value
});
var value;
var init_leaf = __esm({
  "leaf.mjs"() {
    value = 1;
  }
});

// star-as-a.mjs
var init_star_as_a = __esm({
  "star-as-a.mjs"() {
    init_leaf();
  }
});

// star-as-c.mjs
var init_star_as_c = __esm({
  "star-as-c.mjs"() {
    init_leaf();
  }
});

// star-as-b.mjs
var init_star_as_b = __esm({
  "star-as-b.mjs"() {
    init_star_as_c();
  }
});

// same.mjs
var init_same = __esm({
  "same.mjs"() {
    init_leaf();
  }
});

// via-same.mjs
var init_via_same = __esm({
  "via-same.mjs"() {
    init_same();
  }
});

// import-a.mjs
var init_import_a = __esm({
  "import-a.mjs"() {
    init_leaf();
  }
});

// import-b.mjs
var init_import_b = __esm({
  "import-b.mjs"() {
    init_leaf();
  }
});

// ext-a.mjs
import * as ext from "node:path";
var init_ext_a = __esm({
  "ext-a.mjs"() {
  }
});

// ext-b.mjs
import * as ext2 from "node:path";
var init_ext_b = __esm({
  "ext-b.mjs"() {
  }
});

// hub.mjs
var hub_exports = {};
__export(hub_exports, {
  imported: () => leaf_exports,
  leaf: () => leaf_exports,
  own: () => own,
  same: () => leaf_exports
});
var own;
var init_hub = __esm({
  "hub.mjs"() {
    init_star_as_a();
    init_star_as_b();
    init_same();
    init_via_same();
    init_import_a();
    init_import_b();
    init_ext_a();
    init_ext_b();
    own = 1;
  }
});

// main.mjs
init_hub();
console.log(Object.keys(hub_exports).join(","));
console.log(leaf_exports === await Promise.resolve().then(() => (init_leaf(), leaf_exports)), typeof leaf_exports, typeof leaf_exports, typeof void 0);
var late = await Promise.resolve().then(() => (init_hub(), hub_exports));
console.log(Object.keys(late).join(","));
