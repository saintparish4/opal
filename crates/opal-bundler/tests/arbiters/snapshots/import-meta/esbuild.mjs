// sub/other.mjs
import path from "node:path";
function report() {
  const rel2 = (p) => path.relative(process.cwd(), p);
  console.log("other", rel2(import.meta.filename), rel2(import.meta.dirname), import.meta.main, import.meta.url === new URL("./other.mjs", import.meta.url).href);
}

// main.mjs
import path2 from "node:path";
var rel = (p) => path2.relative(process.cwd(), p);
console.log("main", new URL(import.meta.url).pathname.endsWith("/main.mjs"), rel(import.meta.filename), rel(import.meta.dirname) || ".", import.meta.main);
console.log("resolve", new URL(import.meta.resolve("./sub/other.mjs")).pathname.endsWith("/sub/other.mjs"));
report();
