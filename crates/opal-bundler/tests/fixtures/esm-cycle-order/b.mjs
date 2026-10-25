import { fromA, late } from "./a.mjs";
export function fromB() {
  return "b";
}
console.log("b body", typeof fromA);
try {
  late;
} catch (error) {
  console.log(error.constructor.name, error.message);
}
