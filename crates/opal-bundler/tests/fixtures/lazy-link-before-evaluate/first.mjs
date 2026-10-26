import * as entry from "./entry.mjs";
console.log("first runs; hoisted is", typeof entry.hoisted, entry.hoisted());
try {
  entry.late;
} catch (error) {
  console.log("late is in its TDZ:", error.constructor.name);
}
export const first = 1;
