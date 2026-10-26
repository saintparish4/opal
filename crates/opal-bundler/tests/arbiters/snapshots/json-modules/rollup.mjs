import { createRequire } from 'node:module';

var a = 1;
var __proto__ = {
	polluted: true
};
var list = [
	true,
	null
];
var data = {
	a: a,
	__proto__: __proto__,
	list: list
};

var ns = /*#__PURE__*/Object.freeze({
	__proto__: null,
	["__proto__"]: __proto__,
	a: a,
	default: data,
	list: list
});

// Rule 8: JSON through import and require shares one object, and
// "__proto__" is an own property because JSON.parse reads the text.
const require$1 = createRequire(import.meta.url);
console.log(Object.keys(data), Object.getPrototypeOf(data) === Object.prototype);
console.log(Object.keys(ns), data === data);
console.log("shared with require:", require$1("./data.json") === data);
