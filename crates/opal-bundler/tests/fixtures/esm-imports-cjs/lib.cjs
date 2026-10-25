"use strict";
exports.counter = 0;
exports.bump = () => {
  exports.counter++;
};
const values = { fromGetter: "getter read once" };
// The form TypeScript emits for re-exports, which the lexer recognizes.
Object.defineProperty(exports, "fromGetter", { enumerable: true, get: function () { return values.fromGetter; } });
Object.defineProperty(exports, "unrecognizedGetter", {
  enumerable: true,
  get() {
    return "only through default";
  },
});
Object.defineProperty(exports, "viaDefine", { enumerable: true, value: "defined" });
exports.later = undefined;
setTimeout(() => {
  exports.later = "set later";
}, 0);
