import * as node_path from 'node:path';

const value = 1;

var leaf = /*#__PURE__*/Object.freeze({
	__proto__: null,
	value: value
});

const own = 1;

var hub = /*#__PURE__*/Object.freeze({
	__proto__: null,
	ext: node_path,
	imported: leaf,
	leaf: leaf,
	own: own,
	same: leaf
});

console.log(Object.keys(hub).join(","));
console.log(leaf === (await Promise.resolve().then(function () { return leaf; })), typeof leaf, typeof leaf, typeof node_path);
const late = await Promise.resolve().then(function () { return hub; });
console.log(Object.keys(late).join(","));
