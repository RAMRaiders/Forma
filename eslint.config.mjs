import js from '@eslint/js';
import globals from 'globals';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';

export default tseslint.config(
  {
    ignores: ['**/node_modules/**', '**/dist/**', '**/coverage/**', 'target/**', 'crates/**']
  },

  js.configs.recommended,
  ...tseslint.configs.recommendedTypeChecked,

  {
    files: ['**/*.ts'],
    languageOptions: {
      ecmaVersion: 2023,
      sourceType: 'module',
      globals: { ...globals.node },
      parserOptions: {
        // Type-aware linting, against a tsconfig that also covers tests and config files.
        projectService: false,
        project: ['./tsconfig.eslint.json'],
        tsconfigRootDir: import.meta.dirname
      }
    },
    rules: {
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }
      ],
      '@typescript-eslint/consistent-type-imports': 'warn',

      // Async correctness: a theme that drops a promise silently loses an input or a launch.
      '@typescript-eslint/no-floating-promises': 'error',
      '@typescript-eslint/no-misused-promises': 'error',
      '@typescript-eslint/await-thenable': 'error',
      '@typescript-eslint/require-await': 'error',

      eqeqeq: ['error', 'always'],
      'no-console': 'off'
    }
  },

  /* The SDK and themes run in the WebView, not in Node. */
  {
    files: [
      'packages/sdk/src/**/*.ts',
      'packages/theme-*/src/**/*.ts',
      'packages/shell/src/**/*.ts'
    ],
    languageOptions: { globals: { ...globals.browser } }
  },

  /* Test files: relax rules that fight test style. */
  {
    files: ['**/*.test.ts'],
    rules: {
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/unbound-method': 'off',
      '@typescript-eslint/require-await': 'off',
      // Fakes of Tauri's IPC reject with plain objects, because that is what the IPC does.
      '@typescript-eslint/prefer-promise-reject-errors': 'off'
    }
  },

  /* Plain ESM helper scripts: node globals, no type information needed. */
  {
    files: ['**/*.mjs'],
    extends: [tseslint.configs.disableTypeChecked],
    languageOptions: {
      ecmaVersion: 2023,
      sourceType: 'module',
      globals: { ...globals.node }
    }
  },

  /* Must be last: turns off rules Prettier owns, so the two never disagree. */
  prettier
);
