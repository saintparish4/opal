let counter = 0;
beforeEach(() => { counter = 0; });
describe('arith', () => {
  test('adds', () => { counter++; expect(1 + 1).toBe(2); });
  test('objects', () => { expect({ a: 1, b: [1, 2, { c: undefined }] }).toEqual({ a: 1, b: [1, 2, {}] }); });
  test('throws', () => { expect(() => { throw new Error('x'); }).toThrow('x'); });
  test('async', async () => { await Promise.resolve(); expect(counter).toBe(0); });
  test('strings', () => { expect('hello world').toMatch(/wor/); });
  test('arrays', () => { expect([1, 2, 3]).toContain(2); expect([1, 2, 3]).toHaveLength(3); });
  test('truthy', () => { expect('').toBeFalsy(); expect(0).not.toBeTruthy(); });
  test('numbers', () => { expect(0.1 + 0.2).toBeCloseTo(0.3); expect(5).toBeGreaterThan(4); });
  test('nulls', () => { expect(null).toBeNull(); expect(undefined).toBeUndefined(); });
  test('counter', () => { counter += 2; expect(counter).toBe(2); });
});
