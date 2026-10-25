// main.mjs
import fs, { readFileSync } from "node:fs";
import * as pathNs from "path";
import { EventEmitter } from "events";
console.log(typeof fs.readFileSync, readFileSync === fs.readFileSync, typeof pathNs.join, typeof EventEmitter);
console.log(readFileSync(new URL("./main.mjs", import.meta.url), "utf8").split("\n")[0]);
