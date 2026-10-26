function getDefaultExportFromCjs (x) {
	return x && x.__esModule && Object.prototype.hasOwnProperty.call(x, 'default') ? x['default'] : x;
}

var main$1 = {};

var real;
var hasRequiredReal;

function requireReal () {
	if (hasRequiredReal) return real;
	hasRequiredReal = 1;
	real = "real";
	return real;
}

var hasRequiredMain;

function requireMain () {
	if (hasRequiredMain) return main$1;
	hasRequiredMain = 1;
	try { console.log(requireReal()); } catch (error) { console.log(error.code); }
	return main$1;
}

var mainExports = requireMain();
var main = /*@__PURE__*/getDefaultExportFromCjs(mainExports);

export { main as default };
