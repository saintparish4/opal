function helper$2() {
  return "one";
}
let Thing$1 = class Thing {
  constructor() {
    this.self = Thing;
  }
};
const arrow$1 = () => 1;
let anon$1;
anon$1 = function () {};

var one = /*#__PURE__*/Object.freeze({
  __proto__: null,
  Thing: Thing$1,
  get anon () { return anon$1; },
  arrow: arrow$1,
  helper: helper$2
});

function helper$1() {
  return "two";
}
class Thing {
  static tag = Thing.name;
  constructor() {
    this.self = Thing;
  }
}
const arrow = () => 2;
let anon;
anon = class {};
console.log("two static", Thing.tag);

var two = /*#__PURE__*/Object.freeze({
  __proto__: null,
  Thing: Thing,
  get anon () { return anon; },
  arrow: arrow,
  helper: helper$1
});

// Two modules declare the same top-level names; one is renamed in the
// bundle, and every observable name must survive: .name of functions,
// classes, and anonymous functions named by their binding.

function check(label, ns) {
  console.log(label, ns.helper.name, ns.Thing.name, ns.arrow.name, ns.anon.name, new ns.Thing().self === ns.Thing);
}
check("one", one);
check("two", two);
console.log(helper$2(), helper$1());
const helper = () => "main's own";
console.log(helper(), helper.name);
{
  // A block binding named like a renamed import must not capture it.
  const helper = 1;
  console.log(helper, helper$2.name);
}
