import type { Forma, ThemeInfo } from '@ramraiders/forma-sdk';
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
