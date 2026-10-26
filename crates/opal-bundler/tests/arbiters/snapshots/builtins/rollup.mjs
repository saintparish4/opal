import fs, { readFileSync } from 'node:fs';
import * as pathNs from 'path';
import { EventEmitter } from 'events';

// Rule 12: builtins stay Node's, named and default imports alike.
console.log(typeof fs.readFileSync, readFileSync === fs.readFileSync, typeof pathNs.join, typeof EventEmitter);
console.log(readFileSync(new URL("./main.mjs", import.meta.url), "utf8").split("\n")[0]);
