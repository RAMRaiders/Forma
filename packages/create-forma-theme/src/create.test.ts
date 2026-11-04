import { mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

import sdk from '../../sdk/package.json' with { type: 'json' };
import { SDK_VERSION, createTheme, toId } from './create.js';

const fresh = () => join(mkdtempSync(join(tmpdir(), 'cft-')), 'out');

describe('create-forma-theme', () => {
  it('starts themes on the SDK in this repo', () => {
    expect(SDK_VERSION).toBe(sdk.version);
  });

  it('writes a theme with its id, name and author, and real dotfiles', () => {
    const dir = fresh();
    const theme = createTheme(dir, { name: 'Café "Neon"', author: 'Ana' });
    expect(theme.id).toBe('cafe-neon');
    const manifest = JSON.parse(readFileSync(join(dir, 'public/theme.json'), 'utf8')) as unknown;
    expect(manifest).toMatchObject({
      id: 'cafe-neon',
      name: 'Café "Neon"',
      author: 'Ana',
      sdk: '^1',
      entry: 'index.html'
    });
    const pkg = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')) as {
      name: string;
      dependencies: Record<string, string>;
    };
    expect(pkg.name).toBe('cafe-neon');
    expect(pkg.dependencies['@ramraiders/forma-sdk']).toBe(`^${sdk.version}`);
    expect(readdirSync(dir).sort()).toEqual(
      [
        '.gitignore',
        '.npmrc',
        'README.md',
        'index.html',
        'package.json',
        'public',
        'src',
        'tsconfig.json',
        'vite.config.ts'
      ].sort()
    );
    const all = ['README.md', 'index.html', 'src/app.ts', 'src/main.ts'].map((f) =>
      readFileSync(join(dir, f), 'utf8')
    );
    expect(all.join('\n')).not.toMatch(/__[A-Z_]+__/);
  });

  it('names the theme after its folder, and refuses a folder in use', () => {
    const dir = fresh();
    expect(createTheme(dir.replace(/out$/, 'My Arcade')).id).toBe('my-arcade');
    createTheme(dir);
    writeFileSync(join(dir, 'x'), '');
    expect(() => createTheme(dir)).toThrow('not empty');
    expect(toId('***')).toBe('');
  });
});
