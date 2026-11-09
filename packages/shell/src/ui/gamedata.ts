import type { Forma, ScraperOffers, ScraperSettings, VersionGroup } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';
import { backupNote, themeUse } from './scraper.js';

/** The fields a user can type, in order; `hint` says how, for those it matters for. */
const EDITABLE: { field: string; label: string; hint?: string }[] = [
  { field: 'name', label: 'Name' },
  { field: 'description', label: 'Description' },
  { field: 'developer', label: 'Developer' },
  { field: 'publisher', label: 'Publisher' },
  { field: 'genre', label: 'Genre' },
  { field: 'players', label: 'Players' },
  { field: 'releaseDate', label: 'Release date', hint: 'YYYY, YYYY-MM or YYYY-MM-DD' },
  { field: 'rating', label: 'Rating', hint: 'From 0 to 100' }
];

/** Select on a game: its details by hand, and scraping it choosing from every scraper. */
export function gameRows(forma: Forma, page: Page, game: { id: number; name: string }): Row[] {
  return [
    {
      icon: 'scrape',
      label: 'Scrape',
      detail: 'Find its details and pictures online, and choose',
      run: () => pick(forma, page, game)
    },
    {
      icon: 'edit',
      label: 'Edit details',
      detail: 'Change its name, description and more by hand',
      run: () => page.push(`Edit ${game.name}`, editScreen(forma, page, game.id))
    }
  ];
}

/** Last on the page, apart from the rest. */
export function deleteGameRow(forma: Forma, page: Page, game: { id: number; name: string }): Row {
  return {
    icon: 'delete',
    danger: true,
    label: 'Delete',
    detail: 'Remove the game and its files',
    run: () => page.push(`Delete ${game.name}?`, deleteScreen(forma, page, game))
  };
}

/** Keeping it comes first, so a stray A does no harm. */
function deleteScreen(forma: Forma, page: Page, game: { id: number; name: string }): () => Row[] {
  return () => [
    { label: 'Keep it', detail: 'Go back', run: () => page.pop() },
    {
      danger: true,
      label: 'Delete for good',
      detail: 'Its ROM, its details and its pictures. This cannot be undone',
      run: () =>
        page.run('Deleting…', async () => {
          await forma.library.deleteGame(game.id);
          page.close();
          return `Deleted ${game.name}`;
        })
    }
  ];
}

/** A rating as the user reads and types it: 0.85 is `85`. */
const percent = (rating: string) => String(Math.round(Number(rating) * 100));

function editScreen(forma: Forma, page: Page, id: number): () => Promise<Row[]> {
  // What was saved here, until the library has read it back from the gamelist.
  const saved = new Map<string, string>();

  return async () => {
    const game = await forma.library.game(id);
    if (!game) return [{ label: 'This game is gone' }];
    const stored: Record<string, string | null> = {
      name: game.name,
      description: game.description,
      developer: game.developer,
      publisher: game.publisher,
      genre: game.genre,
      players: game.players,
      releaseDate: game.releaseDate,
      rating: game.rating === null ? null : String(game.rating)
    };
    return EDITABLE.map(({ field, label, hint }) => {
      const value = saved.get(field) ?? stored[field] ?? '';
      const shown = field === 'rating' && value ? percent(value) : value;
      return {
        label,
        detail: shown || (hint ?? 'Empty'),
        run: async () => {
          const typed = await page.ask({
            title: hint ? `${label} (${hint})` : label,
            value: shown
          });
          if (typed === null || typed === shown) return;
          const text = typed.trim();
          const next = field === 'rating' && text ? String(Number(text) / 100) : text;
          await page.run('Saving…', async () => {
            if (field === 'rating' && !(Number(next) >= 0 && Number(next) <= 1)) {
              return 'A rating is a number from 0 to 100';
            }
            await forma.scraper.edit(id, { [field]: next });
            saved.set(field, next);
          });
        }
      };
    });
  };
}

/** Start → Games → Game versions: which titles have several files, per system. Read only. */
export function versionsScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const systems = await forma.library.systems();
    const found = await Promise.all(
      systems.map(async (s) => ({ system: s, groups: await forma.library.versions(s.name) }))
    );
    const rows = found
      .filter(({ groups }) => groups.length > 0)
      .map(({ system, groups }): Row => {
        const label = system.fullname || system.name;
        return {
          label,
          detail: `${groups.length} ${groups.length === 1 ? 'title' : 'titles'} with several versions`,
          run: () => page.push(label, () => groups.map((g) => versionGroupRow(page, g)))
        };
      });
    return rows.length > 0 ? rows : [{ label: 'Every game has just one version' }];
  };
}

function versionGroupRow(page: Page, group: VersionGroup): Row {
  const identical = group.games.filter((g) => g.identicalTo !== null).length;
  return {
    label: group.title,
    detail: `${group.games.length} versions`,
    badge: identical > 0 ? { text: `${identical} identical`, kept: true } : undefined,
    run: () =>
      page.push(group.title, () =>
        group.games.map((g) => ({
          label: g.tags.join(' · ') || 'No tags',
          detail: g.path,
          badge: g.identicalTo !== null ? { text: 'Identical copy', kept: true } : undefined
        }))
      )
  };
}

/** What tells an offer from the others: `USA · Small file · MP4 · 12 MB`. Empty if nothing does. */
export function offerNote(offer: ScraperOffers['media'][number]['offers'][number]): string {
  const { region, variant, format, size } = offer;
  const megabytes = size === null ? null : size / 1_000_000;
  const weight =
    megabytes === null
      ? null
      : megabytes >= 1
        ? `${megabytes.toFixed(megabytes >= 10 ? 0 : 1)} MB`
        : `${Math.max(1, Math.round(megabytes * 1000))} KB`;
  return [region, variant, format?.toUpperCase(), weight].filter(Boolean).join(' · ');
}

/** The badge of a field no scraper's answer is chosen for. */
const keptBadge = (current: unknown): NonNullable<Row['badge']> => ({
  text: current ? 'Keep current' : 'Stays empty',
  kept: true
});

/** Asks every scraper about the game, then opens the page to choose from. */
function pick(forma: Forma, page: Page, game: { id: number; name: string }): Promise<void> {
  return page.run('Asking every scraper…', async () => {
    const [offers, settings] = await Promise.all([
      forma.scraper.offers(game.id),
      forma.scraper.settings()
    ]);
    if (offers.text.length === 0 && offers.media.length === 0) {
      return offers.problems[0] ?? 'No scraper knows this game';
    }
    await page.push(`Scrape ${game.name}`, pickScreen(forma, page, game.id, offers, settings));
  });
}

function pickScreen(
  forma: Forma,
  page: Page,
  id: number,
  offers: ScraperOffers,
  { theme, mediaKinds }: ScraperSettings
): () => Row[] {
  // Picked by default: the first scraper's, for what the game does not have. The rest stays
  // as it is until the user says otherwise.
  const text = new Map<string, string>();
  const media = new Map<string, number>();
  for (const s of offers.text) {
    if (s.current === null) text.set(s.field, s.offers[0]!.value);
  }
  for (const s of offers.media) {
    const use = themeUse(theme, s.kind);
    const replaced = offers.media.some((m) => m.kind === theme?.backups[s.kind]);
    if (!s.current && use !== 'unused' && !(use === 'backup' && replaced)) {
      media.set(s.kind, s.offers[0]!.id);
    }
  }
  const previews = new Map<number, string | null>();

  const textRow = (section: ScraperOffers['text'][number]): Row => {
    const value = text.get(section.field);
    const from = section.offers.find((o) => o.value === value);
    return {
      label: section.label,
      // Only what would be written; where it comes from is the badge.
      detail: from ? value : (section.current ?? ''),
      badge: from
        ? { text: from.note ? `From ${from.source} · ${from.note}` : `From ${from.source}` }
        : keptBadge(section.current),
      run: () =>
        page.push(section.label, () => [
          {
            label: section.current === null ? 'Leave empty' : 'Keep what it has',
            detail: section.current ?? '',
            run: choose(() => text.delete(section.field))
          },
          ...section.offers.map((o) => ({
            label: o.note ? `${o.source} · ${o.note}` : o.source,
            detail: o.value,
            run: choose(() => text.set(section.field, o.value))
          }))
        ])
    };
  };

  const choose = (change: () => unknown) => async () => {
    change();
    await page.pop();
  };

  const mediaRow = (section: ScraperOffers['media'][number]): Row => {
    const offer = section.offers.find((o) => o.id === media.get(section.kind));
    return {
      label: section.label,
      detail: [
        offer && offerNote(offer),
        themeUse(theme, section.kind) === 'backup' && backupNote(theme, section.kind, mediaKinds)
      ]
        .filter(Boolean)
        .join(' · '),
      badge: offer ? { text: `From ${offer.source}` } : keptBadge(section.current),
      image: offer ? previews.get(offer.id) : undefined,
      run: () =>
        page.run(`Loading ${section.label}…`, async () => {
          for (const [n, o] of section.offers.entries()) {
            if (previews.has(o.id)) continue;
            page.progress(`Loading ${section.label}: ${n + 1} of ${section.offers.length}…`);
            previews.set(o.id, await forma.scraper.preview(o.id));
          }
          await page.push(section.label, () => [
            {
              label: section.current ? 'Keep what it has' : 'Leave empty',
              run: choose(() => media.delete(section.kind))
            },
            ...section.offers.map((o) => ({
              label: o.source,
              detail: offerNote(o) || (previews.get(o.id) ? '' : 'No details from this source'),
              image: previews.get(o.id),
              run: choose(() => media.set(section.kind, o.id))
            }))
          ]);
        })
    };
  };

  return () => {
    const changes = text.size + media.size;
    const unused = offers.media.filter((s) => themeUse(theme, s.kind) === 'unused');
    const sections: Row[] = [
      ...offers.text.map(textRow),
      ...offers.media.filter((s) => !unused.includes(s)).map(mediaRow),
      ...unused.map((s, i) => ({
        ...mediaRow(s),
        heading: i === 0 ? `Not used by ${theme?.name ?? 'this theme'}` : undefined
      }))
    ];
    sections[0]!.heading ??= 'What will be saved · A to change';
    return [
      {
        label: 'Save',
        detail: changes === 1 ? '1 change' : `${changes} changes`,
        run:
          changes === 0
            ? undefined
            : () =>
                page.run('Saving…', async () => {
                  const problems = await forma.scraper.apply(id, Object.fromEntries(text), [
                    ...media.values()
                  ]);
                  await page.pop();
                  return problems[0] ?? 'Saved';
                })
      },
      ...sections,
      ...offers.problems.map((label, i) => ({
        heading: i === 0 ? 'Could not answer' : undefined,
        label
      }))
    ];
  };
}
