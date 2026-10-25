var __defProp = Object.defineProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};

// one.mjs
var one_exports = {};
__export(one_exports, {
  Thing: () => Thing,
  anon: () => anon,
  arrow: () => arrow,
  helper: () => helper
});
function helper() {
  return "one";
}
var Thing = class _Thing {
  constructor() {
    this.self = _Thing;
  }
};
var arrow = () => 1;
var anon;
anon = function() {
};

// two.mjs
var two_exports = {};
__export(two_exports, {
  Thing: () => Thing2,
  anon: () => anon2,
  arrow: () => arrow2,
  helper: () => helper2
});
function helper2() {
  return "two";
}
var Thing2 = class _Thing {
  static tag = _Thing.name;
  constructor() {
    this.self = _Thing;
  }
};
var arrow2 = () => 2;
var anon2;
anon2 = class {
};
console.log("two static", Thing2.tag);

// main.mjs
function check(label, ns) {
  console.log(label, ns.helper.name, ns.Thing.name, ns.arrow.name, ns.anon.name, new ns.Thing().self === ns.Thing);
}
check("one", one_exports);
check("two", two_exports);
console.log(helper(), helper2());
var helper3 = () => "main's own";
console.log(helper3(), helper3.name);
{
  const helper4 = 1;
  console.log(helper4, helper.name);
}
