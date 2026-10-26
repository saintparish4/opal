import path from 'node:path';

function report() {
  const rel = (p) => path.relative(process.cwd(), p);
  console.log("other", rel(import.meta.filename), rel(import.meta.dirname), import.meta.main, import.meta.url === new URL("./other.mjs", import.meta.url).href);
}

// Rule 9: import.meta of each module is the original module's.
const rel = (p) => path.relative(process.cwd(), p);
console.log("main", new URL(import.meta.url).pathname.endsWith("/main.mjs"), rel(import.meta.filename), rel(import.meta.dirname) || ".", import.meta.main);
console.log("resolve", new URL(import.meta.resolve("./sub/other.mjs")).pathname.endsWith("/sub/other.mjs"));
report();
