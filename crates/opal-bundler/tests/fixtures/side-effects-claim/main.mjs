// Rule 15: a package's "sideEffects": false lets an unused module's
// statements go even when they are not syntactically pure. The honest
// package loses nothing observable; the lying one's log disappears, which
// is the divergence the contract allows when the statement is false.
import { used } from "honest";
import { alsoUsed } from "lying";
console.log(used, alsoUsed);
