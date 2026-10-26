export function used() {
  return "used";
}
export function unusedFunction() {
  return sideEffect();
}
export const unusedConst = { a: 1, f() {} };
export class UnusedClass {
  method() {}
}
// Effects stay even though nothing reads their bindings.
export const effectful = console.log("effectful initializer runs") ?? 1;
console.log("statement runs");
export class WithStaticBlock {
  static {
    console.log("static block runs");
  }
}
// A chain: chain reads helper, so helper stays.
const helper = () => "helper";
export const chain = () => helper();
function sideEffect() {
  console.log("never called");
}
