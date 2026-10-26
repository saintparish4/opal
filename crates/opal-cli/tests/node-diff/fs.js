// The synchronous fs functions: what each returns, and what each throws,
// on files this program makes in a fresh temporary directory. The
// directory is printed as <dir>, so the output is the same on every run.
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(`${process.env.TMPDIR || "/tmp"}/opal-fs-`);
const at = (name) => path.join(root, name);
const clean = (value) => (typeof value === "string" ? value.split(root).join("<dir>") : value);

function show(label, run) {
  let result;
  try {
    result = run();
  } catch (error) {
    const fields = Object.keys(error).map((key) => `${key}=${clean(String(error[key]))}`);
    console.log(`${label} threw ${error.name}: ${clean(error.message)} [${fields.join(" ")}]`);
    return;
  }
  if (Buffer.isBuffer(result)) result = `<Buffer ${result.toString("hex")}>`;
  else if (typeof result === "string") result = JSON.stringify(clean(result));
  else if (Array.isArray(result)) result = JSON.stringify(result.map(clean));
  else if (typeof result === "bigint") result = `${result}n`;
  console.log(`${label}: ${result}`);
}

console.log("keys:", Object.keys(fs).join(" "));
console.log("constants:", JSON.stringify(fs.constants));
console.log("F_OK R_OK W_OK X_OK:", fs.F_OK, fs.R_OK, fs.W_OK, fs.X_OK);

// Writing and reading whole files.
show("writeFileSync", () => fs.writeFileSync(at("a.txt"), "héllo\n"));
show("readFileSync utf8", () => fs.readFileSync(at("a.txt"), "utf8"));
show("readFileSync utf-8 option", () => fs.readFileSync(at("a.txt"), { encoding: "utf-8" }));
show("readFileSync buffer", () => fs.readFileSync(at("a.txt")));
for (const encoding of ["latin1", "hex", "base64", "base64url", "ucs2", "ascii"]) {
  show(`readFileSync ${encoding}`, () => fs.readFileSync(at("a.txt"), encoding));
}
show("writeFileSync hex", () => fs.writeFileSync(at("b.bin"), "00ff10", "hex"));
show("readFileSync hex file", () => fs.readFileSync(at("b.bin")));
show("writeFileSync Uint8Array", () => fs.writeFileSync(at("c.bin"), new Uint8Array([1, 2, 3])));
show("readFileSync Uint8Array file", () => fs.readFileSync(at("c.bin")));
show("writeFileSync DataView", () => fs.writeFileSync(at("c.bin"), new DataView(new ArrayBuffer(2))));
show("writeFileSync object", () => fs.writeFileSync(at("d.txt"), { toString: () => "x" }));
show("writeFileSync number", () => fs.writeFileSync(at("d.txt"), 42));
show("writeFileSync flag a", () => fs.writeFileSync(at("a.txt"), "more", { flag: "a" }));
show("appendFileSync", () => fs.appendFileSync(at("a.txt"), "!"));
show("appendFileSync new", () => fs.appendFileSync(at("e.txt"), Buffer.from("e")));
show("readFileSync after append", () => fs.readFileSync(at("a.txt"), "utf8"));
show("writeFileSync flag wx", () => fs.writeFileSync(at("a.txt"), "x", { flag: "wx" }));
show("writeFileSync bad flag", () => fs.writeFileSync(at("a.txt"), "x", { flag: "q" }));
show("writeFileSync no dir", () => fs.writeFileSync(at("no/such/file"), "x"));
show("readFileSync missing", () => fs.readFileSync(at("missing")));
show("readFileSync missing utf8", () => fs.readFileSync(at("missing"), "utf8"));
show("readFileSync dir", () => fs.readFileSync(root));
show("readFileSync dir utf8", () => fs.readFileSync(root, "utf8"));
show("readFileSync empty", () => fs.readFileSync("/dev/null", "utf8"));
show("readFileSync bad encoding", () => fs.readFileSync(at("a.txt"), "nope"));
show("readFileSync number path", () => fs.readFileSync(1.5));
show("readFileSync object path", () => fs.readFileSync({}));
show("readFileSync NUL path", () => fs.readFileSync("a\0b"));
show("readFileSync Buffer path", () => fs.readFileSync(Buffer.from(at("a.txt")), "utf8"));
show("readFileSync invalid utf8", () => {
  fs.writeFileSync(at("bad.txt"), Buffer.from([0x61, 0xff, 0xc3, 0x28, 0xe2, 0x82, 0x62]));
  return JSON.stringify(fs.readFileSync(at("bad.txt"), "utf8"));
});
show("big file", () => {
  const data = Buffer.alloc(3 * 65536 + 17, "abc");
  fs.writeFileSync(at("big"), data);
  return fs.readFileSync(at("big")).equals(data) && fs.readFileSync(at("big"), "utf8") === data.toString();
});
show("writeFileSync lone surrogate", () => {
  fs.writeFileSync(at("s.txt"), "a\ud800b");
  return fs.readFileSync(at("s.txt"));
});

// Descriptors.
let fd;
show("openSync r", () => typeof (fd = fs.openSync(at("a.txt"), "r")));
show("readSync", () => {
  const buffer = Buffer.alloc(4);
  return [fs.readSync(fd, buffer, 0, 4, 0), buffer.toString("hex")].join(" ");
});
show("readSync position null", () => {
  const buffer = Buffer.alloc(3);
  return [fs.readSync(fd, buffer, 1, 2, null), buffer.toString("hex")].join(" ");
});
show("readSync bigint position", () => {
  const buffer = Buffer.alloc(2);
  return [fs.readSync(fd, buffer, 0, 2, 2n), buffer.toString("hex")].join(" ");
});
show("readSync options", () => {
  const buffer = Buffer.alloc(5);
  return [fs.readSync(fd, buffer, { position: 1, length: 2 }), buffer.toString("hex")].join(" ");
});
show("readSync at end", () => fs.readSync(fd, Buffer.alloc(4), 0, 4, 1000));
show("readSync bad offset", () => fs.readSync(fd, Buffer.alloc(4), 5, 1, 0));
show("readSync bad length", () => fs.readSync(fd, Buffer.alloc(4), 0, 9, 0));
show("readvSync", () => {
  const a = Buffer.alloc(2);
  const b = Buffer.alloc(3);
  return [fs.readvSync(fd, [a, b], 0), a.toString(), b.toString()].join(" ");
});
show("fstatSync", () => {
  const stats = fs.fstatSync(fd);
  return [stats.isFile(), stats.size].join(" ");
});
show("writeSync on read-only fd", () => fs.writeSync(fd, "x"));
show("closeSync", () => fs.closeSync(fd));
show("closeSync again", () => fs.closeSync(fd));
show("fstatSync closed", () => fs.fstatSync(fd));
show("readSync closed", () => fs.readSync(fd, Buffer.alloc(1)));
show("openSync missing", () => fs.openSync(at("missing"), "r"));
show("openSync wx existing", () => fs.openSync(at("a.txt"), "wx"));
show("openSync dir for writing", () => fs.openSync(root, "w"));
for (const fd of ["x", -1, 1.5, 2 ** 31, NaN, Infinity, null, undefined, 1n, {}, [], () => {}, Symbol("s"), "a'b", "a long string that is too long to show"]) {
  show(`closeSync ${String(fd)}`, () => fs.closeSync(fd));
}
for (const mode of ["x", -1, 1.5, 8, 2 ** 31, null, 1n]) {
  show(`accessSync mode ${String(mode)}`, () => fs.accessSync(at("a.txt"), mode));
  show(`copyFileSync mode ${String(mode)}`, () => fs.copyFileSync(at("a.txt"), at("mode-copy"), mode));
}
show("openSync numeric flags", () => {
  const fd = fs.openSync(at("n.txt"), fs.constants.O_CREAT | fs.constants.O_WRONLY, 0o600);
  fs.closeSync(fd);
  return (fs.statSync(at("n.txt")).mode & 0o777).toString(8);
});
show("writes", () => {
  const fd = fs.openSync(at("w.txt"), "w+");
  const out = [
    fs.writeSync(fd, "abc"),
    fs.writeSync(fd, Buffer.from("defg"), 1, 2),
    fs.writeSync(fd, "Z", 0),
    fs.writeSync(fd, "ffff", 4, "hex"),
    fs.writeSync(fd, new Uint8Array([0x31, 0x32])),
    fs.writevSync(fd, [Buffer.from("x"), Buffer.from("yz")]),
    fs.writevSync(fd, [Buffer.from("Q")], 1),
  ];
  fs.ftruncateSync(fd, 9);
  fs.fsyncSync(fd);
  fs.fdatasyncSync(fd);
  out.push(fs.fstatSync(fd).size);
  fs.closeSync(fd);
  out.push(fs.readFileSync(at("w.txt")).toString("hex"));
  return out.join(" ");
});
show("writeSync bad fd", () => fs.writeSync(9999, "x"));
show("writeSync bad fd buffer", () => fs.writeSync(9999, Buffer.from("x")));
show("writeSync object", () => fs.writeSync(1, {}));
show("truncateSync", () => {
  fs.truncateSync(at("w.txt"), 2);
  return fs.readFileSync(at("w.txt"), "utf8");
});
show("truncateSync grow", () => {
  fs.truncateSync(at("w.txt"), 4);
  return fs.readFileSync(at("w.txt"));
});
show("truncateSync missing", () => fs.truncateSync(at("missing")));

// Stats.
show("statSync", () => {
  const stats = fs.statSync(at("a.txt"));
  return [
    stats instanceof fs.Stats,
    stats.isFile(),
    stats.isDirectory(),
    stats.isSymbolicLink(),
    stats.size,
    stats.nlink,
    typeof stats.ino,
    typeof stats.mtimeMs,
    stats.mtime instanceof Date,
    Math.abs(stats.mtime.getTime() - stats.mtimeMs) <= 0.5,
    Object.keys(stats).join(","),
  ].join(" ");
});
show("statSync dir", () => {
  const stats = fs.statSync(root);
  return [stats.isDirectory(), stats.isFile()].join(" ");
});
show("statSync bigint", () => {
  const stats = fs.statSync(at("a.txt"), { bigint: true });
  return [typeof stats.size, typeof stats.mtimeNs, typeof stats.mtimeMs, stats.size, Object.keys(stats).join(",")].join(" ");
});
show("statSync missing", () => fs.statSync(at("missing")));
show("statSync missing no throw", () => fs.statSync(at("missing"), { throwIfNoEntry: false }));
show("statSync not a dir", () => fs.statSync(at("a.txt/x")));
show("statSync not a dir no throw", () => fs.statSync(at("a.txt/x"), { throwIfNoEntry: false }));
show("lstatSync missing", () => fs.lstatSync(at("missing")));
show("lstatSync missing no throw", () => fs.lstatSync(at("missing"), { throwIfNoEntry: false }));
show("statSync bad option", () => fs.statSync(at("a.txt"), { throwIfNoEntry: "x" }));
show("statfsSync", () => {
  const stats = fs.statfsSync(root);
  return [Object.keys(stats).join(","), typeof stats.bsize, stats.bsize > 0].join(" ");
});
show("statfsSync bigint", () => typeof fs.statfsSync(root, { bigint: true }).blocks);
show("statfsSync missing", () => fs.statfsSync(at("missing")));
show("chmodSync", () => {
  fs.chmodSync(at("a.txt"), 0o640);
  return (fs.statSync(at("a.txt")).mode & 0o777).toString(8);
});
show("chmodSync string mode", () => {
  fs.chmodSync(at("a.txt"), "600");
  return (fs.statSync(at("a.txt")).mode & 0o777).toString(8);
});
show("chmodSync bad mode", () => fs.chmodSync(at("a.txt"), "rw"));
show("chmodSync missing", () => fs.chmodSync(at("missing"), 0o600));
show("fchmodSync", () => {
  const fd = fs.openSync(at("a.txt"), "r");
  fs.fchmodSync(fd, 0o644);
  fs.closeSync(fd);
  return (fs.statSync(at("a.txt")).mode & 0o777).toString(8);
});
show("chownSync self", () => {
  const { uid, gid } = fs.statSync(at("a.txt"));
  fs.chownSync(at("a.txt"), uid, gid);
  fs.lchownSync(at("a.txt"), uid, gid);
  const fd = fs.openSync(at("a.txt"), "r");
  fs.fchownSync(fd, uid, gid);
  fs.closeSync(fd);
  return "ok";
});
show("chownSync missing", () => fs.chownSync(at("missing"), 0, 0));
show("chownSync bad uid", () => fs.chownSync(at("a.txt"), -2, 0));
show("utimesSync", () => {
  fs.utimesSync(at("a.txt"), 1000000000, 1000000000.5);
  const stats = fs.statSync(at("a.txt"));
  return [stats.atimeMs, stats.mtimeMs, stats.mtime.toISOString()].join(" ");
});
show("utimesSync Date", () => {
  fs.utimesSync(at("a.txt"), new Date(1e12), new Date(1.5e12));
  return fs.statSync(at("a.txt")).mtimeMs;
});
show("utimesSync string", () => {
  fs.utimesSync(at("a.txt"), "123", "456.25");
  return fs.statSync(at("a.txt")).mtimeMs;
});
show("utimesSync bad", () => fs.utimesSync(at("a.txt"), "x", {}));
show("utimesSync missing", () => fs.utimesSync(at("missing"), 1, 1));
show("futimesSync", () => {
  const fd = fs.openSync(at("a.txt"), "r");
  fs.futimesSync(fd, 5, 6);
  fs.closeSync(fd);
  return fs.statSync(at("a.txt")).mtimeMs;
});
show("futimesSync bad fd", () => fs.futimesSync(9999, 5, 6));

// Directories.
show("mkdirSync", () => fs.mkdirSync(at("dir")));
show("mkdirSync existing", () => fs.mkdirSync(at("dir")));
show("mkdirSync missing parent", () => fs.mkdirSync(at("p/q")));
show("mkdirSync recursive", () => fs.mkdirSync(at("dir/x/y/z"), { recursive: true }));
show("mkdirSync recursive existing", () => fs.mkdirSync(at("dir/x"), { recursive: true }));
show("mkdirSync recursive trailing slash", () => fs.mkdirSync(at("dir/t/u/"), { recursive: true }));
show("mkdirSync recursive on file", () => fs.mkdirSync(at("a.txt"), { recursive: true }));
show("mkdirSync recursive through file", () => fs.mkdirSync(at("a.txt/sub/deeper"), { recursive: true }));
show("mkdirSync mode", () => {
  fs.mkdirSync(at("m"), 0o700);
  return (fs.statSync(at("m")).mode & 0o777).toString(8);
});
show("mkdirSync relative recursive", () => {
  const cwd = process.cwd();
  return fs.mkdirSync(path.relative(cwd, at("rel/a/b")), { recursive: true }) === path.relative(cwd, at("rel"));
});
show("mkdtempSync", () => {
  const made = fs.mkdtempSync(at("t-"));
  return [path.basename(made).length, fs.statSync(made).isDirectory()].join(" ");
});
show("mkdtempSync buffer", () => Buffer.isBuffer(fs.mkdtempSync(at("t-"), "buffer")));
show("mkdtempSync missing", () => fs.mkdtempSync(at("missing/t-")));
for (const name of ["b", "B", "a", "_", "é", "10", "9", ".hidden", "Z z"]) {
  fs.writeFileSync(at(`dir/${name}`), name);
}
fs.symlinkSync("b", at("dir/link"));
show("readdirSync", () => fs.readdirSync(at("dir")));
show("readdirSync buffer", () => fs.readdirSync(at("dir"), "buffer").map((b) => b.toString("hex")));
show("readdirSync hex", () => fs.readdirSync(at("dir"), { encoding: "hex" }));
show("readdirSync withFileTypes", () =>
  fs.readdirSync(at("dir"), { withFileTypes: true }).map((d) =>
    [d.name, clean(d.parentPath), d.isFile(), d.isDirectory(), d.isSymbolicLink(), d instanceof fs.Dirent].join(":"),
  ),
);
show("readdirSync recursive", () => fs.readdirSync(at("dir"), { recursive: true }));
show("readdirSync recursive withFileTypes", () =>
  fs.readdirSync(at("dir"), { recursive: true, withFileTypes: true }).map((d) => `${clean(d.parentPath)}/${d.name}`),
);
show("readdirSync file", () => fs.readdirSync(at("a.txt")));
show("readdirSync missing", () => fs.readdirSync(at("missing")));
show("rmdirSync not empty", () => fs.rmdirSync(at("dir")));
show("rmdirSync file", () => fs.rmdirSync(at("a.txt")));
show("rmdirSync missing", () => fs.rmdirSync(at("missing")));
show("rmdirSync", () => fs.rmdirSync(at("m")));

// Links.
show("symlinkSync", () => fs.symlinkSync("a.txt", at("l")));
show("symlinkSync existing", () => fs.symlinkSync("a.txt", at("l")));
show("readlinkSync", () => fs.readlinkSync(at("l")));
show("readlinkSync buffer", () => fs.readlinkSync(at("l"), "buffer"));
show("readlinkSync file", () => fs.readlinkSync(at("a.txt")));
show("lstatSync link", () => [fs.lstatSync(at("l")).isSymbolicLink(), fs.statSync(at("l")).isFile()].join(" "));
show("symlinkSync dangling", () => fs.symlinkSync("nothing", at("dangling")));
show("existsSync dangling", () => fs.existsSync(at("dangling")));
show("statSync dangling", () => fs.statSync(at("dangling")));
show("symlinkSync loop", () => {
  fs.symlinkSync("loop2", at("loop1"));
  fs.symlinkSync("loop1", at("loop2"));
  return fs.statSync(at("loop1"));
});
show("realpathSync", () => fs.realpathSync(at("l")) === fs.realpathSync(at("a.txt")));
show("realpathSync.native", () => fs.realpathSync.native(at("l")) === fs.realpathSync(at("a.txt")));
show("realpathSync buffer", () => Buffer.isBuffer(fs.realpathSync(at("l"), "buffer")));
show("realpathSync missing", () => fs.realpathSync(at("missing")));
show("realpathSync.native missing", () => fs.realpathSync.native(at("missing")));
show("realpathSync loop", () => fs.realpathSync(at("loop1")));
show("realpathSync.native loop", () => fs.realpathSync.native(at("loop1")));
show("linkSync", () => {
  fs.linkSync(at("a.txt"), at("hard"));
  return fs.statSync(at("a.txt")).nlink;
});
show("linkSync existing", () => fs.linkSync(at("a.txt"), at("hard")));
show("linkSync missing", () => fs.linkSync(at("missing"), at("hard2")));
show("lutimesSync", () => {
  fs.lutimesSync(at("l"), 7, 8);
  return fs.lstatSync(at("l")).mtimeMs;
});

// Access.
show("accessSync", () => fs.accessSync(at("a.txt")));
show("accessSync R_OK|W_OK", () => fs.accessSync(at("a.txt"), fs.constants.R_OK | fs.constants.W_OK));
show("accessSync X_OK", () => fs.accessSync(at("a.txt"), fs.constants.X_OK));
show("accessSync missing", () => fs.accessSync(at("missing")));
show("accessSync bad mode", () => fs.accessSync(at("a.txt"), 99));
show("existsSync", () => [fs.existsSync(at("a.txt")), fs.existsSync(root), fs.existsSync(at("missing"))].join(" "));
show("existsSync odd", () => [fs.existsSync(), fs.existsSync(null), fs.existsSync(1.5), fs.existsSync("a\0b")].join(" "));

// Renaming, copying and removing.
show("renameSync", () => fs.renameSync(at("e.txt"), at("f.txt")));
show("renameSync over", () => fs.renameSync(at("f.txt"), at("d.txt")));
show("renameSync missing", () => fs.renameSync(at("missing"), at("x")));
show("renameSync into missing dir", () => fs.renameSync(at("d.txt"), at("no/where")));
show("renameSync dir over file", () => fs.renameSync(at("dir"), at("a.txt")));
show("copyFileSync", () => {
  fs.chmodSync(at("a.txt"), 0o640);
  fs.copyFileSync(at("a.txt"), at("copy"));
  return [fs.readFileSync(at("copy"), "utf8"), (fs.statSync(at("copy")).mode & 0o777).toString(8)].join(" ");
});
show("copyFileSync over", () => {
  fs.writeFileSync(at("long"), "a much longer file than a.txt is");
  fs.copyFileSync(at("a.txt"), at("long"));
  return fs.readFileSync(at("long"), "utf8");
});
show("copyFileSync EXCL", () => fs.copyFileSync(at("a.txt"), at("copy"), fs.constants.COPYFILE_EXCL));
show("copyFileSync FICLONE", () => fs.copyFileSync(at("a.txt"), at("clone"), fs.constants.COPYFILE_FICLONE));
show("copyFileSync missing", () => fs.copyFileSync(at("missing"), at("x")));
show("copyFileSync to dir", () => fs.copyFileSync(at("a.txt"), root));
show("copyFileSync self", () => {
  fs.copyFileSync(at("a.txt"), at("a.txt"));
  return fs.readFileSync(at("a.txt"), "utf8");
});
show("copyFileSync bad mode", () => fs.copyFileSync(at("a.txt"), at("y"), 8));
show("unlinkSync", () => fs.unlinkSync(at("copy")));
show("unlinkSync missing", () => fs.unlinkSync(at("copy")));
show("unlinkSync dir", () => fs.unlinkSync(at("dir")));
show("rmSync file", () => fs.rmSync(at("clone")));
show("rmSync missing", () => fs.rmSync(at("clone")));
show("rmSync missing force", () => fs.rmSync(at("clone"), { force: true }));
show("rmSync dir", () => fs.rmSync(at("dir")));
show("rmSync dir recursive", () => fs.rmSync(at("dir"), { recursive: true }));
show("rmSync link", () => {
  fs.rmSync(at("l"));
  return [fs.existsSync(at("l")), fs.existsSync(at("a.txt"))].join(" ");
});
show("rmSync bad option", () => fs.rmSync(at("a.txt"), { maxRetries: -1 }));
show("rmdirSync recursive", () => {
  fs.mkdirSync(at("r/s"), { recursive: true });
  fs.rmdirSync(at("r"), { recursive: true });
  return fs.existsSync(at("r"));
});

// What is left, in order.
show("left", () => fs.readdirSync(root).map((name) => name.replace(/^t-.{6}$/, "t-XXXXXX")));
fs.rmSync(root, { recursive: true, force: true });
show("gone", () => fs.existsSync(root));
