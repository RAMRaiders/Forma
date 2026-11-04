import type { CatalogueTheme, Forma, ThemeInfo } from '@ramraiders/forma-sdk';
import type { Page, Row } from './page.js';

/** How long a first press of Remove waits for the second. */
const CONFIRM_MS = 3000;

const nameOf = (t: ThemeInfo) => t.name ?? t.id;

const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/**
 * One theme: who made it, what it may do, and using or removing it. Removing asks for a
 * second press, then goes back to the list.
 */
export function themeScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void,
  id: string
): () => Promise<Row[]> {
  let confirmUntil = 0;
  return async () => {
    const theme = (await forma.themes.list()).find((t) => t.id === id);
    if (!theme) return [{ label: 'This theme is no longer installed' }];

    const rows: Row[] = [
      {
        label: 'Version',
        detail: `${theme.version || 'Not given'} · ${
          theme.origin === 'bundled' ? 'comes with Forma' : 'installed'
        }`
      }
    ];
    if (theme.author) rows.push({ label: 'Made by', detail: theme.author });
    if (theme.description) rows.push({ label: theme.description });
    if (theme.unavailable) rows.push({ label: 'Cannot load', detail: theme.unavailable });

    const allowed = theme.permissionDescriptions;
    rows.push(
      ...(allowed.length > 0
        ? allowed.map((what, i) => ({
            heading: i === 0 ? 'It may' : undefined,
            label: sentence(what)
          }))
        : [{ heading: 'It may', label: 'Only show your library and play games' }])
    );

    const actions: Row[] = [];
    if (theme.update) {
      const version = theme.update;
      actions.push({
        label: `Update to ${version}`,
        detail: theme.current
          ? 'From the theme catalogue; the screen reloads into it'
          : 'From the theme catalogue',
        run: () =>
          page.run(`Updating ${nameOf(theme)}…`, async () => {
            await forma.themes.install(theme.id);
            return `${nameOf(theme)} ${version} installed`;
          })
      });
    }
    if (!theme.current && !theme.unavailable) {
      actions.push({
        label: 'Use this theme',
        run: () => page.run(`Switching to ${nameOf(theme)}…`, () => forma.themes.select(theme.id))
      });
    }
    if (theme.removable) {
      actions.push({
        label: 'Remove',
        detail: 'Deletes it from this machine',
        run: async () => {
          const now = Date.now();
          if (now > confirmUntil) {
            confirmUntil = now + CONFIRM_MS;
            notify(`Press A again to remove ${nameOf(theme)}`);
            return;
          }
          confirmUntil = 0;
          let removed = false;
          await page.run('Removing…', async () => {
            await forma.themes.remove(theme.id);
            removed = true;
            return `${nameOf(theme)} removed`;
          });
          if (removed) await page.pop();
        }
      });
    } else if (theme.current && theme.origin === 'user') {
      actions.push({ label: 'Remove', detail: 'In use: switch to another theme first' });
    }
    if (actions.length > 0) actions[0]!.heading = 'Theme';
    return [...rows, ...actions];
  };
}

/** What the catalogue says about a theme, in a row's detail. */
function catalogueDetail(t: CatalogueTheme): string {
  if (!t.runsHere) return 'Needs a newer Forma';
  if (t.update) return `Installed ${t.installed ?? ''} · update to ${t.version}`;
  if (t.installed) return 'Installed';
  return t.author ? `By ${t.author}` : 'A to see it';
}

/**
 * The theme catalogue: themes to install and updates, each opening its own page. Offline, the
 * list fetched last, with a note saying so.
 */
export function catalogueScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void
): () => Promise<Row[]> {
  return async () => {
    const { data, stale, fetchedAt } = await forma.themes.catalogue();
    const rows: Row[] = data.themes.map((t) => ({
      label: `${t.name} ${t.version}`,
      detail: catalogueDetail(t),
      run: () => page.push(t.name, catalogueThemeScreen(forma, page, notify, t.id))
    }));
    if (rows.length === 0) rows.push({ label: 'No themes in the catalogue yet' });
    if (stale) {
      rows.unshift({
        label: 'Offline',
        detail: `The list from ${fetchedAt.replace('T', ' ')} UTC`
      });
    }
    return rows;
  };
}

/** One theme in the catalogue: what it is, what it would be allowed to do, and Install. */
function catalogueThemeScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void,
  id: string
): () => Promise<Row[]> {
  let confirmUntil = 0;
  return async () => {
    const t = (await forma.themes.catalogue()).data.themes.find((c) => c.id === id);
    if (!t) return [{ label: 'No longer in the catalogue' }];
    const status = t.installed || !t.runsHere ? ` · ${catalogueDetail(t)}` : '';
    const rows: Row[] = [{ label: 'Version', detail: `${t.version}${status}` }];
    if (t.author) rows.push({ label: 'Made by', detail: t.author });
    if (t.description) rows.push({ label: t.description });
    rows.push(
      ...(t.permissionDescriptions.length > 0
        ? t.permissionDescriptions.map((what, i) => ({
            heading: i === 0 ? 'It would be allowed to' : undefined,
            label: sentence(what)
          }))
        : [{ heading: 'It would be allowed to', label: 'Only show your library and play games' }])
    );
    if (!t.runsHere) {
      rows.push({
        heading: 'Theme',
        label: 'Install',
        detail: 'Needs a newer Forma: update Forma first'
      });
      return rows;
    }
    if (t.installed && !t.update) return rows;
    const asks = t.permissions.length;
    rows.push({
      heading: 'Theme',
      label: t.update ? `Update to ${t.version}` : 'Install',
      detail: asks > 0 ? 'A twice: it gets the permissions above' : undefined,
      run: async () => {
        const now = Date.now();
        if (asks > 0 && now > confirmUntil) {
          confirmUntil = now + CONFIRM_MS;
          notify(`Press A again to let ${t.name} do what is listed`);
          return;
        }
        confirmUntil = 0;
        await page.run(t.update ? `Updating ${t.name}…` : `Installing ${t.name}…`, async () => {
          await forma.themes.install(t.id);
          return `${t.name} ${t.version} installed · Interface to use it`;
        });
      }
    });
    return rows;
  };
}
