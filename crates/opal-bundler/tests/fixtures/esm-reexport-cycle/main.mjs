import { foo } from "./a.mjs";
import * as a from "./a.mjs";
import { foo as foo2 } from "./a2.mjs";
import * as b2 from "./b2.mjs";
console.log(foo, Object.keys(a).join(","), foo2, Object.keys(b2).join(","));
