import { defineConfig } from 'vitest/config';

export default defineConfig({
  // Forma serves themes from `forma://localhost/theme/<id>/`: asset URLs must be relative.
  base: './',
  build: { target: 'es2022' },
  test: { environment: 'jsdom' }
});
