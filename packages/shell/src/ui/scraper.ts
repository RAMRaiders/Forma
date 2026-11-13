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
 * Start → Games → Scrapers: choose the scrapers (in order) and their accounts, what to fetch,
 * and scrape the selected game, a system or everything, in the background. The page follows a
 * run as it goes.
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
      rows.push({
        heading: 'Scrape',
        label: 'Start scraping',
        detail: usable
          ? 'A game, a system, or everything'
          : 'Turn on a scraper that is set up first',
        run: usable ? () => page.push('Start scraping', scope) : undefined
      });
      if (status.scope) {
        rows.push({ label: `Last: ${status.scope}`, detail: summary(status) });
      }
    }
    for (const problem of status.problems.slice(-3)) rows.push({ label: problem });

    rows.push(
      ...settings.sources.map((s, i) => ({
        heading: i === 0 ? 'Scrapers, asked in this order' : undefined,
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
      })),
      {
        heading: 'Options',
        label: 'What to fetch',
        detail: `${settings.media.length} kinds of media${settings.metadata ? ' and details' : ''}`,
        run: () => page.push('What to fetch', fetchScreen(forma, page))
      }
    );
    return rows;
  };

  /** What a run covers: the selected game, its system, one to choose, or everything. */
  const scope = (): Row[] => {
    const leaving = (what: string, target: ScrapeScope) => async () => {
      await page.pop();
      await start(target, what);
    };
    const rows: Row[] = [];
    if (context.game) {
      rows.push({
        label: `Scrape ${context.game.name}`,
        run: leaving(context.game.name, { kind: 'game', id: context.game.id })
      });
    }
    if (context.system) {
      const name = context.system.fullname || context.system.name;
      rows.push({
        label: `Scrape every ${name} game`,
        run: leaving(name, { kind: 'system', name: context.system.name })
      });
    }
    rows.push(
      {
        label: 'Scrape a system…',
        run: () => page.push('Scrape a system', systemsScreen(forma, page, start))
      },
      {
        label: 'Scrape everything',
        detail: 'Every game of every system',
        run: leaving('everything', { kind: 'all' })
      }
    );
    return rows;
  };

  const stop = forma.events.on('scraper:progress', () => {
    if (!page.has(build)) return stop();
    if (page.showing(build)) void page.refresh();
  });
  return build;
}

/** What a run fetches: details, media kinds, and what the theme in use needs. */
function fetchScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const settings = await forma.scraper.settings();
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
    const theme = settings.theme;
    const fromTheme = new Set(theme && settings.themeNeeds ? theme.media : []);
    const themeNeeds = saving(page, () =>
      forma.scraper.configure({ themeNeeds: !settings.themeNeeds })
    );
    return [
      {
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
      ...(theme
        ? [
            {
              label: `What ${theme.name} asks for`,
              detail: `${onOff(settings.themeNeeds)}${theme.view ? ` · for its ${theme.view} view` : ''} · ${themeAsks(theme, settings.mediaKinds)}`,
              run: themeNeeds,
              adjust: themeNeeds
            }
          ]
        : []),
      ...[
        ...settings.mediaKinds.filter(({ kind }) => themeUse(theme, kind) !== 'unused'),
        ...settings.mediaKinds.filter(({ kind }) => themeUse(theme, kind) === 'unused')
      ].map(({ kind, label }, i, all) => {
        const unused = themeUse(theme, kind) === 'unused';
        const firstUnused = unused && (i === 0 || themeUse(theme, all[i - 1]!.kind) !== 'unused');
        const state =
          !kinds.has(kind) && fromTheme.has(kind)
            ? `On · for ${theme?.name ?? 'the theme'}`
            : onOff(kinds.has(kind));
        return {
          heading: firstUnused ? `Not used by ${theme?.name ?? 'this theme'}` : undefined,
          label,
          detail:
            themeUse(theme, kind) === 'backup'
              ? `${state} · ${backupNote(theme, kind, settings.mediaKinds)}`
              : state,
          run: toggleKind(kind),
          adjust: toggleKind(kind)
        };
      })
    ];
  };
}

/**
 * How the theme in use treats a kind of media: draws it, draws it only when another is missing,
 * or never draws it. A theme that does not say draws everything, as far as anyone can tell.
 */
export function themeUse(
  theme: ScraperSettings['theme'],
  kind: MediaKind
): 'shown' | 'backup' | 'unused' {
  if (!theme || theme.shows.length === 0) return 'shown';
  if (theme.backups[kind]) return 'backup';
  return theme.shows.includes(kind) ? 'shown' : 'unused';
}

/** `Cyber uses this only when there is no Wheel`. */
export function backupNote(
  theme: ScraperSettings['theme'],
  kind: MediaKind,
  kinds: ScraperSettings['mediaKinds']
): string {
  const main = theme?.backups[kind];
  return `${theme?.name ?? 'The theme'} uses this only when there is no ${kinds.find((k) => k.kind === main)?.label ?? main}`;
}

/** What a theme asks the scraper for, in words: `Box spine, Cartridge, age ratings`. */
function themeAsks(
  theme: { media: MediaKind[]; fields: string[] },
  kinds: { kind: MediaKind; label: string }[]
): string {
  const words = theme.media.map((k) => kinds.find((x) => x.kind === k)?.label ?? k);
  if (theme.fields.includes('ageRating')) words.push('age ratings');
  return words.join(', ');
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
function accountRows(
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
