/**
 * Knip: find unused files, exports and dependencies.
 *
 * Matters most for `@ramraiders/forma-sdk`, whose exports are the public theme API: an export nothing
 * re-exports from `index.ts` is either dead code or API that was never wired up.
 */
export default {
  workspaces: {
    '.': {
      entry: ['*.config.{ts,mjs}']
    },
    'packages/sdk': {
      // The package's two public entry points (package.json `exports`).
      entry: ['src/index.ts', 'src/mock/index.ts'],
      project: ['src/**/*.ts']
    },
    'packages/theme-default': {
      entry: ['index.html'],
      project: ['src/**/*.ts']
    }
  },
  // The Rust toolchain, invoked from npm scripts and lint-staged; not an npm dependency.
  ignoreBinaries: ['rustfmt'],
  rules: {
    exports: 'warn',
    types: 'off',
    dependencies: 'error',
    unlisted: 'error',
    binaries: 'error'
  }
};
