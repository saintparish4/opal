// fs.promises: what each call resolves to and what each rejects with, on
// files this program makes in a fresh temporary directory (printed as
// <dir>), FileHandle and Dir included. The calls run one after another, so
// the output is the same on every run.
const fs = require("fs");
const fsp = require("fs/promises");
const path = require("path");

const root = fs.mkdtempSync(`${process.env.TMPDIR || "/tmp"}/opal-fs-promises-`);
const at = (name) => path.join(root, name);
// mkdtemp's random suffix is printed as RANDOM.
const clean = (value) =>
  typeof value === "string" ? value.split(root).join("<dir>").replace(/t-(?!XXXXXX)[A-Za-z0-9]{6}/g, "t-RANDOM") : value;

function shown(value) {
  if (Buffer.isBuffer(value)) return `<Buffer ${value.toString("hex")}>`;
  if (value instanceof fs.Stats) return `Stats(size=${value.size}, file=${value.isFile()}, dir=${value.isDirectory()})`;
  if (value instanceof fs.Dirent) return `Dirent(${value.name}, file=${value.isFile()})`;
  if (Array.isArray(value)) return `[${value.map(shown).join(", ")}]`;
  if (typeof value === "string") return JSON.stringify(clean(value));
  if (typeof value === "bigint") return `${value}n`;
  if (value && typeof value === "object") return `${value.constructor.name}{${Object.keys(value).join(",")}}`;
  return String(value);
}

function failure(error) {
  const fields = Object.keys(error).map((key) => `${key}=${clean(String(error[key]))}`);
  const frames = error.stack.split("\n").length - 1;
  return `${error.name}: ${clean(error.message)} [${fields.join(" ")}] frames=${frames}`;
}

const cases = [];
function check(label, start) {
  cases.push(async () => {
    let promise;
    try {
      promise = start();
    } catch (error) {
      console.log(`${label} threw ${error.name}: ${clean(error.message)} [code=${error.code}]`);
      return;
    }
    const kind = promise instanceof Promise ? "" : " (not a promise)";
    try {
      console.log(`${label}${kind}: ${shown(await promise)}`);
    } catch (error) {
      console.log(`${label}${kind} rejected ${failure(error)}`);
    }
  });
}

// Whole files.
check("writeFile", () => fsp.writeFile(at("a.txt"), "héllo\n"));
check("readFile utf8", () => fsp.readFile(at("a.txt"), "utf8"));
check("readFile buffer", () => fsp.readFile(at("a.txt")));
check("readFile hex", () => fsp.readFile(at("a.txt"), { encoding: "hex" }));
check("appendFile", () => fsp.appendFile(at("a.txt"), "more"));
check("readFile after append", () => fsp.readFile(at("a.txt"), "latin1"));
check("readFile missing", () => fsp.readFile(at("missing")));
check("readFile dir", () => fsp.readFile(root));
check("readFile bad options", () => fsp.readFile(at("a.txt"), 5));
check("writeFile flag wx", () => fsp.writeFile(at("a.txt"), "x", { flag: "wx" }));
check("writeFile no dir", () => fsp.writeFile(at("no/file"), "x"));
check("writeFile iterable", () => fsp.writeFile(at("it.txt"), ["a", "b", "c"]));
check("readFile iterable", () => fsp.readFile(at("it.txt"), "utf8"));
check("writeFile bad data", () => fsp.writeFile(at("bad.txt"), 5));
check("readFile big", async () => {
  const data = Buffer.alloc(300000, "abc");
  await fsp.writeFile(at("big"), data);
  const back = await fsp.readFile(at("big"));
  return `${back.equals(data)} ${back.length}`;
});

// FileHandle.
let handle;
check("open", async () => {
  handle = await fsp.open(at("a.txt"), "r+");
  return `${handle.constructor.name} ${typeof handle.fd}`;
});
check("handle.read", async () => {
  const buffer = Buffer.alloc(4);
  const { bytesRead, buffer: same } = await handle.read(buffer, 0, 4, 0);
  return `${bytesRead} ${shown(buffer)} ${same === buffer}`;
});
check("handle.read options", async () => {
  const result = await handle.read({ buffer: Buffer.alloc(3), position: 1 });
  return `${Object.keys(result)} ${result.bytesRead} ${shown(result.buffer)}`;
});
check("handle.read default", async () => {
  const { bytesRead, buffer } = await handle.read();
  return `${bytesRead} ${buffer.length}`;
});
check("handle.read at end", async () => (await handle.read(Buffer.alloc(4), 0, 4, 1000)).bytesRead);
check("handle.readv", async () => {
  const a = Buffer.alloc(2);
  const b = Buffer.alloc(3);
  const result = await handle.readv([a, b], 0);
  return `${Object.keys(result)} ${result.bytesRead} ${shown(a)} ${shown(b)}`;
});
check("handle.write string", async () => {
  const result = await handle.write("ZZ", 0);
  return `${Object.keys(result)} ${result.bytesWritten} ${shown(result.buffer)}`;
});
check("handle.write buffer", async () => {
  const result = await handle.write(Buffer.from("yy"), 0, 2, 2);
  return `${result.bytesWritten} ${shown(result.buffer)}`;
});
check("handle.writev", async () => {
  const result = await handle.writev([Buffer.from("1"), Buffer.from("2")], 4);
  return `${Object.keys(result)} ${result.bytesWritten}`;
});
check("handle.readFile", () => handle.readFile("utf8"));
check("handle.stat", () => handle.stat());
check("handle.stat bigint", async () => typeof (await handle.stat({ bigint: true })).size);
check("handle.truncate", () => handle.truncate(4));
check("handle.sync", () => handle.sync());
check("handle.datasync", () => handle.datasync());
check("handle.chmod", () => handle.chmod(0o640));
check("handle.utimes", () => handle.utimes(5, 6));
check("handle.appendFile", () => handle.appendFile("!"));
check("handle.writeFile", () => handle.writeFile("rewritten"));
check("readFile after handle.writeFile", () => fsp.readFile(at("a.txt"), "utf8"));
check("handle.close", () => handle.close());
check("handle.fd after close", () => Promise.resolve(handle.fd));
check("handle.close again", () => handle.close());
check("handle.read closed", () => handle.read(Buffer.alloc(1), 0, 1, 0));
check("handle.stat closed", () => handle.stat());
check("open missing", () => fsp.open(at("missing"), "r"));
check("open bad flag", () => fsp.open(at("a.txt"), "q"));
check("open dir", async () => {
  const dir = await fsp.open(root, "r");
  const stats = await dir.stat();
  await dir.close();
  return stats.isDirectory();
});
check("readFile of handle", async () => {
  const fh = await fsp.open(at("a.txt"));
  try {
    return await fsp.readFile(fh, "utf8");
  } finally {
    await fh.close();
  }
});
check("writeFile to handle", async () => {
  const fh = await fsp.open(at("h.txt"), "w");
  await fsp.writeFile(fh, "via handle");
  await fh.close();
  return fsp.readFile(at("h.txt"), "utf8");
});
check("handle Symbol.asyncDispose", async () => {
  const fh = await fsp.open(at("a.txt"));
  await fh[Symbol.asyncDispose]();
  return fh.fd;
});

// Stats.
check("stat", () => fsp.stat(at("a.txt")));
check("stat dir", () => fsp.stat(root));
check("stat bigint", async () => typeof (await fsp.stat(at("a.txt"), { bigint: true })).mtimeNs);
check("stat missing", () => fsp.stat(at("missing")));
check("lstat missing", () => fsp.lstat(at("missing")));
check("statfs", async () => {
  const s = await fsp.statfs(root);
  return `${Object.keys(s)} ${Object.values(s).map((v) => typeof v)}`;
});
check("statfs bigint", async () => Object.values(await fsp.statfs(root, { bigint: true })).map((v) => typeof v).join());
check("chmod", () => fsp.chmod(at("a.txt"), 0o600));
check("chmod missing", () => fsp.chmod(at("missing"), 0o600));
check("mode after chmod", async () => ((await fsp.stat(at("a.txt"))).mode & 0o777).toString(8));
check("utimes", () => fsp.utimes(at("a.txt"), 1000000000, 1000000000));
check("mtime after utimes", async () => (await fsp.stat(at("a.txt"))).mtimeMs);
check("access", () => fsp.access(at("a.txt")));
check("access missing", () => fsp.access(at("missing")));
check("access bad mode", () => fsp.access(at("a.txt"), 99));

// Directories.
check("mkdir", () => fsp.mkdir(at("dir")));
check("mkdir existing", () => fsp.mkdir(at("dir")));
check("mkdir recursive", () => fsp.mkdir(at("dir/x/y"), { recursive: true }));
check("mkdir recursive existing", () => fsp.mkdir(at("dir/x"), { recursive: true }));
check("mkdir through file", () => fsp.mkdir(at("a.txt/sub"), { recursive: true }));
check("mkdtemp", async () => path.basename(await fsp.mkdtemp(at("t-"))).length);
check("mkdtemp missing", () => fsp.mkdtemp(at("missing/t-")));
check("fill dir", async () => {
  for (const name of ["b", "a", "C"]) await fsp.writeFile(at(`dir/${name}`), name);
});
check("readdir", () => fsp.readdir(at("dir")));
check("readdir withFileTypes", () => fsp.readdir(at("dir"), { withFileTypes: true }));
check("readdir buffer", () => fsp.readdir(at("dir"), "buffer"));
check("readdir recursive", async () => (await fsp.readdir(at("dir"), { recursive: true })).sort());
check("readdir recursive withFileTypes", async () =>
  (await fsp.readdir(at("dir"), { recursive: true, withFileTypes: true })).map((d) => d.name).sort());
check("readdir missing", () => fsp.readdir(at("missing")));
check("readdir file", () => fsp.readdir(at("a.txt")));
check("opendir", async () => {
  const dir = await fsp.opendir(at("dir"));
  const names = [];
  for await (const entry of dir) names.push(`${entry.name}:${entry.isDirectory()}`);
  return `${dir.constructor.name} ${clean(dir.path)} ${names.sort()}`;
});
check("opendir read", async () => {
  const dir = await fsp.opendir(at("dir"), { bufferSize: 1 });
  const names = [];
  let entry;
  while ((entry = await dir.read()) !== null) names.push(entry.name);
  await dir.close();
  return names.sort();
});
check("opendir close twice", async () => {
  const dir = await fsp.opendir(at("dir"));
  await dir.close();
  return dir.close();
});
check("opendir read closed", async () => {
  const dir = await fsp.opendir(at("dir"));
  await dir.close();
  return dir.read();
});
check("opendir missing", () => fsp.opendir(at("missing")));
check("opendir file", () => fsp.opendir(at("a.txt")));
check("opendir recursive", async () => {
  const names = [];
  for await (const entry of await fsp.opendir(at("dir"), { recursive: true })) names.push(entry.name);
  return names.sort();
});
check("opendirSync", () => {
  const dir = fs.opendirSync(at("dir"));
  const names = [];
  let entry;
  while ((entry = dir.readSync()) !== null) names.push(entry.name);
  dir.closeSync();
  return Promise.resolve(names.sort());
});
check("opendir callback", () =>
  new Promise((resolve, reject) =>
    fs.opendir(at("dir"), (err, dir) => {
      if (err) return reject(err);
      dir.read((err, entry) => {
        if (err) return reject(err);
        dir.close((err) => (err ? reject(err) : resolve(typeof entry.name)));
      });
    }),
  ),
);
check("rmdir not empty", () => fsp.rmdir(at("dir")));
check("rmdir missing", () => fsp.rmdir(at("missing")));

// Links.
check("symlink", () => fsp.symlink("a.txt", at("l")));
check("symlink existing", () => fsp.symlink("a.txt", at("l")));
check("readlink", () => fsp.readlink(at("l")));
check("readlink buffer", () => fsp.readlink(at("l"), "buffer"));
check("readlink file", () => fsp.readlink(at("a.txt")));
check("lstat link", async () => (await fsp.lstat(at("l"))).isSymbolicLink());
check("realpath", async () => (await fsp.realpath(at("l"))) === fs.realpathSync(at("a.txt")));
check("realpath missing", () => fsp.realpath(at("missing")));
check("link", () => fsp.link(at("a.txt"), at("hard")));
check("link existing", () => fsp.link(at("a.txt"), at("hard")));
check("lutimes", () => fsp.lutimes(at("l"), 7, 8));
check("lchown self", () => {
  const { uid, gid } = fs.lstatSync(at("l"));
  return fsp.lchown(at("l"), uid, gid);
});
check("chown self", () => {
  const { uid, gid } = fs.statSync(at("a.txt"));
  return fsp.chown(at("a.txt"), uid, gid);
});

// Renaming, copying and removing.
check("rename", () => fsp.rename(at("hard"), at("moved")));
check("rename missing", () => fsp.rename(at("missing"), at("x")));
check("copyFile", () => fsp.copyFile(at("a.txt"), at("copy")));
check("copyFile EXCL", () => fsp.copyFile(at("a.txt"), at("copy"), fs.constants.COPYFILE_EXCL));
check("copyFile missing", () => fsp.copyFile(at("missing"), at("x")));
check("truncate", () => fsp.truncate(at("copy"), 2));
check("read truncated", () => fsp.readFile(at("copy")));
check("unlink", () => fsp.unlink(at("copy")));
check("unlink missing", () => fsp.unlink(at("copy")));
check("rm file", () => fsp.rm(at("moved")));
check("rm missing", () => fsp.rm(at("moved")));
check("rm missing force", () => fsp.rm(at("moved"), { force: true }));
check("rm dir", () => fsp.rm(at("dir")));
check("rm dir recursive", () => fsp.rm(at("dir"), { recursive: true }));
check("left", async () => (await fsp.readdir(root)).map((n) => n.replace(/^t-.{6}$/, "t-XXXXXX")).sort());
check("constants", () => Promise.resolve(fsp.constants === fs.constants));
check("same object", () => Promise.resolve(fsp === fs.promises && fsp === require("node:fs/promises")));

(async () => {
  for (const run of cases) await run();
  await fsp.rm(root, { recursive: true, force: true });
  console.log("cleaned up", fs.existsSync(root));
})();
