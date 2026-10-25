// Evaluation order in a cycle (ECMA-262 §16.2.1.6.1.3.1): main -> a -> b -> a.
// b runs first and sees a's function (initialized at link) but not a's let.
import { fromA } from "./a.mjs";
import "./c.mjs";
console.log("main", fromA());
