// Creates a Forma theme from ../template: copies it, fills in the theme's id, name, author and
// the SDK version, and renames the dotfiles npm would otherwise leave out of the package.
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const TEMPLATE = fileURLToPath(new URL('../template', import.meta.url));

/** The SDK version new themes start on: kept in step with packages/sdk by a test. */
export const SDK_VERSION = '1.36.0';

/** npm drops dotfiles from a published package, so the template spells them with `_`. */
const RENAMED = { _gitignore: '.gitignore', _npmrc: '.npmrc' };

/** `My Theme!` → `my-theme`. */
export function toId(name) {
  return name
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');
}

/** `my-theme` → `My Theme`. */
function toName(id) {
  return id
    .split('-')
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(' ');
}

const json = (text) => JSON.stringify(text).slice(1, -1);

/**
 * Writes a new theme into `dir`, which must not exist or be empty. Returns what it used.
 * @param {string} dir
 * @param {{ name?: string, id?: string, author?: string }} [options]
 */
export function createTheme(dir, options = {}) {
  const target = resolve(dir);
  if (existsSync(target) && readdirSync(target).length > 0) {
    throw new Error(`${target} is not empty`);
  }
  const id = toId(options.id ?? options.name ?? basename(target));
  if (!id) throw new Error('the theme needs a name with letters or digits in it');
  const name = options.name?.trim() || toName(id);
  const author = options.author?.trim() || 'You';
  const values = {
    __ID__: id,
    __NAME__: name,
    __AUTHOR__: author,
    __SDK__: SDK_VERSION,
    __SDK_MAJOR__: SDK_VERSION.split('.')[0]
  };

  const copy = (from, to) => {
    mkdirSync(to, { recursive: true });
    for (const entry of readdirSync(from)) {
      if (entry === 'node_modules' || entry === 'dist') continue;
      const source = join(from, entry);
      const dest = join(to, RENAMED[entry] ?? entry);
      if (statSync(source).isDirectory()) {
        copy(source, dest);
        continue;
      }
      let text = readFileSync(source, 'utf8');
      // In JSON files the values go inside strings: escape them for JSON.
      const escape = entry.endsWith('.json') ? json : (v) => v;
      for (const [key, value] of Object.entries(values)) {
        text = text.replaceAll(key, escape(value));
      }
      writeFileSync(dest, text);
    }
  };
  copy(TEMPLATE, target);
  return { dir: target, id, name, author };
}
