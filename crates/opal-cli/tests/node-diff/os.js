// node:os: what each function returns on this machine. Values that change
// from one moment to the next (free memory, load, uptime, CPU times and
// speeds) are printed by type and shape only.
const os = require("os");

function show(label, f) {
  try {
    const value = f();
    console.log(`${label}: ${typeof value === "string" ? JSON.stringify(value) : value}`);
  } catch (error) {
    console.log(`${label} threw ${error.name}: ${error.message} [code=${error.code}]`);
  }
}

show("hostname", () => os.hostname());
show("type", () => os.type());
show("platform", () => os.platform());
show("release", () => os.release());
show("version", () => os.version());
show("machine", () => os.machine());
show("arch", () => os.arch());
show("endianness", () => os.endianness());
show("EOL", () => os.EOL);
show("devNull", () => os.devNull);
show("homedir", () => os.homedir());
show("totalmem", () => os.totalmem());
show("freemem", () => `${typeof os.freemem()} ${os.freemem() > 0 && os.freemem() <= os.totalmem()}`);
show("uptime", () => `${typeof os.uptime()} ${os.uptime() > 0}`);
show("loadavg", () => os.loadavg().map((n) => typeof n).join());
show("availableParallelism", () => os.availableParallelism());

const cpus = os.cpus();
show("cpus", () => `${cpus.length} ${[...new Set(cpus.map((c) => c.model))].join("|")}`);
show("cpu keys", () => cpus.map((c) => `${Object.keys(c)}/${Object.keys(c.times)}`)[0]);
show("cpu types", () => cpus.every((c) => typeof c.speed === "number" && Object.values(c.times).every(Number.isInteger)));

const interfaces = os.networkInterfaces();
for (const [name, addresses] of Object.entries(interfaces)) {
  for (const a of addresses) {
    console.log(`interface ${name}: ${Object.keys(a)} ${a.address} ${a.netmask} ${a.family} ${a.mac} ${a.internal} ${a.cidr} ${a.scopeid}`);
  }
}

const info = os.userInfo();
show("userInfo", () => `${Object.getPrototypeOf(info)} ${Object.keys(info)} ${info.uid} ${info.gid} ${info.username} ${info.homedir} ${info.shell}`);
const raw = os.userInfo({ encoding: "buffer" });
show("userInfo buffer", () => `${Buffer.isBuffer(raw.username)} ${raw.username} ${raw.homedir}`);
show("userInfo bad", () => os.userInfo(5));

show("getPriority", () => os.getPriority());
show("getPriority self", () => os.getPriority(process.pid) === os.getPriority());
show("getPriority missing", () => os.getPriority(2 ** 30));
show("getPriority bad", () => os.getPriority("x"));
show("setPriority missing", () => os.setPriority(2 ** 30, 10));
show("setPriority range", () => os.setPriority(0, 30));
show("setPriority same", () => os.setPriority(os.getPriority()));

show("constants signals", () => Object.entries(os.constants.signals).map(([k, v]) => `${k}=${v}`).join(" "));
show("constants errno", () => Object.keys(os.constants.errno).length);
show("constants priority", () => JSON.stringify(os.constants.priority));
show("constants dlopen", () => JSON.stringify(os.constants.dlopen));
show("constants UV_UDP_REUSEADDR", () => os.constants.UV_UDP_REUSEADDR);
show("frozen", () => `${Object.isFrozen(os.constants)} ${Object.isFrozen(os.constants.signals)}`);

// os.tmpdir() reads TMPDIR, TMP and TEMP, in that order, each time.
for (const [name, value] of [["TMPDIR", "/tmpdir/"], ["TMPDIR", ""], ["TMP", "/tmp2"], ["TMP", ""], ["TEMP", "/temp//"], ["TEMP", ""], ["TMPDIR", "/"]]) {
  process.env[name] = value;
  show(`tmpdir ${name}=${JSON.stringify(value)}`, () => os.tmpdir());
}
delete process.env.TMPDIR;
show("tmpdir unset", () => os.tmpdir());
const home = process.env.HOME;
process.env.HOME = "/somewhere";
show("homedir HOME", () => os.homedir());
process.env.HOME = "";
show("homedir empty HOME", () => os.homedir());
delete process.env.HOME;
show("homedir no HOME", () => os.homedir() === os.userInfo().homedir);
process.env.HOME = home;

show("String(hostname)", () => `${os.hostname}`.length > 0 && `${os.hostname}` === os.hostname());
show("keys", () => Object.keys(os).sort().join());
