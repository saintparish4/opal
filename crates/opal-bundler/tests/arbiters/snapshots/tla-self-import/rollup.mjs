// Rule 11: while the entry's top-level await is pending, the entry is still
// evaluating, so importing it waits for it to finish, which here is never.
// Node deadlocks and exits 13 with its "unsettled top-level await" warning,
// and so must the bundle.
console.log("before");
const self = await Promise.resolve().then(function () { return main; });
console.log("never", Object.keys(self));
const x = 1;

var main = /*#__PURE__*/Object.freeze({
	__proto__: null,
	x: x
});

export { x };
