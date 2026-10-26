"use strict";
globalThis.flakyRuns = (globalThis.flakyRuns || 0) + 1;
if (globalThis.flakyRuns === 1) {
  throw new Error("first run fails");
}
module.exports = "ran " + globalThis.flakyRuns + " times";
