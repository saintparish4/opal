// Rule 12: --external keeps a package out; its ES module import is
// hoisted (which this program does not observe), and a require() of one
// is Node's own, from the original module's location.
import { value } from "ext-esm";
import { fromCjs } from "./bridge.cjs";
console.log(value, fromCjs);
