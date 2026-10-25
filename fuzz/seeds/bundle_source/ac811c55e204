import { fromA, value } from "./a.mjs";
console.log("b body runs first", typeof fromA);
try {
  console.log(value);
} catch (error) {
  console.log("TDZ:", error.constructor.name, error.message);
}
export function fromB() {
  return "b";
}
export const late = "late";
