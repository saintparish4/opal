import { shared as fromMain } from "./main.mjs";
let shared = "a's";
export function readEarly() {
  return shared + " " + fromMain;
}
try {
  fromMain;
} catch (error) {
  console.log(error.constructor.name, error.message);
}
