// Rule 11: while the entry's top-level await is pending, the entry is still
// evaluating, so importing it waits for it to finish, which here is never.
// Node deadlocks and exits 13 with its "unsettled top-level await" warning,
// and so must the bundle.
console.log("before");
const self = await import("./main.mjs");
console.log("never", Object.keys(self));
export const x = 1;
