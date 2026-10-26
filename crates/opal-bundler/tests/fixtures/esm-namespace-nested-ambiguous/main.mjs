import * as top from "./top.mjs";
import * as mid from "./mid.mjs";
console.log(Object.keys(top).join(","), "|", Object.keys(mid).join(","), top.log);
const late = await import("./top.mjs");
console.log(Object.keys(late).join(","));
