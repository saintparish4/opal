var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __decorateClass = (decorators, target, key, kind) => {
  var result = kind > 1 ? void 0 : kind ? __getOwnPropDesc(target, key) : target;
  for (var i = decorators.length - 1, decorator; i >= 0; i--)
    if (decorator = decorators[i])
      result = (kind ? decorator(target, key, result) : decorator(result)) || result;
  if (kind && result) __defProp(target, key, result);
  return result;
};

// node_modules/reflect-metadata-lite/index.js
var store = /* @__PURE__ */ new WeakMap();
var keyed = (target, key) => {
  let byKey = store.get(target);
  if (!byKey) store.set(target, byKey = /* @__PURE__ */ new Map());
  let entries = byKey.get(key);
  if (!entries) byKey.set(key, entries = /* @__PURE__ */ new Map());
  return entries;
};
Reflect.metadata = (metadataKey, value) => (target, key) => {
  keyed(target, key).set(metadataKey, value);
};
Reflect.getMetadata = (metadataKey, target, key) => keyed(target, key).get(metadataKey);

// lib.ts
var Color = /* @__PURE__ */ ((Color2) => {
  Color2[Color2["Red"] = 0] = "Red";
  Color2[Color2["Green"] = 5] = "Green";
  Color2[Color2["Blue"] = 6] = "Blue";
  return Color2;
})(Color || {});
var Geometry;
((Geometry2) => {
  Geometry2.unit = 1;
  class Point {
    constructor(x, y) {
      this.x = x;
      this.y = y;
    }
    length() {
      return Math.hypot(this.x, this.y).toFixed(2);
    }
  }
  Geometry2.Point = Point;
})(Geometry || (Geometry = {}));
function Track(name) {
  return (target) => {
    globalThis.decorated = [...globalThis.decorated ?? [], `${name}:${target.name}`];
  };
}
function Log(_target, key, descriptor) {
  const original = descriptor.value;
  descriptor.value = function(...args) {
    return `logged(${String(key)}) ` + original.apply(this, args);
  };
}
var Service = class {
  constructor(dep) {
    this.dep = dep;
  }
  describe() {
    return `service with ${this.dep}`;
  }
};
__decorateClass([
  Log
], Service.prototype, "describe", 1);
Service = __decorateClass([
  Track("service")
], Service);
var Plain = class {
  constructor() {
    this.initialized = 1;
  }
};

// main.ts
console.log(0 /* Red */, Color[5 /* Green */], 2 /* B */, Geometry.unit, new Geometry.Point(1, 2).length());
var service = new Service("dep");
console.log(service.describe(), globalThis.decorated);
console.log(Reflect.getMetadata("design:paramtypes", Service)?.map((t) => t.name));
console.log(Reflect.getMetadata("design:returntype", Service.prototype, "describe")?.name);
console.log(Object.keys(new Plain()), JSON.stringify(new Plain()));
var typed = { n: 1 };
console.log(typed.n);
