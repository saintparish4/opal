// fs's callback forms: what each passes its callback, and what each
// fails with, on files this program makes in a fresh temporary directory
// (printed as <dir>). The calls run one after another, each starting in the
// callback of the one before, so the output is the same on every run.
const fs = require("fs");
const path = require("path");

const root = fs.mkdtempSync(`${process.env.TMPDIR || "/tmp"}/opal-fs-callbacks-`);
const at = (name) => path.join(root, name);
// mkdtemp's random suffix is printed as RANDOM, and its template as given.
const clean = (value) =>
  typeof value === "string" ? value.split(root).join("<dir>").replace(/t-(?!XXXXXX)[A-Za-z0-9]{6}/g, "t-RANDOM") : value;

function shown(value) {
  if (Buffer.isBuffer(value)) return `<Buffer ${value.toString("hex")}>`;
  if (value instanceof fs.Stats) return `Stats(size=${value.size}, file=${value.isFile()}, dir=${value.isDirectory()})`;
  if (value instanceof fs.Dirent) return `Dirent(${value.name}, file=${value.isFile()})`;
  if (Array.isArray(value)) return `[${value.map(shown).join(", ")}]`;
  if (typeof value === "string") return JSON.stringify(clean(value));
  if (typeof value === "bigint") return `${value}n`;
  return String(value);
}

function failure(error) {
  const fields = Object.keys(error).map((key) => `${key}=${clean(String(error[key]))}`);
  const frames = error.stack.split("\n").length - 1;
  return `${error.name}: ${clean(error.message)} [${fields.join(" ")}] frames=${frames}`;
}

const cases = [];
function check(label, start) {
  cases.push((next) => {
    let threw;
    try {
      start(function (...args) {
        const [error, ...values] = args;
        const head = `${label}: arguments=${args.length}`;
        if (error) console.log(`${head} ${failure(error)}`);
        else console.log(`${head} ${values.map(shown).join(" ")}`);
        next();
      });
    } catch (error) {
      threw = error;
    }
    if (threw) {
      console.log(`${label} threw ${threw.name}: ${clean(threw.message)} [code=${threw.code}]`);
      next();
    }
  });
}

// Whole files.
check("writeFile", (cb) => fs.writeFile(at("a.txt"), "héllo\n", cb));
check("readFile utf8", (cb) => fs.readFile(at("a.txt"), "utf8", cb));
check("readFile buffer", (cb) => fs.readFile(at("a.txt"), cb));
check("readFile hex", (cb) => fs.readFile(at("a.txt"), { encoding: "hex" }, cb));
check("appendFile", (cb) => fs.appendFile(at("a.txt"), "more", cb));
check("readFile after append", (cb) => fs.readFile(at("a.txt"), "latin1", cb));
check("readFile missing", (cb) => fs.readFile(at("missing"), cb));
check("readFile dir", (cb) => fs.readFile(root, cb));
check("readFile no callback", () => fs.readFile(at("a.txt")));
check("writeFile flag wx", (cb) => fs.writeFile(at("a.txt"), "x", { flag: "wx" }, cb));
check("writeFile no dir", (cb) => fs.writeFile(at("no/file"), "x", cb));
check("writeFile buffer", (cb) => fs.writeFile(at("b.bin"), Buffer.from([1, 2, 3]), cb));
check("readFile big", (cb) => {
  const data = Buffer.alloc(300000, "abc");
  fs.writeFile(at("big"), data, (err) => {
    if (err) return cb(err);
    fs.readFile(at("big"), (err, back) => cb(err, back && back.equals(data), back && back.length));
  });
});

// Descriptors.
let fd;
check("open", (cb) => fs.open(at("a.txt"), "r+", (err, value) => { fd = value; cb(err, typeof value); }));
check("read", (cb) => {
  const buffer = Buffer.alloc(4);
  fs.read(fd, buffer, 0, 4, 0, (err, n, same) => cb(err, n, buffer, same === buffer));
});
check("read options", (cb) => fs.read(fd, { buffer: Buffer.alloc(3), position: 1 }, (err, n, b) => cb(err, n, b)));
check("read at end", (cb) => fs.read(fd, Buffer.alloc(4), 0, 4, 1000, cb));
check("readv", (cb) => {
  const a = Buffer.alloc(2);
  const b = Buffer.alloc(3);
  fs.readv(fd, [a, b], 0, (err, n) => cb(err, n, a, b));
});
check("write string", (cb) => fs.write(fd, "ZZ", 0, cb));
check("write buffer", (cb) => fs.write(fd, Buffer.from("yy"), 0, 2, 2, (err, n, b) => cb(err, n, b)));
check("writev", (cb) => fs.writev(fd, [Buffer.from("1"), Buffer.from("2")], 4, cb));
check("fstat", (cb) => fs.fstat(fd, cb));
check("fstat bigint", (cb) => fs.fstat(fd, { bigint: true }, (err, s) => cb(err, typeof s.size, s.size)));
check("ftruncate", (cb) => fs.ftruncate(fd, 4, cb));
check("fsync", (cb) => fs.fsync(fd, cb));
check("fdatasync", (cb) => fs.fdatasync(fd, cb));
check("fchmod", (cb) => fs.fchmod(fd, 0o640, cb));
check("futimes", (cb) => fs.futimes(fd, 5, 6, cb));
check("close", (cb) => fs.close(fd, cb));
check("close again", (cb) => fs.close(fd, cb));
check("read closed", (cb) => fs.read(fd, Buffer.alloc(1), 0, 1, 0, cb));
check("close bad type", (cb) => fs.close("x", cb));
check("open missing", (cb) => fs.open(at("missing"), "r", cb));
check("open bad flag", (cb) => fs.open(at("a.txt"), "q", cb));
check("open no callback", () => fs.open(at("a.txt"), "r"));

// Stats.
check("stat", (cb) => fs.stat(at("a.txt"), cb));
check("stat dir", (cb) => fs.stat(root, cb));
check("stat bigint", (cb) => fs.stat(at("a.txt"), { bigint: true }, (err, s) => cb(err, typeof s.mtimeNs)));
check("stat missing", (cb) => fs.stat(at("missing"), cb));
check("lstat missing", (cb) => fs.lstat(at("missing"), cb));
check("statfs", (cb) => fs.statfs(root, (err, s) => cb(err, Object.keys(s).join(","), Object.values(s).map((v) => typeof v).join(","))));
check("statfs bigint", (cb) => fs.statfs(root, { bigint: true }, (err, s) => cb(err, Object.values(s).map((v) => typeof v).join(","))));
check("stat missing no throw", (cb) => fs.stat(at("missing"), { throwIfNoEntry: false }, cb));
check("lstat missing no throw", (cb) => fs.lstat(at("missing"), { throwIfNoEntry: false }, cb));
check("chmod", (cb) => fs.chmod(at("a.txt"), 0o600, cb));
check("chmod missing", (cb) => fs.chmod(at("missing"), 0o600, cb));
check("mode after chmod", (cb) => fs.stat(at("a.txt"), (err, s) => cb(err, (s.mode & 0o777).toString(8))));
check("utimes", (cb) => fs.utimes(at("a.txt"), 1000000000, 1000000000, cb));
check("mtime after utimes", (cb) => fs.stat(at("a.txt"), (err, s) => cb(err, s.mtimeMs)));
check("access", (cb) => fs.access(at("a.txt"), cb));
check("access missing", (cb) => fs.access(at("missing"), cb));
check("access bad mode", (cb) => fs.access(at("a.txt"), 99, cb));
check("exists", (cb) => fs.exists(at("a.txt"), (yes) => cb(null, yes)));
check("exists missing", (cb) => fs.exists(at("missing"), (yes) => cb(null, yes)));

// Directories.
check("mkdir", (cb) => fs.mkdir(at("dir"), cb));
check("mkdir existing", (cb) => fs.mkdir(at("dir"), cb));
check("mkdir recursive", (cb) => fs.mkdir(at("dir/x/y"), { recursive: true }, cb));
check("mkdir recursive existing", (cb) => fs.mkdir(at("dir/x"), { recursive: true }, cb));
check("mkdir through file", (cb) => fs.mkdir(at("a.txt/sub"), { recursive: true }, cb));
check("mkdtemp", (cb) => fs.mkdtemp(at("t-"), (err, made) => cb(err, path.basename(made).length)));
check("mkdtemp missing", (cb) => fs.mkdtemp(at("missing/t-"), cb));
check("fill dir", (cb) => {
  for (const name of ["b", "a", "C"]) fs.writeFileSync(at(`dir/${name}`), name);
  cb(null);
});
check("readdir", (cb) => fs.readdir(at("dir"), cb));
check("readdir withFileTypes", (cb) => fs.readdir(at("dir"), { withFileTypes: true }, cb));
check("readdir buffer", (cb) => fs.readdir(at("dir"), "buffer", cb));
check("readdir recursive", (cb) => fs.readdir(at("dir"), { recursive: true }, (err, names) => cb(err, names && names.sort())));
check("readdir missing", (cb) => fs.readdir(at("missing"), cb));
check("readdir file", (cb) => fs.readdir(at("a.txt"), cb));
check("rmdir not empty", (cb) => fs.rmdir(at("dir"), cb));
check("rmdir missing", (cb) => fs.rmdir(at("missing"), cb));

// Links.
check("symlink", (cb) => fs.symlink("a.txt", at("l"), cb));
check("symlink existing", (cb) => fs.symlink("a.txt", at("l"), cb));
check("readlink", (cb) => fs.readlink(at("l"), cb));
check("readlink buffer", (cb) => fs.readlink(at("l"), "buffer", cb));
check("readlink file", (cb) => fs.readlink(at("a.txt"), cb));
check("lstat link", (cb) => fs.lstat(at("l"), (err, s) => cb(err, s.isSymbolicLink())));
check("realpath", (cb) => fs.realpath(at("l"), (err, real) => cb(err, real === fs.realpathSync(at("a.txt")))));
check("realpath.native", (cb) => fs.realpath.native(at("l"), (err, real) => cb(err, real === fs.realpathSync(at("a.txt")))));
check("realpath missing", (cb) => fs.realpath(at("missing"), cb));
check("realpath.native missing", (cb) => fs.realpath.native(at("missing"), cb));
check("link", (cb) => fs.link(at("a.txt"), at("hard"), cb));
check("link existing", (cb) => fs.link(at("a.txt"), at("hard"), cb));
check("lutimes", (cb) => fs.lutimes(at("l"), 7, 8, cb));
check("lchown self", (cb) => { const { uid, gid } = fs.lstatSync(at("l")); fs.lchown(at("l"), uid, gid, cb); });
check("chown self", (cb) => { const { uid, gid } = fs.statSync(at("a.txt")); fs.chown(at("a.txt"), uid, gid, cb); });

// Renaming, copying and removing.
check("rename", (cb) => fs.rename(at("hard"), at("moved"), cb));
check("rename missing", (cb) => fs.rename(at("missing"), at("x"), cb));
check("copyFile", (cb) => fs.copyFile(at("a.txt"), at("copy"), cb));
check("copyFile EXCL", (cb) => fs.copyFile(at("a.txt"), at("copy"), fs.constants.COPYFILE_EXCL, cb));
check("copyFile missing", (cb) => fs.copyFile(at("missing"), at("x"), cb));
check("truncate", (cb) => fs.truncate(at("copy"), 2, cb));
check("read truncated", (cb) => fs.readFile(at("copy"), cb));
check("unlink", (cb) => fs.unlink(at("copy"), cb));
check("unlink missing", (cb) => fs.unlink(at("copy"), cb));
check("rm file", (cb) => fs.rm(at("moved"), cb));
check("rm missing", (cb) => fs.rm(at("moved"), cb));
check("rm missing force", (cb) => fs.rm(at("moved"), { force: true }, cb));
check("rm dir", (cb) => fs.rm(at("dir"), cb));
check("rm dir recursive", (cb) => fs.rm(at("dir"), { recursive: true }, cb));
check("left", (cb) => fs.readdir(root, (err, names) => cb(err, names && names.map((n) => n.replace(/^t-.{6}$/, "t-XXXXXX")))));

let i = 0;
(function next() {
  if (i < cases.length) cases[i++](next);
  else fs.rm(root, { recursive: true, force: true }, (err) => console.log("cleaned up", err, fs.existsSync(root)));
})();
