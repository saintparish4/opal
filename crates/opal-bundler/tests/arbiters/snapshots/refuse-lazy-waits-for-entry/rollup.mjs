const value = "lazy sees " + fromEntry;

var lazy$1 = /*#__PURE__*/Object.freeze({
	__proto__: null,
	value: value
});

const lazy = await Promise.resolve().then(function () { return lazy$1; });
console.log(lazy.value);
const fromEntry = "entry";

export { fromEntry };
