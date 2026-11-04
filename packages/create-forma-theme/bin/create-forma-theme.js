#!/usr/bin/env node
// npm create @ramraiders/forma-theme <folder> [--name "My Theme"] [--author "Me"]
import { relative } from 'node:path';
import { parseArgs } from 'node:util';

import { createTheme } from '../src/create.js';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: {
    name: { type: 'string' },
    id: { type: 'string' },
    author: { type: 'string' },
    help: { type: 'boolean', short: 'h' }
  }
});

if (values.help || positionals.length !== 1) {
  console.log(
    'Usage: npm create @ramraiders/forma-theme <folder> [--name "My Theme"] [--author "Me"]'
  );
  process.exit(values.help ? 0 : 1);
}

try {
  const theme = createTheme(positionals[0], values);
  const near = relative(process.cwd(), theme.dir) || '.';
  const where = near.startsWith('..') ? theme.dir : near;
  console.log(`Created ${theme.name} (${theme.id}) in ${where}. Next:

  cd ${where}
  npm login --scope=@ramraiders --registry=https://npm.pkg.github.com   # once
  npm install
  npm run dev`);
} catch (error) {
  console.error(`create-forma-theme: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}
