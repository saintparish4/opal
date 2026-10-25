function _mergeNamespaces(n, m) {
	m.forEach(function (e) {
		e && typeof e !== 'string' && !Array.isArray(e) && Object.keys(e).forEach(function (k) {
			if (k !== 'default' && !(k in n)) {
				var d = Object.getOwnPropertyDescriptor(e, k);
				Object.defineProperty(n, k, d.get ? d : {
					enumerable: true,
					get: function () { return e[k]; }
				});
			}
		});
	});
	return Object.freeze(n);
}

function getDefaultExportFromCjs (x) {
	return x && x.__esModule && Object.prototype.hasOwnProperty.call(x, 'default') ? x['default'] : x;
}

var main$1 = {};

var hasRequiredMain;

function requireMain () {
	if (hasRequiredMain) return main$1;
	hasRequiredMain = 1;
	// import() from a strict CommonJS module of an ES module and of CommonJS.
	Promise.resolve().then(function () { return esm; }).then((ns) => {
	  console.log("esm:", Object.keys(ns), ns.x);
	  return Promise.resolve().then(function () { return other$1; });
	}).then((ns) => {
	  console.log("cjs:", Object.keys(ns), ns.default.y);
	});
	console.log("sync code first");
	return main$1;
}

var mainExports = requireMain();
var main = /*@__PURE__*/getDefaultExportFromCjs(mainExports);

const x = "esm x"; console.log("esm runs");

var esm = /*#__PURE__*/Object.freeze({
	__proto__: null,
	x: x
});

var other$2 = {};

var hasRequiredOther;

function requireOther () {
	if (hasRequiredOther) return other$2;
	hasRequiredOther = 1;
	other$2.y = "cjs y";
	return other$2;
}

var otherExports = requireOther();
var other = /*@__PURE__*/getDefaultExportFromCjs(otherExports);

var other$1 = /*#__PURE__*/_mergeNamespaces({
	__proto__: null,
	default: other
}, [otherExports]);

export { main as default };
