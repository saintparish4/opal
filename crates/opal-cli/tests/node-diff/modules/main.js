// require() across a small project: local files with and without their
// extension, a directory's index, JSON, packages with "main" and with
// "exports" (conditions, subpaths, patterns, a blocked path), a scoped
// package, a package requiring itself by name, a cycle, the cache, and the
// errors Node throws when a request leads nowhere.
const show = (label, f) => {
  let out;
  try {
    out = f();
  } catch (e) {
    out = `${e.code} ${e.message.split("\n")[0].replaceAll(__dirname, "<dir>")}`;
  }
  console.log(label, typeof out === "string" ? out.replaceAll(__dirname, "<dir>") : JSON.stringify(out));
};
show("relative", () => require("./lib/a"));
show("with extension", () => require("./lib/a.js"));
show("index", () => require("./lib"));
show("json", () => require("./lib/data.json"));
show("json invalid", () => require("./lib/bad.json"));
show("legacy main", () => require("legacy"));
show("legacy subpath", () => require("legacy/lib/x"));
show("exports main", () => require("exp"));
show("exports subpath", () => require("exp/feature"));
show("exports pattern", () => require("exp/features/one"));
show("exports blocked", () => require("exp/internal/secret"));
show("exports not exported", () => require("exp/lib/hidden.js"));
show("exports package.json", () => require("exp/package.json").name);
show("scoped", () => require("@scope/pkg"));
show("self reference", () => require("self-ref/check"));
show("missing", () => require("./nope"));
show("missing package", () => require("no-such-package"));
show("resolve", () => require.resolve("exp/feature"));
show("resolve builtin", () => require.resolve("node:path"));
show("resolve.paths builtin", () => require.resolve.paths("path"));
show("cycle", () => require("./lib/cycle-a"));
show("cache", () => require("./lib/a") === require("./lib/a.js") && Object.keys(require.cache).length > 1);
show("children", () => module.children.map((m) => m.id.replaceAll(__dirname, "<dir>")).slice(0, 3));
show("main", () => require.main === module && module.id);
show("paths", () => module.paths.slice(0, 2));
show("filename", () => __filename);
show("unknown builtin", () => require("node:nope"));
show("isBuiltin", () => [require("module").isBuiltin("fs"), require("module").isBuiltin("node:path"), require("module").isBuiltin("exp")]);
show("createRequire", () => require("module").createRequire(__dirname + "/lib/")("./a"));
