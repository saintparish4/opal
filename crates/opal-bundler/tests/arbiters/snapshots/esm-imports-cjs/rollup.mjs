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

var lib$1 = {};

var hasRequiredLib;

function requireLib () {
	if (hasRequiredLib) return lib$1;
	hasRequiredLib = 1;
	(function (exports) {
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
	} (lib$1));
	return lib$1;
}

var libExports = requireLib();
var lib = /*@__PURE__*/getDefaultExportFromCjs(libExports);

var ns = /*#__PURE__*/_mergeNamespaces({
	__proto__: null,
	default: lib
}, [libExports]);

var transpiled = {};

var hasRequiredTranspiled;

function requireTranspiled () {
	if (hasRequiredTranspiled) return transpiled;
	hasRequiredTranspiled = 1;
	Object.defineProperty(transpiled, "__esModule", { value: true });
	transpiled.named = "named";
	transpiled.default = "the default property";
	return transpiled;
}

var transpiledExports = requireTranspiled();
var esModuleDefault = /*@__PURE__*/getDefaultExportFromCjs(transpiledExports);

var base = {};

var hasRequiredBase;

function requireBase () {
	if (hasRequiredBase) return base;
	hasRequiredBase = 1;
	base.fromBase = "base";
	base.alsoBase = 2;
	return base;
}

var reexporting$2;
var hasRequiredReexporting;

function requireReexporting () {
	if (hasRequiredReexporting) return reexporting$2;
	hasRequiredReexporting = 1;
	reexporting$2 = {
	  ...requireBase(),
	  own: 1,
	};
	return reexporting$2;
}

var reexportingExports = requireReexporting();
var reexporting = /*@__PURE__*/getDefaultExportFromCjs(reexportingExports);

var reexporting$1 = /*#__PURE__*/_mergeNamespaces({
	__proto__: null,
	default: reexporting
}, [reexportingExports]);

// Rule 7: an ES module importing CommonJS sees what Node's synthetic module
// record holds: default is module.exports, named exports are the lexer's
// names read once after the module ran, and the namespace is fixed.

console.log(Object.keys(ns));
console.log(lib === lib, libExports["module.exports"] === lib);
console.log("snapshot:", libExports.counter);
libExports.bump();
console.log("after bump:", libExports.counter, libExports.counter, lib.counter);
console.log(libExports.fromGetter, libExports.viaDefine, "unrecognizedGetter" in ns, lib.unrecognizedGetter);
console.log("__esModule default is still module.exports:", typeof esModuleDefault, esModuleDefault.default, transpiledExports.named);
console.log("re-exported names:", Object.keys(reexporting$1));
console.log(Object.getOwnPropertyDescriptor(ns, "counter"));
