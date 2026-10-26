// TypeScript Node 24 can run itself by stripping types: the original runs
// as written (SEMANTICS.md rule 13).
import { type Shape, area, describe } from "./shapes.ts";
import type { Unused } from "./types-only.ts";

const shapes: Shape[] = [
  { kind: "circle", radius: 1 },
  { kind: "square", side: 2 },
];
const total = shapes.reduce((sum, shape) => sum + area(shape), 0) satisfies number;
console.log(total.toFixed(3), shapes.map(describe));
let maybe: Unused | undefined;
console.log(maybe?.x ?? "no unused");
