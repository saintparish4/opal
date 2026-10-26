export * from "./shared.mjs";
import * as self from "./lazy.mjs";
console.log(Object.keys(self), self.b);
const b = "lazy's own b";
console.log(b);
