export enum Color {
  Red,
  Green = 5,
  Blue,
}
export const enum Flags {
  A = 1,
  B = A << 1,
}
export namespace Geometry {
  export const unit = 1;
  export class Point {
    constructor(public x: number, private y: number) {}
    length() {
      return Math.hypot(this.x, this.y).toFixed(2);
    }
  }
}
function Track(name: string): ClassDecorator {
  return (target) => {
    (globalThis as any).decorated = [...((globalThis as any).decorated ?? []), `${name}:${target.name}`];
  };
}
function Log(_target: object, key: string | symbol, descriptor: PropertyDescriptor) {
  const original = descriptor.value;
  descriptor.value = function (this: unknown, ...args: unknown[]) {
    return `logged(${String(key)}) ` + original.apply(this, args);
  };
}
@Track("service")
export class Service {
  constructor(private readonly dep: string) {}
  @Log
  describe(): string {
    return `service with ${this.dep}`;
  }
}
export class Plain {
  declared!: number;
  initialized = 1;
  uninitialized: string | undefined;
}
