// Two modules declare the same top-level names; one is renamed in the
// bundle, and every observable name must survive: .name of functions,
// classes, and anonymous functions named by their binding.
import * as one from "./one.mjs";
import * as two from "./two.mjs";
import { helper as aliased } from "./one.mjs";

function check(label, ns) {
  console.log(label, ns.helper.name, ns.Thing.name, ns.arrow.name, ns.anon.name, new ns.Thing().self === ns.Thing);
}
check("one", one);
check("two", two);
console.log(aliased(), two.helper());
const helper = () => "main's own";
console.log(helper(), helper.name);
{
  // A block binding named like a renamed import must not capture it.
  const helper = 1;
  console.log(helper, aliased.name);
}
