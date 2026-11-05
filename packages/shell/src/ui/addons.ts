import type { AddonInfo, Forma } from '@ramraiders/forma-sdk';
import type { Page, Row } from './page.js';
import { accountRows } from './scraper.js';

/** How long a first press of Remove or Install waits for the second. */
const CONFIRM_MS = 3000;

const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

const provides = (a: Pick<AddonInfo, 'provides'>) =>
  a.provides.map((p) => (p === 'scraper' ? 'a scraper source' : p)).join(', ');

function detail(a: AddonInfo): string {
  if (a.problem) return `Cannot run: ${a.problem}`;
  const state = a.enabled ? 'On' : 'Off';
  return a.update ? `${state} · update to ${a.update}` : `${state} · ${provides(a)}`;
}

/** What it may do, and where it may go. */
function mayRows(permissionDescriptions: string[], hosts: string[], heading: string): Row[] {
  const rows: Row[] = permissionDescriptions.map((what, i) => ({
    heading: i === 0 ? heading : undefined,
    label: sentence(what)
  }));
  if (rows.length === 0) rows.push({ heading, label: 'Nothing beyond running in Forma' });
  for (const h of hosts) rows.push({ label: h.replace(/^https:\/\//, '') });
  return rows;
}

/**
 * Start → Add-ons: what is installed, each opening its page, and the catalogue. Add-ons
 * extend Forma itself (a scraper source, for now); they run sandboxed and reach only the sites
 * they list.
 */
export function addonsScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void
): () => Promise<Row[]> {
  return async () => {
    const installed = await forma.addons.list();
    const rows: Row[] = installed.map((a, i) => ({
      heading: i === 0 ? 'Installed' : undefined,
      label: a.version ? `${a.name} ${a.version}` : a.name,
      detail: detail(a),
      run: () => page.push(a.name, addonScreen(forma, page, notify, a.id))
    }));
    if (rows.length === 0) {
      rows.push({ heading: 'Installed', label: 'No add-ons yet', detail: 'Get some below' });
    }
    rows.push({
      heading: 'More',
      label: 'Get more add-ons…',
      detail: 'From the Forma catalogue',
      run: () => page.push('Get add-ons', catalogueScreen(forma, page, notify))
    });
    return rows;
  };
}

/** One add-on: what it is and may do, its settings, on or off, update, remove. */
function addonScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void,
  id: string
): () => Promise<Row[]> {
  let confirmUntil = 0;
  return async () => {
    const a = (await forma.addons.list()).find((x) => x.id === id);
    if (!a) return [{ label: 'This add-on is no longer installed' }];
    const rows: Row[] = [
      { label: 'Version', detail: a.version ?? 'Not given' },
      { label: 'Adds', detail: provides(a) || 'Nothing this Forma knows' }
    ];
    if (a.author) rows.push({ label: 'Made by', detail: a.author });
    if (a.description) rows.push({ label: a.description });
    if (a.problem) rows.push({ label: 'Cannot run', detail: a.problem });
    rows.push(...mayRows(a.permissionDescriptions, a.hosts, 'It may'));

    // A scraper add-on's settings are its account among the scraper's sources.
    if (a.scraperSource && a.enabled) {
      const source = (await forma.scraper.settings()).sources.find((s) => s.id === a.scraperSource);
      if (source && source.needs.length > 0) {
        rows.push(...accountRows(forma, page, source.id, source, 'Settings'));
      }
    }

    const toggle = () =>
      page.run('Saving…', async () => {
        await forma.addons.setEnabled(a.id, !a.enabled);
        return a.enabled ? `${a.name} is off` : `${a.name} is on`;
      });
    const actions: Row[] = [
      {
        label: 'Use it',
        detail: a.enabled ? (a.scraperSource ? 'On · in Start → Scraper’s sources' : 'On') : 'Off',
        run: toggle,
        adjust: toggle
      }
    ];
    if (a.update) {
      const version = a.update;
      actions.push({
        label: `Update to ${version}`,
        run: () =>
          page.run(`Updating ${a.name}…`, async () => {
            await forma.addons.install(a.id);
            return `${a.name} ${version} installed`;
          })
      });
    }
    actions.push({
      label: 'Remove',
      detail: 'Deletes it from this machine',
      run: async () => {
        const now = Date.now();
        if (now > confirmUntil) {
          confirmUntil = now + CONFIRM_MS;
          notify(`Press A again to remove ${a.name}`);
          return;
        }
        confirmUntil = 0;
        let removed = false;
        await page.run('Removing…', async () => {
          await forma.addons.remove(a.id);
          removed = true;
          return `${a.name} removed`;
        });
        if (removed) await page.pop();
      }
    });
    actions[0]!.heading = 'Add-on';
    return [...rows, ...actions];
  };
}

/** The catalogue's add-ons, each with what it would be allowed to do and Install. */
function catalogueScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void
): () => Promise<Row[]> {
  const confirmUntil = new Map<string, number>();
  return async () => {
    const { data, stale, fetchedAt } = await forma.addons.catalogue();
    const rows: Row[] = [];
    if (stale) {
      rows.push({ label: 'Offline', detail: `The list from ${fetchedAt.replace('T', ' ')} UTC` });
    }
    for (const c of data.addons) {
      const status = !c.runsHere
        ? 'Needs a newer Forma'
        : c.update
          ? `Installed ${c.installed ?? ''} · A to update to ${c.version}`
          : c.installed
            ? 'Installed'
            : `${c.description ?? ''} · A twice to install`.replace(/^ · /, '');
      rows.push({
        heading: rows.length === 0 || (stale && rows.length === 1) ? 'Add-ons' : undefined,
        label: `${c.name} ${c.version}`,
        detail: status,
        run:
          !c.runsHere || (c.installed && !c.update)
            ? undefined
            : async () => {
                const now = Date.now();
                if (now > (confirmUntil.get(c.id) ?? 0)) {
                  confirmUntil.set(c.id, now + CONFIRM_MS);
                  const may = c.permissionDescriptions.join('; ') || 'nothing beyond running';
                  notify(`${c.name} may: ${may}. Press A again to install`);
                  return;
                }
                confirmUntil.delete(c.id);
                await page.run(`Installing ${c.name}…`, async () => {
                  await forma.addons.install(c.id);
                  return `${c.name} ${c.version} installed`;
                });
              }
      });
    }
    if (data.addons.length === 0) rows.push({ label: 'No add-ons in the catalogue yet' });
    return rows;
  };
}
