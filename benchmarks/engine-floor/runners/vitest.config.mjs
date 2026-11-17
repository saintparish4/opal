// globals: the fixtures use Jest's global describe/test/expect, which vitest
// only provides when asked.
export default { test: { globals: true, include: ["fixtures/one/**/*.test.js", "fixtures/forty/**/*.test.js"] } };
