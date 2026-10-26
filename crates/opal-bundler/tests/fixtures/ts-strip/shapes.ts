export type Shape = { kind: "circle"; radius: number } | { kind: "square"; side: number };
export function area(shape: Shape): number {
  return shape.kind === "circle" ? Math.PI * shape.radius ** 2 : shape.side ** 2;
}
export const describe = <T extends Shape>(shape: T): string => `${shape.kind}`;
