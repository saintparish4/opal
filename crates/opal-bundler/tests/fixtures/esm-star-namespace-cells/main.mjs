import * as hub from "./hub.mjs";
console.log(Object.keys(hub).join(","));
console.log(hub.same === (await import("./leaf.mjs")), typeof hub.leaf, typeof hub.imported, typeof hub.ext);
const late = await import("./hub.mjs");
console.log(Object.keys(late).join(","));
