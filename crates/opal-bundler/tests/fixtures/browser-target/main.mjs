// --target browser: the browser condition picks files, and no Node API is
// used. Its oracle is the same program under Node with the browser
// condition, which these packages' exports reach without a node key.
import { where } from "dual";
console.log(where, typeof globalThis.window);
