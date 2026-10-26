import { counter, increment, reset } from "./counter.mjs";
import * as ns from "./counter.mjs";
import def from "./default.mjs";
import namedDefault, { other } from "./default-fn.mjs";

console.log(counter, ns.counter);
increment();
increment();
console.log(counter, ns.counter);
reset();
console.log(counter);
console.log(Object.keys(ns), Reflect.ownKeys(ns).length, String(ns[Symbol.toStringTag]));
console.log(Object.getOwnPropertyDescriptor(ns, "counter"));
console.log(Object.isExtensible(ns), Object.getPrototypeOf(ns), "counter" in ns, "nope" in ns);
console.log(def.name, def(), namedDefault.name, other);
try {
  ns.counter = 5;
} catch (error) {
  console.log("assign to namespace:", error.constructor.name);
}
try {
  counter = 1;
} catch (error) {
  console.log("assign to import:", error.constructor.name, error.message);
}
console.log(Reflect.defineProperty(ns, "counter", { value: counter }), Reflect.defineProperty(ns, "counter", { value: 99 }));
console.log(delete ns.nope, Reflect.deleteProperty(ns, "counter"));
