import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

/**
 * One classic script, `dist/shell.js`, with its CSS inside: Forma puts it first in every theme
 * page's `<head>`, so it must run before (and without waiting for) the theme's modules.
 */
export default defineConfig({
  resolve: {
    alias: {
      '@ramraiders/forma-sdk': fileURLToPath(new URL('../sdk/src/index.ts', import.meta.url))
    }
  },
  build: {
    target: 'es2022',
    emptyOutDir: true,
    lib: {
      entry: fileURLToPath(new URL('src/main.ts', import.meta.url)),
      name: 'FormaShell',
      formats: ['iife'],
      fileName: () => 'shell.js'
    }
  }
});
