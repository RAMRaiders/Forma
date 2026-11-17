import type {
  Forma,
  MediaKind,
  ScraperSettings,
  ScraperSource,
  ScraperStatus
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
 * Start → ROMs → Media: the scrapers' accounts, the region profile, and scraping the systems
 * chosen, in the background. What is fetched is the theme's to say. The page follows a run.
 */
export function mediaScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  // The systems to scrape, chosen on this page: none until the user picks them.
  let chosen: Set<string> | null = null;

  const build = async (): Promise<Row[]> => {
    const [settings, status] = await Promise.all([
      forma.scraper.settings(),
      forma.scraper.status()
    ]);
    const rows: Row[] = [];
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
    } else if (status.scope) {
      rows.push({ heading: 'Last run', label: status.scope, detail: summary(status) });
    }
    for (const problem of status.problems.slice(-3)) rows.push({ label: problem });
    const on = settings.sources.filter((s) => s.enabled);
    const unready = on.filter((s) => !s.ready).length;
    rows.push(
      {
        heading: 'Media',
        label: 'Accounts',
        detail: `${on.length} of ${settings.sources.length} services on${unready ? ` · ${unready} to set up` : ''}`,
        run: () => page.push('Accounts', accountsScreen(forma, page))
      },
      {
        label: 'Profile',
        detail: profileSummary(settings),
        run: () => page.push('Profile', profileScreen(forma, page))
      },
      {
        label: 'Scrape',
        detail: status.running ? 'Scraping now: see above' : 'Choose the systems, then start',
        run: status.running
          ? undefined
          : () =>
              page.push(
                'Scrape',
                scrapeScreen(
                  forma,
                  page,
                  () => chosen,
                  (c) => (chosen = c)
                )
              )
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

/** The profile in a line: `This game's own region, then World, USA, Europe`. */
function profileSummary(s: ScraperSettings): string {
  const list = s.regions.join(', ') || 'none chosen';
  return s.ownRegion ? `The game's own region, then ${list}` : list;
}

/** The services, in the order they are asked: A sets one up, Y turns it on or off. */
function accountsScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const settings = await forma.scraper.settings();
    return settings.sources.map((s, i) => ({
      heading: i === 0 ? 'Asked in this order' : undefined,
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
    }));
  };
}

/**
 * The region profile: whether a game's own region (from its file name) comes first, and the
 * regions preferred, best first, which also choose between a game's several. Left and right
 * move one up or down the list; Y takes it off.
 */
function profileScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const settings = await forma.scraper.settings();
    const { regions } = settings;
    const set = (next: string[]) => saving(page, () => forma.scraper.configure({ regions: next }));
    const own = saving(page, () => forma.scraper.configure({ ownRegion: !settings.ownRegion }));
    const manuals = saving(page, () => forma.scraper.configure({ manuals: !settings.manuals }));
    const left = settings.allRegions.filter((r) => !regions.includes(r));
    return [
      {
        label: "The game's own region first",
        detail: settings.ownRegion
          ? 'On: a Europe copy gets its European name and box. Of several, the one higher below'
          : 'Off: always the regions below, whatever the game is',
        run: own,
        adjust: own
      },
      {
        label: 'Download manuals',
        detail: settings.manuals
          ? 'On: a game gets its manual, where the theme shows manuals. They are large'
          : 'Off: no manuals, to save space and time',
        run: manuals,
        adjust: manuals
      },
      ...regions.map((r, i): Row => ({
        heading: i === 0 ? 'Preferred regions, best first' : undefined,
        // The name alone: the selection follows it as it moves.
        label: r,
        detail: `${i + 1} of ${regions.length} · ◀ ▶ to move it · Y to take it off`,
        adjust: (step) => {
          const to = i + step;
          if (to < 0 || to >= regions.length) return;
          const next = [...regions];
          [next[i], next[to]] = [next[to]!, next[i]!];
          return set(next)();
        },
        alt: { label: 'Take off', run: set(regions.filter((x) => x !== r)) }
      })),
      ...(left.length > 0
        ? [
            {
              heading: regions.length === 0 ? 'Preferred regions, best first' : undefined,
              label: 'Add a region…',
              detail: 'A country, or a region such as Europe',
              run: () =>
                page.push('Add a region', () =>
                  left.map((r) => ({
                    label: r,
                    run: async () => {
                      await page.pop();
                      await set([...regions, r])();
                    }
                  }))
                )
            }
          ]
        : [])
    ];
  };
}

/** What a run does with a game already scraped. */
const EXISTING = [
  { skipScraped: true, replace: false, label: 'Skip them' },
  { skipScraped: false, replace: false, label: 'Fill in what is missing' },
  { skipScraped: false, replace: true, label: 'Replace what they have' }
] as const;

/**
 * Scrape: the systems to scrape, what to do with games already scraped, what is fetched (the
 * theme's), and Start.
 */
function scrapeScreen(
  forma: Forma,
  page: Page,
  chosen: () => Set<string> | null,
  choose: (systems: Set<string>) => void
): () => Promise<Row[]> {
  return async () => {
    const [settings, systems] = await Promise.all([
      forma.scraper.settings(),
      forma.library.systems()
    ]);
    const withGames = systems.filter((s) => s.gameCount > 0);
    const picked = chosen() ?? new Set<string>();
    const names = withGames.filter((s) => picked.has(s.name));
    const games = names.reduce((n, s) => n + s.gameCount, 0);
    const at = EXISTING.findIndex(
      (e) => e.skipScraped === settings.skipScraped && e.replace === settings.replace
    );
    const existing = (step: -1 | 1) => {
      const next = EXISTING[(at + step + EXISTING.length) % EXISTING.length]!;
      return saving(page, () =>
        forma.scraper.configure({ skipScraped: next.skipScraped, replace: next.replace })
      )();
    };
    const usable = settings.sources.some((s) => s.enabled && s.ready);
    const fetched = settings.media.map(
      (k) => settings.mediaKinds.find((x) => x.kind === k)?.label ?? k
    );
    return [
      {
        label: 'Systems',
        detail:
          names.length === 0
            ? 'None chosen yet'
            : names.length === withGames.length
              ? `Every system · ${games} games`
              : `${names.length} of ${withGames.length} · ${games} games`,
        run: () => page.push('Systems', systemsScreen(page, withGames, picked, choose))
      },
      {
        label: 'Games already scraped',
        detail: EXISTING[Math.max(0, at)]!.label,
        run: () => existing(1),
        adjust: existing
      },
      {
        label: 'What is fetched',
        detail: `Details and ${fetched.join(', ')}${settings.theme ? ` · as ${settings.theme.name} asks` : ''}`
      },
      {
        heading: 'Ready',
        label: 'Start scraping',
        detail: !usable
          ? 'Turn on a service that is set up first, under Accounts'
          : names.length === 0
            ? 'Choose a system first'
            : 'In the background: it carries on if you leave',
        run:
          usable && names.length > 0
            ? () =>
                page.run('Starting…', async () => {
                  await forma.scraper.start({ kind: 'systems', names: names.map((s) => s.name) });
                  await page.pop();
                  return 'Scraping in the background: ROMs → Media shows how far it is';
                })
            : undefined
      }
    ];
  };
}

/** Every system with games, each one on or off for the run; A flips one, or all at once. */
function systemsScreen(
  page: Page,
  systems: { name: string; fullname: string; gameCount: number }[],
  picked: Set<string>,
  choose: (systems: Set<string>) => void
): () => Row[] {
  const flip = (change: (next: Set<string>) => void) => async () => {
    const next = new Set(picked);
    change(next);
    picked = next;
    choose(next);
    await page.refresh();
  };
  return () => {
    const on = systems.filter((s) => picked.has(s.name)).length;
    return [
      {
        label: 'Select all',
        detail: `${on} of ${systems.length} on`,
        run: flip((next) => systems.forEach((s) => next.add(s.name)))
      },
      {
        label: 'Unselect all',
        run: flip((next) => next.clear())
      },
      ...systems.map((s, i): Row => ({
        heading: i === 0 ? 'Systems' : undefined,
        label: s.fullname || s.name,
        detail: `${onOff(picked.has(s.name))} · ${s.gameCount} games`,
        run: flip((next) => (next.has(s.name) ? next.delete(s.name) : next.add(s.name)))
      }))
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
  // What it asks for (a view's own, say) it draws too, listed in `shows` or not.
  return theme.shows.includes(kind) || theme.media.includes(kind) ? 'shown' : 'unused';
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
