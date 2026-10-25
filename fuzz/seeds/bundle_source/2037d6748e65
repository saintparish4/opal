import { bump, count } from "./lib.mjs";

console.log("main start", count);
bump();
// A live binding: the import reads lib's `count` after bump() changed it.
console.log("after bump", count);
console.error("to stderr");
throw new TypeError("done");
