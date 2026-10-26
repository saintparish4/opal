import { helper } from "./lazy-dep.mjs";
console.log("lazy body runs");
export const value = helper();
export function later() {
  return "later";
}
