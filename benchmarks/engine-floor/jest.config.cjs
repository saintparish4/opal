// The reference Jest's config for the engine-floor timings. It sits here, not
// in runners/, because jest 30.5 fails to find a config passed with --config
// from outside the working directory ("Could not find a config file").
// roots keeps jest's file crawl off target/.
module.exports = {
  roots: ["<rootDir>/fixtures/one", "<rootDir>/fixtures/forty"],
  testEnvironment: "node",
  transform: {},
};
