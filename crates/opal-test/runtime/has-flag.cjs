// Copied from has-flag 4.0.0 (has-flag/index.js) by
// crates/opal-test/port/port-chalk.mjs, with its requires pointed at the
// other ported files. Do not edit by hand; edit the port.
//
// MIT license, in compat/test-runners/LICENSES/has-flag.txt.
'use strict';

module.exports = (flag, argv = process.argv) => {
	const prefix = flag.startsWith('-') ? '' : (flag.length === 1 ? '-' : '--');
	const position = argv.indexOf(prefix + flag);
	const terminatorPosition = argv.indexOf('--');
	return position !== -1 && (terminatorPosition === -1 || position < terminatorPosition);
};
