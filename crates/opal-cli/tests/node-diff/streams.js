// node:stream, fs's streams, AbortController and EventTarget: the order
// events arrive in and what they carry, run one case after another so the
// output is the same on every run. Files are made in a fresh temporary
// directory, printed as <dir>.
const stream = require("stream");
const { Readable, Writable, Duplex, Transform, PassThrough, pipeline, finished } = stream;
const streamPromises = require("stream/promises");
const fs = require("fs");
const path = require("path");
const { StringDecoder } = require("string_decoder");

const root = fs.mkdtempSync(`${process.env.TMPDIR || "/tmp"}/opal-streams-`);
const at = (name) => path.join(root, name);
const clean = (value) => String(value).split(root).join("<dir>");
const failure = (error) => `${error.name} [${error.code}]: ${clean(error.message)}`;

const cases = [];
function check(label, run) {
  cases.push(async () => {
    const log = (...parts) => console.log(`${label}: ${parts.map(clean).join(" ")}`);
    try {
      await run(log);
    } catch (error) {
      log(`threw ${failure(error)}`);
    }
  });
}

// Records each event a stream emits, in order.
function watch(s, log, events = ["data", "end", "finish", "close", "error", "pause", "resume", "drain", "prefinish"]) {
  for (const event of events) {
    s.on(event, (arg) => {
      if (event === "data") log(`data ${JSON.stringify(Buffer.isBuffer(arg) ? `<${arg.toString("hex")}>` : arg)}`);
      else if (event === "error") log(`error ${failure(arg)}`);
      else log(event);
    });
  }
}

const done = (s) => new Promise((resolve) => finished(s, (err) => resolve(err)));

check("readable from array", async (log) => {
  const r = Readable.from(["a", "b", "c"]);
  watch(r, log);
  await done(r);
});
check("readable push", async (log) => {
  const r = new Readable({ read() {} });
  watch(r, log);
  r.push("one");
  r.push(Buffer.from("two"));
  r.push(null);
  await done(r);
});
check("readable encoding", async (log) => {
  const r = new Readable({ read() {} });
  r.setEncoding("utf8");
  watch(r, log, ["data", "end"]);
  const euro = Buffer.from("€");
  r.push(euro.subarray(0, 1));
  r.push(euro.subarray(1));
  r.push(null);
  await done(r);
});
check("readable object mode", async (log) => {
  const r = new Readable({ objectMode: true, read() {} });
  r.on("data", (d) => log(`data ${JSON.stringify(d)}`));
  r.push({ a: 1 });
  r.push([2]);
  r.push(null);
  await done(r);
  log(r.readableObjectMode, r.readableEnded);
});
check("readable paused read()", async (log) => {
  const r = new Readable({ read() {} });
  r.push("abc");
  r.push("def");
  r.push(null);
  r.on("readable", () => {
    let chunk;
    while ((chunk = r.read(2)) !== null) log(`read ${chunk}`);
  });
  await done(r);
});
check("readable async iteration", async (log) => {
  const chunks = [];
  for await (const chunk of Readable.from(["x", "y"])) chunks.push(chunk);
  log(chunks.join(","));
});
check("readable error", async (log) => {
  const r = new Readable({ read() { this.destroy(new Error("boom")); } });
  watch(r, log);
  r.resume();
  log(`finished ${failure(await done(r))}`);
});
check("readable destroy", async (log) => {
  const r = new Readable({ read() {} });
  watch(r, log);
  r.destroy();
  log(r.destroyed, r.readableAborted);
  await done(r);
});
check("readable operators", async (log) => {
  const result = await Readable.from([1, 2, 3, 4]).map((n) => n * 2).filter((n) => n > 2).toArray();
  log(JSON.stringify(result));
  log(await Readable.from([1, 2, 3]).reduce((a, b) => a + b, 0));
});
check("writable", async (log) => {
  const w = new Writable({
    highWaterMark: 4,
    write(chunk, encoding, cb) {
      log(`write ${chunk} ${encoding}`);
      setImmediate(cb);
    },
  });
  watch(w, log);
  log(`write returned ${w.write("ab")}`);
  log(`write returned ${w.write("cdef")}`);
  w.end("g");
  await done(w);
  log(w.writableFinished, w.writableLength);
});
check("writable decodeStrings false", async (log) => {
  const w = new Writable({
    decodeStrings: false,
    write(chunk, encoding, cb) {
      log(`write ${typeof chunk} ${encoding}`);
      cb();
    },
  });
  w.write("text", "latin1");
  w.end();
  await done(w);
});
check("writable writev", async (log) => {
  const w = new Writable({
    writev(chunks, cb) {
      log(`writev ${chunks.map((c) => c.chunk).join("|")}`);
      cb();
    },
  });
  w.cork();
  w.write("a");
  w.write("b");
  process.nextTick(() => w.uncork());
  await new Promise((resolve) => setImmediate(resolve));
  w.end();
  await done(w);
});
check("writable error", async (log) => {
  const w = new Writable({ write(chunk, enc, cb) { cb(new Error("nope")); } });
  watch(w, log, ["error", "close", "finish"]);
  w.write("x", (err) => log(`write cb ${err && err.message}`));
  await done(w);
});
check("write after end", async (log) => {
  const w = new Writable({ write(chunk, enc, cb) { cb(); } });
  watch(w, log, ["error", "close", "finish"]);
  w.end();
  w.write("late", (err) => log(`write cb ${failure(err)}`));
  await done(w);
});
check("transform", async (log) => {
  const upper = new Transform({
    transform(chunk, encoding, cb) {
      cb(null, String(chunk).toUpperCase());
    },
    flush(cb) {
      this.push("!");
      cb();
    },
  });
  upper.setEncoding("utf8");
  watch(upper, log, ["data", "end", "finish", "close"]);
  upper.write("ab");
  upper.end("cd");
  await done(upper);
});
check("duplex", async (log) => {
  const d = new Duplex({
    read() {
      this.push("from duplex");
      this.push(null);
    },
    write(chunk, enc, cb) {
      log(`duplex got ${chunk}`);
      cb();
    },
  });
  d.setEncoding("utf8");
  watch(d, log, ["data", "end", "finish", "close"]);
  d.end("to duplex");
  await done(d);
});
check("pipe", async (log) => {
  const out = [];
  const w = new Writable({ write(chunk, enc, cb) { out.push(String(chunk)); cb(); } });
  Readable.from(["p", "i", "p", "e"]).pipe(w);
  await done(w);
  log(out.join(""));
});
check("pipeline callback", async (log) => {
  const out = [];
  await new Promise((resolve) =>
    pipeline(
      Readable.from(["a", "b"]),
      new Transform({ transform(c, e, cb) { cb(null, `${c}${c}`); } }),
      new Writable({ write(c, e, cb) { out.push(String(c)); cb(); } }),
      (err) => {
        log(`pipeline done ${err}`, out.join(""));
        resolve();
      },
    ),
  );
});
check("pipeline error", async (log) => {
  await new Promise((resolve) =>
    pipeline(
      Readable.from(["a"]),
      new Transform({ transform(c, e, cb) { cb(new Error("bad transform")); } }),
      new PassThrough(),
      (err) => {
        log(`pipeline done ${failure(err)}`);
        resolve();
      },
    ),
  );
});
check("pipeline promise with generator", async (log) => {
  const out = [];
  await streamPromises.pipeline(
    Readable.from(["1", "2"]),
    async function* (source) {
      for await (const chunk of source) yield `<${chunk}>`;
    },
    new Writable({ write(c, e, cb) { out.push(String(c)); cb(); } }),
  );
  log(out.join(""));
});
check("finished promise", async (log) => {
  const r = Readable.from([]);
  r.resume();
  await streamPromises.finished(r);
  log("finished");
});
check("addAbortSignal", async (log) => {
  const controller = new AbortController();
  const r = stream.addAbortSignal(controller.signal, new Readable({ read() {} }));
  watch(r, log, ["error", "close"]);
  controller.abort();
  await done(r);
});
check("stream utilities", async (log) => {
  const r = new Readable({ read() {} });
  log(stream.isReadable(r), stream.isErrored(r), stream.isDisturbed(r), Readable.getDefaultHighWaterMark(false), Readable.getDefaultHighWaterMark(true));
  log(typeof stream.Stream, stream === stream.Stream, stream.promises === streamPromises);
  log(r instanceof stream.Stream, r instanceof require("events"));
});

// fs streams.
check("createReadStream", async (log) => {
  fs.writeFileSync(at("in.txt"), "0123456789abcdef");
  const r = fs.createReadStream(at("in.txt"), { highWaterMark: 6 });
  watch(r, log, ["open", "ready", "data", "end", "close", "error"]);
  await done(r);
  log(r.bytesRead, clean(r.path));
});
check("createReadStream range", async (log) => {
  const r = fs.createReadStream(at("in.txt"), { start: 2, end: 5, encoding: "utf8" });
  watch(r, log, ["data", "end", "close"]);
  await done(r);
});
check("createReadStream missing", async (log) => {
  const r = fs.createReadStream(at("missing"));
  watch(r, log, ["open", "error", "close"]);
  await done(r);
});
check("createReadStream bad start", async (log) => {
  fs.createReadStream(at("in.txt"), { start: 5, end: 2 });
});
check("createReadStream fd", async (log) => {
  const fd = fs.openSync(at("in.txt"), "r");
  const r = fs.createReadStream(null, { fd, start: 10, encoding: "utf8" });
  watch(r, log, ["data", "close"]);
  await done(r);
});
check("createWriteStream", async (log) => {
  const w = fs.createWriteStream(at("out.txt"));
  watch(w, log, ["open", "ready", "finish", "close", "error"]);
  w.write("hello ");
  w.write(Buffer.from("world"));
  w.end("\n");
  await done(w);
  log(w.bytesWritten, JSON.stringify(fs.readFileSync(at("out.txt"), "utf8")));
});
check("createWriteStream append", async (log) => {
  const w = fs.createWriteStream(at("out.txt"), { flags: "a" });
  w.end("again\n");
  await done(w);
  log(JSON.stringify(fs.readFileSync(at("out.txt"), "utf8")));
});
check("createWriteStream no dir", async (log) => {
  const w = fs.createWriteStream(at("no/out.txt"));
  watch(w, log, ["open", "error", "close"]);
  w.write("x");
  await done(w);
});
check("fs stream pipeline", async (log) => {
  await streamPromises.pipeline(fs.createReadStream(at("in.txt")), fs.createWriteStream(at("copy.txt")));
  log(fs.readFileSync(at("copy.txt"), "utf8"));
});
check("filehandle streams", async (log) => {
  const fh = await fs.promises.open(at("in.txt"));
  const r = fh.createReadStream({ start: 12 });
  r.setEncoding("utf8");
  watch(r, log, ["data", "close"]);
  await done(r);
  log(fh.fd);
});
check("filehandle readLines", async (log) => {
  fs.writeFileSync(at("lines.txt"), "one\ntwo\r\n\nthree");
  const fh = await fs.promises.open(at("lines.txt"));
  const lines = [];
  for await (const line of fh.readLines()) lines.push(line);
  log(JSON.stringify(lines));
});
check("readFile signal", async (log) => {
  await fs.promises.readFile(at("in.txt"), { signal: AbortSignal.abort() });
});

// AbortController, AbortSignal, EventTarget.
check("AbortController", async (log) => {
  const controller = new AbortController();
  const { signal } = controller;
  signal.addEventListener("abort", (event) => log(`abort event ${event.type} ${event.isTrusted}`));
  signal.onabort = () => log("onabort");
  log(signal.aborted, signal.reason);
  controller.abort();
  log(signal.aborted, failure(signal.reason), signal.reason instanceof DOMException);
  controller.abort("again");
  log(String(signal.reason.name));
  try {
    signal.throwIfAborted();
  } catch (error) {
    log(`throwIfAborted ${error.name}`);
  }
});
check("AbortSignal.abort reason", async (log) => {
  log(AbortSignal.abort("why").reason, AbortSignal.abort().reason.name);
});
check("AbortSignal.any", async (log) => {
  const a = new AbortController();
  const any = AbortSignal.any([a.signal, new AbortController().signal]);
  a.abort("first");
  log(any.aborted, any.reason);
});
check("AbortSignal.timeout", async (log) => {
  // Its timer is unref'd, so something else keeps the loop alive meanwhile.
  const keepAlive = setTimeout(() => {}, 10000);
  const signal = AbortSignal.timeout(5);
  await new Promise((resolve) => signal.addEventListener("abort", resolve));
  clearTimeout(keepAlive);
  log(signal.reason.name, signal.reason.message);
});
check("EventTarget", async (log) => {
  const target = new EventTarget();
  const listener = (event) => log(`got ${event.type} ${event.eventPhase} ${event.target === target} ${event.cancelable}`);
  target.addEventListener("ping", listener);
  target.addEventListener("ping", listener);
  target.addEventListener("ping", () => log("once"), { once: true });
  log(target.dispatchEvent(new Event("ping", { cancelable: true })));
  target.dispatchEvent(new Event("ping"));
  target.removeEventListener("ping", listener);
  log(target.dispatchEvent(new Event("ping")));
  const custom = new CustomEvent("c", { detail: { x: 1 } });
  log(custom.detail.x, custom instanceof Event, Object.prototype.toString.call(custom));
});
check("Event defaultPrevented", async (log) => {
  const target = new EventTarget();
  target.addEventListener("e", (event) => event.preventDefault());
  const event = new Event("e", { cancelable: true });
  log(target.dispatchEvent(event), event.defaultPrevented);
});
check("StringDecoder", async (log) => {
  const decoder = new StringDecoder("utf8");
  const bytes = Buffer.from("aé€😀");
  log(JSON.stringify([...bytes].map((b) => decoder.write(Buffer.from([b]))).join("|")), JSON.stringify(decoder.end()));
  const hex = new StringDecoder("hex");
  log(hex.write(Buffer.from([1, 255])), hex.end(Buffer.from([16])));
  const b64 = new StringDecoder("base64");
  log(b64.write(Buffer.from("ab")), b64.end());
  const u16 = new StringDecoder("utf16le");
  log(JSON.stringify(u16.write(Buffer.from([0x3d, 0xd8]))), JSON.stringify(u16.end(Buffer.from([0x00, 0xde]))));
  log(JSON.stringify(new StringDecoder("utf8").end(Buffer.from([0xe2, 0x82]))));
});

(async () => {
  for (const run of cases) await run();
  fs.rmSync(root, { recursive: true, force: true });
  console.log("cleaned up", fs.existsSync(root));
})();
