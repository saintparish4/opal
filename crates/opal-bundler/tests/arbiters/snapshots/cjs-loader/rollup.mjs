import require$$0 from 'node:path';
import require$$6 from 'node:os';
import require$$7 from 'path';

function getDefaultExportFromCjs (x) {
	return x && x.__esModule && Object.prototype.hasOwnProperty.call(x, 'default') ? x['default'] : x;
}

function commonjsRequire(path) {
	throw new Error('Could not dynamically require "' + path + '". Please configure the dynamicRequireTargets or/and ignoreDynamicRequires option of @rollup/plugin-commonjs appropriately for this require call to work.');
}

var main$2 = {exports: {}};

var a = {exports: {}};

var b = {exports: {}};

var hasRequiredB;

function requireB () {
	if (hasRequiredB) return b.exports;
	hasRequiredB = 1;
	(function (module, exports) {
		const a = requireA();
		exports.sawName = a.name + ":" + Object.keys(a).join(",");
		exports.loadedAtEnd = false;
		process.nextTick(() => {
		  exports.loadedAtEnd = module.loaded;
		}); 
	} (b, b.exports));
	return b.exports;
}

var hasRequiredA;

function requireA () {
	if (hasRequiredA) return a.exports;
	hasRequiredA = 1;
	(function (module, exports) {
		exports.name = "a";
		// A cycle: b requires a while a is loading and sees a's partial exports.
		const b = requireB();
		exports.fromB = b.sawName;
		exports.parentId = module.parent.id; 
	} (a, a.exports));
	return a.exports;
}

var flaky;
var hasRequiredFlaky;

function requireFlaky () {
	if (hasRequiredFlaky) return flaky;
	hasRequiredFlaky = 1;
	globalThis.flakyRuns = (globalThis.flakyRuns || 0) + 1;
	if (globalThis.flakyRuns === 1) {
	  throw new Error("first run fails");
	}
	flaky = "ran " + globalThis.flakyRuns + " times";
	return flaky;
}

var list = [
	1,
	2,
	3
];
var __proto__ = {
	own: true
};
var require$$5 = {
	list: list,
	__proto__: __proto__
};

var main$1 = main$2.exports;

var hasRequiredMain;

function requireMain () {
	if (hasRequiredMain) return main$2.exports;
	hasRequiredMain = 1;
	(function (module) {
		// SEMANTICS.md rule 6: the CommonJS wrapper, the module object, the cache,
		// and require() of bundled, builtin, and missing modules.
		const path = require$$0;
		const here = (p) => path.relative(process.cwd(), p);

		console.log("main:", require.main === module, module.id, here(__filename), here(__dirname));
		console.log("this is exports:", main$1 === module.exports, typeof require.resolve, arguments.length);

		const a = requireA();
		console.log("a:", a.name, a.fromB, "b loaded:", requireB().loadedAtEnd);
		console.log("same instance:", requireA() === a);
		try {
		  // require() adds .js, .json, and .node, never .cjs.
		  require("./a");
		} catch (error) {
		  console.log("no .cjs probing:", error.code);
		}
		console.log("children:", module.children.map((m) => here(m.filename)));
		console.log("a.parent is main:", a.parentId);

		// A module that throws is dropped from the cache, so it runs again.
		for (let attempt = 0; attempt < 2; attempt++) {
		  try {
		    console.log("flaky:", requireFlaky());
		  } catch (error) {
		    console.log("flaky threw:", error.message);
		  }
		}

		// JSON through require: parsed once, the same object every time.
		const data = require$$5;
		console.log("json:", data.list.length, require$$5 === data, Object.keys(data));

		// require() of a name only known at run time still finds the bundled module.
		const which = "./a" + ".cjs";
		console.log("dynamic require:", commonjsRequire(which) === a);

		// A builtin through require.
		console.log("builtin:", typeof require$$6.platform, require$$7 === path);

		// A missing module throws Node's error.
		try {
		  require("./does-not-exist");
		} catch (error) {
		  console.log("missing:", error.code);
		}
		console.log("resolve:", here(require.resolve("./a.cjs")));
		console.log("module keys:", Object.keys(module));
		console.log("loaded during run:", module.loaded);
		setTimeout(() => console.log("loaded after:", module.loaded), 0);
		module.exports = { done: true }; 
	} (main$2));
	return main$2.exports;
}

var mainExports = requireMain();
var main = /*@__PURE__*/getDefaultExportFromCjs(mainExports);

export { main as default };
