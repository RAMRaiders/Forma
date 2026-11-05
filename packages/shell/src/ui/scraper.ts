import type {
  Forma,
  MediaKind,
  ScrapeScope,
  ScraperSettings,
  ScraperSource,
  ScraperStatus,
  ShellContext
} from '@ramraiders/forma-sdk';
import type { Page, Row } from './page.js';

const onOff = (on: boolean) => (on ? 'On' : 'Off');

/** A change, then the page read again. */
const saving = (page: Page, work: () => Promise<unknown>) => () =>
  page.run('Saving…', async () => {
    await work();
  });

/** A source's state, in a row's detail. */
function sourceDetail(s: ScraperSource): string {
  if (!s.enabled) return 'Off';
  if (!s.ready) return 'On · needs setting up: A';
  return s.needs.length > 0 ? 'On · set up' : 'On · nothing to set up';
}

/** What a run did, in a line. */
function summary(s: ScraperStatus): string {
  const parts = [`${s.found} found`, `${s.notFound} not found`];
  if (s.skipped > 0) parts.push(`${s.skipped} had everything`);
  if (s.stopped) parts.push(`stopped at ${s.done} of ${s.total}`);
  return parts.join(' · ');
}

/** Sources in order, `id` moved by `step`. */
function moved(settings: ScraperSettings, id: string, step: -1 | 1) {
  const list = settings.sources.map((s) => ({ id: s.id, enabled: s.enabled }));
  const at = list.findIndex((s) => s.id === id);
  const to = at + step;
  if (at < 0 || to < 0 || to >= list.length) return null;
  [list[at], list[to]] = [list[to]!, list[at]!];
  return list;
}

/**
 * Start → Scraper: scrape the selected game, a system or everything, in the background, and
 * choose the sources (in order), their accounts and what to fetch. The page follows a run as
 * it goes.
 */
export function scraperScreen(
  forma: Forma,
  page: Page,
  context: ShellContext
): () => Promise<Row[]> {
  const start = (scope: ScrapeScope, what: string) =>
    page.run(`Starting ${what}…`, async () => {
      await forma.scraper.start(scope);
      return 'Scraping in the background: it carries on if you leave this page';
    });

  const build = async (): Promise<Row[]> => {
    const [settings, status] = await Promise.all([
      forma.scraper.settings(),
      forma.scraper.status()
    ]);
    const rows: Row[] = [];
    const usable = settings.sources.some((s) => s.enabled && s.ready);
    if (status.running) {
      rows.push(
        {
          heading: 'Scraping',
          label: `${status.scope ?? ''}: ${status.done} of ${status.total}`,
          detail: status.current ? `Now ${status.current} · ${summary(status)}` : summary(status)
        },
        {
          label: 'Stop',
          detail: 'After the game it is on',
          run: () => page.run('Stopping…', () => forma.scraper.stop())
        }
      );
    } else {
      const scrape: Row[] = [];
      if (context.game) {
        const game = context.game;
        scrape.push({
          label: `Scrape ${game.name}`,
          run: () => start({ kind: 'game', id: game.id }, game.name)
        });
      }
      if (context.system) {
        const system = context.system;
        const name = system.fullname || system.name;
        scrape.push({
          label: `Scrape every ${name} game`,
          run: () => start({ kind: 'system', name: system.name }, name)
        });
      }
      scrape.push(
        {
          label: 'Scrape a system…',
          run: () => page.push('Scrape a system', systemsScreen(forma, page, start))
        },
        {
          label: 'Scrape everything',
          detail: 'Every game of every system',
          run: () => start({ kind: 'all' }, 'everything')
        }
      );
      if (!usable) {
        for (const r of scrape) {
          r.run = undefined;
          r.detail = 'Turn on a source that is set up first';
        }
      }
      scrape[0]!.heading = 'Scrape';
      rows.push(...scrape);
      if (status.scope) {
        rows.push({ label: `Last: ${status.scope}`, detail: summary(status) });
      }
    }
    for (const problem of status.problems.slice(-3)) rows.push({ label: problem });

    rows.push(
      ...settings.sources.map((s, i) => ({
        heading: i === 0 ? 'Sources, asked in this order' : undefined,
        label: s.name,
        detail: sourceDetail(s),
        run: () => page.push(s.name, sourceScreen(forma, page, s.id)),
        alt: {
          label: s.enabled ? 'Turn off' : 'Turn on',
          run: saving(page, () =>
            forma.scraper.configure({
              sources: settings.sources.map((x) => ({
                id: x.id,
                enabled: x.id === s.id ? !x.enabled : x.enabled
              }))
            })
          )
        }
      }))
    );

    const kinds = new Set(settings.media);
    const toggleKind = (kind: MediaKind) =>
      saving(page, () =>
        forma.scraper.configure({
          media: kinds.has(kind)
            ? settings.media.filter((k) => k !== kind)
            : [...settings.media, kind]
        })
      );
    const metadata = saving(page, () => forma.scraper.configure({ metadata: !settings.metadata }));
    const replace = saving(page, () => forma.scraper.configure({ replace: !settings.replace }));
    rows.push(
      {
        heading: 'What to fetch',
        label: 'Details',
        detail: `${onOff(settings.metadata)} · name, description, developer, genre, date, rating`,
        run: metadata,
        adjust: metadata
      },
      {
        label: 'Games that have some',
        detail: settings.replace ? 'Replace what they have' : 'Only fill what is missing',
        run: replace,
        adjust: replace
      },
      ...settings.mediaKinds.map(({ kind, label }) => ({
        label,
        detail: onOff(kinds.has(kind)),
        run: toggleKind(kind),
        adjust: toggleKind(kind)
      }))
    );
    return rows;
  };

  const stop = forma.events.on('scraper:progress', () => {
    if (!page.has(build)) return stop();
    if (page.showing(build)) void page.refresh();
  });
  return build;
}

/** Every system with games, each scraping it. */
function systemsScreen(
  forma: Forma,
  page: Page,
  start: (scope: ScrapeScope, what: string) => Promise<void>
): () => Promise<Row[]> {
  return async () => {
    const systems = await forma.library.systems();
    return systems.map((s) => ({
      label: s.fullname || s.name,
      detail: `${s.gameCount} games`,
      run: async () => {
        await page.pop();
        await start({ kind: 'system', name: s.name }, s.fullname || s.name);
      }
    }));
  };
}

/** One source: on or off, where it is in the order, and its account. */
function sourceScreen(forma: Forma, page: Page, id: string): () => Promise<Row[]> {
  return async () => {
    const settings = await forma.scraper.settings();
    const at = settings.sources.findIndex((s) => s.id === id);
    const s = settings.sources[at];
    if (!s) return [{ label: 'No such source' }];
    const move = (step: -1 | 1) => {
      const sources = moved(settings, id, step);
      return sources ? saving(page, () => forma.scraper.configure({ sources }))() : undefined;
    };
    const toggle = saving(page, () =>
      forma.scraper.configure({
        sources: settings.sources.map((x) => ({
          id: x.id,
          enabled: x.id === id ? !x.enabled : x.enabled
        }))
      })
    );
    const rows: Row[] = [
      { label: s.about },
      {
        label: 'Use it',
        detail: onOff(s.enabled),
        run: toggle,
        adjust: toggle
      },
      {
        label: 'Asked',
        detail: `${at + 1} of ${settings.sources.length} · left and right to move`,
        adjust: move
      }
    ];
    rows.push(...accountRows(forma, page, id, s));
    return rows;
  };
}

/** A source's account: each thing to enter, typed on the keyboard; Y clears it. */
export function accountRows(
  forma: Forma,
  page: Page,
  id: string,
  s: Pick<ScraperSource, 'needs'>,
  heading = 'Account'
): Row[] {
  return s.needs.map((n, i) => ({
    heading: i === 0 ? heading : undefined,
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
        await forma.scraper.setCredential(id, n.key, typed);
        return typed.trim() ? `${n.label} saved` : `${n.label} cleared`;
      });
    },
    alt: n.set
      ? {
          label: 'Clear',
          run: saving(page, () => forma.scraper.setCredential(id, n.key, null))
        }
      : undefined
  }));
}
