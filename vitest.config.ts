import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

/** Workspace packages resolve to SOURCE, so tests run on a clean checkout without a build. */
const sdkSrc = fileURLToPath(new URL('./packages/sdk/src/index.ts', import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      '@forma/sdk': sdkSrc
    }
  },
  test: {
    include: ['packages/*/src/**/*.test.ts'],
    exclude: ['**/node_modules/**', '**/dist/**'],
    environment: 'node',
    globals: false,
    testTimeout: 10_000,
    hookTimeout: 10_000,
    coverage: {
      provider: 'v8',
      reporter: ['text', 'html', 'lcov'],
      reportsDirectory: './coverage',
      include: ['packages/sdk/src/**/*.ts', 'packages/theme-default/src/logic/**/*.ts'],
      exclude: ['**/*.test.ts', '**/*.d.ts', '**/index.ts'],
      // Thresholds are a ratchet: raise them as coverage grows, never lower them.
      thresholds: {
        lines: 85,
        functions: 85,
        branches: 80,
        statements: 85
      }
    }
  }
});
