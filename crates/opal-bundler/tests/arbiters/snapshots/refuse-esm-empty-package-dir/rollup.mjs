function getDefaultExportFromCjs (x) {
	return x && x.__esModule && Object.prototype.hasOwnProperty.call(x, 'default') ? x['default'] : x;
}

var pkg;
var hasRequiredPkg;

function requirePkg () {
	if (hasRequiredPkg) return pkg;
	hasRequiredPkg = 1;
	pkg = "outer pkg";
	return pkg;
}

var pkgExports = requirePkg();
var value = /*@__PURE__*/getDefaultExportFromCjs(pkgExports);

console.log(value);
