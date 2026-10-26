var lib = {};

var hasRequiredLib;

function requireLib () {
	if (hasRequiredLib) return lib;
	hasRequiredLib = 1;
	(function (exports) {
		Object.defineProperty(exports, "hidden", { enumerable: true, get() { return 1; } }); 
	} (lib));
	return lib;
}

var libExports = requireLib();

console.log(libExports.hidden);
