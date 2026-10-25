import path from "node:path";
export function report() {
  const rel = (p) => path.relative(process.cwd(), p);
  console.log("other", rel(import.meta.filename), rel(import.meta.dirname), import.meta.main, import.meta.url === new URL("./other.mjs", import.meta.url).href);
}
