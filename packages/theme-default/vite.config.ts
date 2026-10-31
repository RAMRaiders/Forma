import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

export default defineConfig({
  resolve: {
    alias: {
      // Develop against the SDK source, so a theme change never waits on an SDK build.
      '@ramraiders/forma-sdk': fileURLToPath(new URL('../sdk/src/index.ts', import.meta.url))
    }
  },
  // Themes are served from `forma://theme/<id>/`, so asset URLs must be relative.
  base: './',
  server: {
    port: 5173,
    strictPort: true
  },
  build: {
    target: 'es2022'
  }
});
