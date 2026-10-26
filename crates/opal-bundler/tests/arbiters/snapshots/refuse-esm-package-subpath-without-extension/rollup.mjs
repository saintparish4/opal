function getDefaultExportFromCjs (x) {
	return x && x.__esModule && Object.prototype.hasOwnProperty.call(x, 'default') ? x['default'] : x;
}

var sub;
var hasRequiredSub;

function requireSub () {
	if (hasRequiredSub) return sub;
	hasRequiredSub = 1;
	sub = "pkg/sub.js";
	return sub;
}

var subExports = requireSub();
var fromBare = /*@__PURE__*/getDefaultExportFromCjs(subExports);

console.log(fromBare, fromBare);
