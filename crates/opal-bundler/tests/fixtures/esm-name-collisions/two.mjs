export function helper() {
  return "two";
}
export class Thing {
  static tag = Thing.name;
  constructor() {
    this.self = Thing;
  }
}
export const arrow = () => 2;
export let anon;
anon = class {};
console.log("two static", Thing.tag);
