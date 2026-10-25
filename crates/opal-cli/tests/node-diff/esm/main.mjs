// import across a small project: ES modules with named and default
// exports and a cycle, a CommonJS module, JSON with its attribute, built-ins,
// import(), import.meta, top-level await, require() of an ES module, and the
// errors Node throws when an import leads nowhere or doesn't fit.
import { named, counter, bump } from "./lib/named.mjs";
import def, * as all from "./lib/default.mjs";
import cjs from "./lib/common.cjs";
import data from "./lib/data.json" with { type: "json" };
import path, { join } from "node:path";
import { EventEmitter } from "events";
import { fromPkg } from "esm-pkg";
import { a } from "./lib/cycle-a.mjs";
import { createRequire } from "node:module";

const dir = import.meta.dirname;
const clean = (value) => (typeof value === "string" ? value.replaceAll(dir, "<dir>") : value);
const show = (label, value) => console.log(label, clean(typeof value === "string" ? value : JSON.stringify(value)));
const fail = async (label, f) => {
  try {
    await f();
    show(label, "no error");
  } catch (e) {
    show(label, `${e.name} ${e.code} ${clean(e.message.split("\n")[0])}`);
  }
};

show("named", named);
bump();
show("live binding", counter);
show("default", def);
show("namespace keys", Object.keys(all));
show("cjs default", cjs);
show("json", data);
show("builtin default", path.join("a", "b") === join("a", "b"));
show("builtin named", typeof EventEmitter);
show("package", fromPkg);
show("cycle", a());
show("meta.url", import.meta.url.replace("file://" + dir, "file://<dir>"));
show("meta.filename", import.meta.filename);
show("meta.main", import.meta.main);
show("meta.resolve", import.meta.resolve("./lib/named.mjs").replace("file://" + dir, "file://<dir>"));
show("meta.resolve missing", import.meta.resolve("./nope.mjs").replace("file://" + dir, "file://<dir>"));
show("this", typeof this);
const awaited = await new Promise((r) => setTimeout(() => r("after await"), 1));
show("top-level await", awaited);
const dynamic = await import("./lib/default.mjs");
show("import()", dynamic.default);
show("import() same namespace", dynamic === all);
const cjsNs = await import("./lib/common.cjs");
show("import() cjs keys", Object.keys(cjsNs));
show("type module package", (await import("./pkg-module/index.js")).kind);
const require = createRequire(import.meta.url);
show("require(esm)", require("./lib/named.mjs").named);
show("require(esm) module.exports", require("./lib/with-module-exports.mjs"));
await fail("require(esm) with await", () => require("./lib/tla.mjs"));
await fail("missing file", () => import("./nope.mjs"));
await fail("missing extension", () => import("./lib/named"));
await fail("directory", () => import("./lib"));
await fail("json without attribute", () => import("./lib/data.json"));
await fail("attribute on js", () => import("./lib/named.mjs", { with: { type: "json" } }));
await fail("unknown attribute", () => import("./lib/data.json", { with: { type: "css" } }));
await fail("missing export", () => import("./lib/bad-import.mjs"));
await fail("unknown builtin", () => import("node:nope"));
await fail("missing package", () => import("no-such-pkg"));
await fail("throws", () => import("./lib/throws.mjs"));
import { alpha, beta } from "./lib/named-cjs.cjs";
import * as cjsAll from "./lib/named-cjs.cjs";
import * as reexported from "./lib/reexport.cjs";
show("cjs named", [alpha, beta]);
await fail("cjs getter export", () => import("./lib/import-gamma.mjs"));
show("cjs namespace keys", Object.keys(cjsAll));
show("cjs reexport keys", Object.keys(reexported));
await fail("cjs missing named", () => import("./lib/bad-cjs-import.mjs"));
