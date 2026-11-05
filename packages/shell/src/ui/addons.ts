import type { AddonInfo, AddonRow, Forma } from '@ramraiders/forma-sdk';
import type { Page, Row } from './page.js';

/** How long a first press of Remove or Install waits for the second. */
const CONFIRM_MS = 3000;

const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

const KINDS: Record<string, string> = {
  scraper: 'a scraper source',
  music: 'web radios in Music',
  menu: 'pages in this menu'
};

const provides = (a: Pick<AddonInfo, 'provides'>) =>
  a.provides.map((p) => KINDS[p] ?? p).join(', ');

/** Whether it has pages to open now. */
const opens = (a: AddonInfo) => Boolean(a.menu && a.enabled && !a.problem);

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
 * Start → Add-ons: the pages of the add-ons that have some, what is installed (each opening
 * its page), and the catalogue. Add-ons extend Forma itself: scraper sources, web radios,
 * pages in this menu. Their scripts run sandboxed and reach only the sites they list; a
 * system part (programs of their own) only from Forma's catalogue.
 */
export function addonsScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void
): () => Promise<Row[]> {
  return async () => {
    const installed = await forma.addons.list();
    const rows: Row[] = installed.filter(opens).map((a, i) => ({
      heading: i === 0 ? 'Open' : undefined,
      label: a.menu!.label,
      detail: a.menu!.description ?? a.name,
      run: () => page.push(a.menu!.label, addonPages(forma, page, a.id, []))
    }));
    rows.push(
      ...installed.map((a, i) => ({
        heading: i === 0 ? 'Installed' : undefined,
        label: a.version ? `${a.name} ${a.version}` : a.name,
        detail: detail(a),
        run: () => page.push(a.name, addonScreen(forma, page, notify, a.id))
      }))
    );
    if (installed.length === 0) {
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
    const rows: Row[] = [];
    if (opens(a)) {
      rows.push({
        label: `Open ${a.menu!.label}`,
        run: () => page.push(a.menu!.label, addonPages(forma, page, a.id, []))
      });
    }
    rows.push(
      { label: 'Version', detail: a.version ?? 'Not given' },
      { label: 'Adds', detail: provides(a) || 'Nothing this Forma knows' }
    );
    if (a.author) rows.push({ label: 'Made by', detail: a.author });
    if (a.description) rows.push({ label: a.description });
    if (a.problem) rows.push({ label: 'Cannot run', detail: a.problem });
    rows.push(...mayRows(a.permissionDescriptions, a.hosts, 'It may'));
    if (a.system) {
      rows.push({
        label: 'Its programs',
        detail: a.verified
          ? 'From Forma’s catalogue: installed with it, removed with it'
          : 'Not from Forma’s catalogue, so they never run'
      });
    }
    rows.push(...settingRows(forma, page, a));

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
                  const warn = c.permissions.includes('system')
                    ? ' It gets full control of this machine.'
                    : '';
                  notify(`${c.name} may: ${may}.${warn} Press A again to install`);
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

/** An add-on's settings, typed on the keyboard; Y clears one. */
function settingRows(forma: Forma, page: Page, a: AddonInfo): Row[] {
  return a.settings.map((n, i) => ({
    heading: i === 0 ? 'Settings' : undefined,
    label: n.label,
    detail: n.set
      ? n.secret
        ? 'Entered · Y to clear'
        : `${n.value ?? ''} · Y to clear`
      : n.required
        ? 'Needed: A to enter'
        : 'Optional: A to enter',
    run: async () => {
      const typed = await page.ask({
        title: n.label,
        value: n.secret ? '' : (n.value ?? ''),
        secret: n.secret
      });
      if (typed === null) return;
      await page.run('Saving…', async () => {
        await forma.addons.setSetting(a.id, n.key, typed);
        return typed.trim() ? `${n.label} saved` : `${n.label} cleared`;
      });
    },
    alt: n.set
      ? {
          label: 'Clear',
          run: () =>
            page.run('Saving…', async () => {
              await forma.addons.setSetting(a.id, n.key, null);
            })
        }
      : undefined
  }));
}

/**
 * One of an add-on's pages, drawn as Forma draws its own: the add-on gives rows, Forma does
 * the rest (the pad, the keyboard, the theme). A row opens a deeper page or asks the add-on to
 * act; what the add-on answers (a message, a page to open, back) Forma does.
 */
function addonPages(forma: Forma, page: Page, id: string, path: string[]): () => Promise<Row[]> {
  const act = async (action: string, input?: string | number) => {
    let next: { open: string[] | null; back: boolean } = { open: null, back: false };
    await page.run('Working…', async () => {
      const done = await forma.addons.action(id, action, input);
      next = done;
      return done.notify ?? undefined;
    });
    if (next.back) await page.pop();
    else if (next.open)
      await page.push(next.open.at(-1) ?? '', addonPages(forma, page, id, next.open));
  };
  const row = (r: AddonRow): Row => {
    const out: Row = {
      label: r.label,
      detail: r.detail ?? undefined,
      heading: r.heading ?? undefined
    };
    if (r.open) {
      const to = r.open;
      out.run = () => page.push(r.label, addonPages(forma, page, id, to));
    } else if (r.action) {
      const action = r.action;
      out.run = r.ask
        ? async () => {
            const typed = await page.ask({
              title: r.ask!.title,
              value: r.ask!.value ?? '',
              secret: r.ask!.secret
            });
            if (typed !== null) await act(action, typed);
          }
        : () => act(action);
    }
    if (r.adjust) {
      const adjust = r.adjust;
      out.adjust = (step) => act(adjust, step);
    }
    if (r.alt) {
      const alt = r.alt;
      out.alt = { label: alt.label, run: () => act(alt.action) };
    }
    return out;
  };
  return async () => {
    const shown = await forma.addons.page(id, path);
    const rows = shown.rows.map(row);
    return rows.length > 0 ? rows : [{ label: 'Nothing here' }];
  };
}
