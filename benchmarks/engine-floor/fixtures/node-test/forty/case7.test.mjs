// The same ten tests as fixtures/one/case1.test.js, written for node:test,
// which has no Jest-style globals or expect.
import { test, describe, beforeEach } from "node:test";
import assert from "node:assert/strict";
let counter = 0;
beforeEach(() => { counter = 0; });
describe("arith", () => {
  test("adds", () => { counter++; assert.equal(1 + 1, 2); });
  test("objects", () => { assert.deepEqual({ a: 1, b: [1, 2, {}] }, { a: 1, b: [1, 2, {}] }); });
  test("throws", () => { assert.throws(() => { throw new Error("x"); }, /x/); });
  test("async", async () => { await Promise.resolve(); assert.equal(counter, 0); });
  test("strings", () => { assert.match("hello world", /wor/); });
  test("arrays", () => { assert.ok([1, 2, 3].includes(2)); assert.equal([1, 2, 3].length, 3); });
  test("truthy", () => { assert.ok(!""); assert.ok(!0); });
  test("numbers", () => { assert.ok(Math.abs(0.1 + 0.2 - 0.3) < 0.005); assert.ok(5 > 4); });
  test("nulls", () => { assert.equal(null, null); assert.equal(undefined, undefined); });
  test("counter", () => { counter += 2; assert.equal(counter, 2); });
});
