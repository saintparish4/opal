// Rule 11: the entry may await at the top level.
import { slow } from "./dep.mjs";
console.log("before await");
const value = await slow();
console.log("after await", value);
for await (const item of [Promise.resolve(1), 2]) {
  console.log("item", item);
}
