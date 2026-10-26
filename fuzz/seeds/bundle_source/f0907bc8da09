// Two modules declare `let shared`; one is renamed in the bundle. A cycle
// reads the other module's binding before it is initialized.
import { readEarly } from "./a.mjs";
export let shared = "main's";
console.log(readEarly());
