import type { Config } from "jest";

/**
 * The Docker-localnet suite, kept out of `npm test` — and so out of CI, which
 * has no Docker — by living in its own directory rather than in `__tests__`,
 * which `jest.config.ts` roots on. Run it with `npm run test:localnet` once
 * `stellar/integration-tests/scripts/start-localnet.sh` has the network up.
 *
 * The transform and mapper are spelled out again rather than spread from
 * `jest.config.ts`: Jest loads a TypeScript config as ESM, where `./jest.config`
 * resolves to a `.js` file that does not exist.
 *
 * One worker, like the Rust harness's `--test-threads=1`: every deploy is
 * sourced from a single admin account, and parallel files would race on its
 * sequence number.
 */
const config: Config = {
  preset: "ts-jest",
  testEnvironment: "node",
  extensionsToTreatAsEsm: [".ts"],
  maxWorkers: 1,
  roots: ["<rootDir>/integration"],
  testMatch: ["**/*.test.ts"],
  // A single test can deploy four contracts and wait out a rate-limit window.
  testTimeout: 300_000,
  transform: {
    "^.+\\.ts$": ["ts-jest", { useESM: true }],
  },
  moduleNameMapper: {
    "^(\\.{1,2}/.*)\\.js$": "$1",
  },
};

export default config;
