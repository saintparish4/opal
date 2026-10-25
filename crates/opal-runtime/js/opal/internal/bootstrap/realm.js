'use strict';

// Opal's internal/bootstrap/realm, for the part Node's library files use:
// BuiltinModule's questions about which modules exist and which a user may
// require. The loader itself is js/bootstrap/realm.js and loader.js.

const { SafeSet, StringPrototypeStartsWith } = primordials;

// What `require()` and `import` accept, with or without `node:`.
const canBeRequiredByUsers = new SafeSet([
  'buffer',
  'console',
  'events',
  'module',
  'path',
  'path/posix',
  'path/win32',
  'process',
  'timers',
  'util',
  'util/types',
]);

// Accepted only with the `node:` scheme.
const schemeOnly = new SafeSet(['node:sea', 'node:sqlite', 'node:test', 'node:test/reporters']);

const BuiltinModule = {
  exists(id) {
    return canBeRequiredByUsers.has(id) || StringPrototypeStartsWith(id, 'internal/');
  },
  canBeRequiredByUsers(id) {
    return canBeRequiredByUsers.has(id);
  },
  canBeRequiredWithoutScheme(id) {
    return canBeRequiredByUsers.has(id);
  },
  normalizeRequirableId(id) {
    const name = StringPrototypeStartsWith(id, 'node:') ? id.slice(5) : id;
    if (canBeRequiredByUsers.has(name) || schemeOnly.has(id)) return name;
    return undefined;
  },
  getSchemeOnlyModuleNames() {
    return [...schemeOnly].map((id) => id.slice(5));
  },
  getAllBuiltinModuleIds() {
    return [...canBeRequiredByUsers];
  },
};

module.exports = { BuiltinModule };
