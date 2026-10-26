// Rule 9: import.meta of each module is the original module's.
import { report } from "./sub/other.mjs";
import path from "node:path";
const rel = (p) => path.relative(process.cwd(), p);
console.log("main", new URL(import.meta.url).pathname.endsWith("/main.mjs"), rel(import.meta.filename), rel(import.meta.dirname) || ".", import.meta.main);
console.log("resolve", new URL(import.meta.resolve("./sub/other.mjs")).pathname.endsWith("/sub/other.mjs"));
report();
