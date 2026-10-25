// Rule 10: lazy modules keep rules 2-4: a cycle among them runs in
// post-order, sees hoisted functions early, and throws in the TDZ.
const a = await import("./a.mjs");
console.log("a:", a.fromA(), Object.keys(a));
const b = await import("./b.mjs");
console.log("b:", b.fromB(), b.late);
