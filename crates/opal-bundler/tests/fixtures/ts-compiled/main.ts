// TypeScript Node cannot strip: enums, namespaces, parameter properties,
// legacy decorators with metadata, and import elision. The reference is
// what tsc emits (SEMANTICS.md rule 13).
import "reflect-metadata-lite";
import { Color, Flags, Geometry, Service, Plain } from "./lib.js";
import { OnlyAType } from "./types.js";

console.log(Color.Red, Color[Color.Green], Flags.B, Geometry.unit, new Geometry.Point(1, 2).length());
const service = new Service("dep");
console.log(service.describe(), (globalThis as any).decorated);
console.log((Reflect as any).getMetadata("design:paramtypes", Service)?.map((t: Function) => t.name));
console.log((Reflect as any).getMetadata("design:returntype", Service.prototype, "describe")?.name);
console.log(Object.keys(new Plain()), JSON.stringify(new Plain()));
let typed: OnlyAType = { n: 1 };
console.log(typed.n);
