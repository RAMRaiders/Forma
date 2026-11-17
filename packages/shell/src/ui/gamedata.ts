import type {
  Forma,
  MediaKind,
  ScraperOffers,
  ScraperSettings,
  VersionGroup
} from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';
import { backupNote, themeUse } from './scraper.js';
import { emulatorScreen } from './shader.js';

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

/** The file a game comes from, whatever it has been named since. For a page's note: read only. */
async function fileNote(forma: Forma, id: number): Promise<string | undefined> {
  const game = await forma.library.game(id);
  return game ? `File: ${game.path.split('/').pop()}` : undefined;
}

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
      run: async () =>
        page.push(
          `Edit ${game.name}`,
          editScreen(forma, page, game.id),
          await fileNote(forma, game.id)
        )
    },
    {
      icon: 'emulator',
      label: 'Game settings',
      detail: 'Emulator, CRT shader, picture, decorations and gameplay, for this game only',
      run: () =>
        page.push(
          `${game.name} settings`,
          emulatorScreen(forma, page, { level: 'game', game: game.id })
        )
    }
  ];
}

/**
 * Last on the page, apart from the rest. A page of its own asks first, saying what goes, with
 * keeping it chosen: a stray A does no harm.
 */
export function deleteGameRow(forma: Forma, page: Page, game: { id: number; name: string }): Row {
  return {
    icon: 'delete',
    danger: true,
    label: 'Delete',
    detail: 'Its ROM, details and pictures, for good',
    run: async () => {
      const found = await forma.library.game(game.id).catch(() => null);
      const file = found?.path.split('/').pop();
      // Its box (or a screenshot), so there is no doubt which game goes.
      const has = (found?.media ?? {}) as Partial<Record<string, string>>;
      const kind = (['thumbnail', 'image'] as const).find((k) => has[k]);
      const picture = kind ? forma.library.mediaUrl(game.id, kind, 320) : null;
      await page.push(
        `Delete ${game.name}?`,
        () => [
          {
            label: 'Keep it',
            detail: 'Nothing is deleted',
            run: () => page.pop()
          },
          {
            icon: 'delete',
            danger: true,
            label: 'Delete for good',
            detail: file
              ? `${file}, its details, pictures and videos`
              : 'Its ROM, details and pictures',
            image: picture,
            run: () =>
              page.run('Deleting…', async () => {
                await forma.library.deleteGame(game.id);
                page.close();
                return `Deleted ${game.name}`;
              })
          }
        ],
        'It goes from the library and from the disk. This cannot be undone.'
      );
    }
  };
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

/** Start → ROMs → Game versions: which titles have several files, per system. Read only. */
export function versionsScreen(
  forma: Forma,
  page: Page,
  openGame: (id: number) => void
): () => Promise<Row[]> {
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
          run: () => page.push(label, () => groups.map((g) => versionGroupRow(page, g, openGame)))
        };
      });
    return rows.length > 0 ? rows : [{ label: 'Every game has just one version' }];
  };
}

/**
 * Start → ROMs → Sibling games: games that are one game under different names, per
 * system, by LaunchBox. Read only.
 */
export function duplicatesScreen(
  forma: Forma,
  page: Page,
  openGame: (id: number) => void
): () => Promise<Row[]> {
  return async () => {
    const systems = await forma.library.systems();
    const found = await Promise.all(
      systems.map(async (s) => ({ system: s, groups: await forma.library.duplicates(s.name) }))
    );
    if (found.every(({ groups }) => groups === null)) {
      return [
        {
          label: "Needs LaunchBox's data",
          detail: 'Turn LaunchBox on in Scraper and scrape a game once'
        }
      ];
    }
    const rows = found.flatMap(({ system, groups }): Row[] => {
      if (!groups?.length) return [];
      const label = system.fullname || system.name;
      return [
        {
          label,
          detail: `${groups.length} ${groups.length === 1 ? 'game' : 'games'} under several names`,
          run: () =>
            page.push(label, () =>
              groups.map((g) => ({
                label: g.title,
                detail: [...new Set(g.games.map((x) => x.name))].join(' · '),
                run: () =>
                  page.push(g.title, () =>
                    g.games.map((x) => ({
                      label: x.name,
                      detail: x.path,
                      run: () => openGame(x.id)
                    }))
                  )
              }))
            )
        }
      ];
    });
    return rows.length > 0 ? rows : [{ label: 'No game is there twice under another name' }];
  };
}

function versionGroupRow(page: Page, group: VersionGroup, openGame: (id: number) => void): Row {
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
          badge: g.identicalTo !== null ? { text: 'Identical copy', kept: true } : undefined,
          run: () => openGame(g.id)
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
    const [offers, settings, note] = await Promise.all([
      forma.scraper.offers(game.id),
      forma.scraper.settings(),
      fileNote(forma, game.id)
    ]);
    // Nothing offered and nothing to remove (a name stays).
    const offered = [...offers.text, ...offers.media].some((s) => s.offers.length > 0);
    const removable = offers.media.length > 0 || offers.text.some((s) => s.field !== 'name');
    if (!offered && !removable) {
      return offers.problems[0] ?? 'No scraper knows this game';
    }
    await page.push(
      `Scrape ${game.name}`,
      pickScreen(forma, page, game.id, offers, settings),
      note
    );
  });
}

function pickScreen(
  forma: Forma,
  page: Page,
  id: number,
  offers: ScraperOffers,
  { theme, mediaKinds }: ScraperSettings
): () => Row[] {
  // Picked by default: the first scraper's, only for a game scraped for the first time (a name
  // alone is not a scrape). Any other stays as it is, gaps too, until the user says otherwise.
  const firstTime =
    !offers.media.some((s) => s.current) &&
    !offers.text.some((s) => s.field !== 'name' && s.current !== null);
  const text = new Map<string, string>();
  const media = new Map<string, number>();
  // Fields to empty on saving: what the game has now goes, and nothing takes its place.
  const cleared = new Set<string>();
  // Media to delete on saving, likewise.
  const removed = new Set<MediaKind>();
  for (const s of firstTime ? offers.text : []) {
    const first = s.offers[0];
    if (s.current === null && first) text.set(s.field, first.value);
  }
  for (const s of firstTime ? offers.media : []) {
    const use = themeUse(theme, s.kind);
    const replaced = offers.media.some((m) => m.kind === theme?.backups[s.kind]);
    const first = s.offers[0];
    if (first && use !== 'unused' && !(use === 'backup' && replaced)) {
      media.set(s.kind, first.id);
    }
  }
  const previews = new Map<number, string | null>();

  const textRow = (section: ScraperOffers['text'][number]): Row => {
    const value = text.get(section.field);
    const from = section.offers.find((o) => o.value === value);
    const typed = value !== undefined && !from;
    const typeIt = async () => {
      const name = await page.ask({
        title: section.label,
        value: value ?? section.current ?? section.offers[0]?.value ?? ''
      });
      const trimmed = name?.trim();
      if (!trimmed) return;
      cleared.delete(section.field);
      text.set(section.field, trimmed);
      await page.pop();
    };
    return {
      label: section.label,
      // Only what would be written; where it comes from is the badge.
      detail: from || typed ? value : cleared.has(section.field) ? '' : (section.current ?? ''),
      badge: from
        ? { text: from.note ? `From ${from.source} · ${from.note}` : `From ${from.source}` }
        : typed
          ? { text: 'Typed by you' }
          : cleared.has(section.field)
            ? { text: 'Will be cleared' }
            : keptBadge(section.current),
      run: () =>
        page.push(section.label, () => [
          {
            label: section.current === null ? 'Leave empty' : 'Keep what it has',
            detail: section.current ?? '',
            run: choose(() => {
              text.delete(section.field);
              cleared.delete(section.field);
            })
          },
          ...section.offers.map((o) => ({
            label: o.note ? `${o.source} · ${o.note}` : o.source,
            detail: o.value,
            run: choose(() => {
              cleared.delete(section.field);
              text.set(section.field, o.value);
            })
          })),
          // A game keeps its name; anything else it has can go.
          ...(section.field !== 'name' && section.current !== null
            ? [
                {
                  label: 'Remove and delete',
                  detail: 'Removes what it has now, and takes nothing in its place',
                  run: choose(() => {
                    text.delete(section.field);
                    cleared.add(section.field);
                  })
                }
              ]
            : []),
          ...(section.field === 'name'
            ? [{ label: 'Type it yourself', detail: typed ? value : '', run: typeIt }]
            : [])
        ])
    };
  };

  const choose = (change: () => unknown) => async () => {
    change();
    await page.pop();
  };

  // What the game has now, to show on its tile while it is kept.
  const current = (section: ScraperOffers['media'][number]) =>
    section.current ? forma.library.mediaUrl(id, section.kind, 320) : null;

  const mediaRow = (section: ScraperOffers['media'][number]): Row => {
    const offer = section.offers.find((o) => o.id === media.get(section.kind));
    return {
      tile: true,
      label: section.label,
      detail: [
        offer && offerNote(offer),
        themeUse(theme, section.kind) === 'backup' && backupNote(theme, section.kind, mediaKinds)
      ]
        .filter(Boolean)
        .join(' · '),
      badge: offer
        ? { text: `From ${offer.source}` }
        : removed.has(section.kind)
          ? { text: 'Will be deleted' }
          : keptBadge(section.current),
      image: offer ? previews.get(offer.id) : removed.has(section.kind) ? null : current(section),
      run: () =>
        page.run(`Loading ${section.label}…`, async () => {
          for (const [n, o] of section.offers.entries()) {
            if (previews.has(o.id)) continue;
            page.progress(`Loading ${section.label}: ${n + 1} of ${section.offers.length}…`);
            previews.set(o.id, await forma.scraper.preview(o.id));
          }
          const chosen = media.get(section.kind);
          await page.push(
            section.label,
            () => [
              {
                tile: true,
                label: section.current ? 'Keep what it has' : 'Leave empty',
                image: current(section),
                badge: chosen === undefined ? { text: 'Chosen', kept: true } : undefined,
                run: choose(() => {
                  media.delete(section.kind);
                  removed.delete(section.kind);
                })
              },
              ...section.offers.map((o): Row => ({
                tile: true,
                label: o.source,
                detail: offerNote(o) || (previews.get(o.id) ? '' : 'No details from this source'),
                image: previews.get(o.id),
                badge: chosen === o.id ? { text: 'Chosen' } : undefined,
                run: choose(() => {
                  removed.delete(section.kind);
                  media.set(section.kind, o.id);
                })
              })),
              ...(section.current
                ? [
                    {
                      danger: true,
                      label: 'Remove and delete',
                      detail: 'Deletes its file, and takes nothing in its place',
                      run: choose(() => {
                        media.delete(section.kind);
                        removed.add(section.kind);
                      })
                    }
                  ]
                : [])
            ],
            undefined,
            // On what is chosen now.
            chosen === undefined ? 0 : 1 + section.offers.findIndex((o) => o.id === chosen)
          );
        })
    };
  };

  // The pictures chosen, fetched one by one once the page is up, each tile showing its own as
  // it comes.
  let loading = false;
  const loadChosen = async () => {
    if (loading) return;
    loading = true;
    for (const offer of [...media.values()]) {
      if (previews.has(offer)) continue;
      previews.set(offer, await forma.scraper.preview(offer).catch(() => null));
      if (page.showing(build)) await page.refresh();
    }
    loading = false;
  };

  const build = (): Row[] => {
    void Promise.resolve().then(loadChosen);
    const changes = text.size + media.size + cleared.size + removed.size;
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
    const problems = offers.problems.map((label, i): Row => ({
      heading: i === 0 ? 'Could not answer' : undefined,
      label
    }));
    return [
      ...sections,
      ...problems,
      {
        // Clear of the group above it, when that is the list of problems.
        heading: problems.length > 0 ? 'Ready' : undefined,
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
                  if (cleared.size > 0) {
                    await forma.scraper.edit(
                      id,
                      Object.fromEntries([...cleared].map((field) => [field, '']))
                    );
                  }
                  if (removed.size > 0) {
                    problems.push(...(await forma.scraper.removeMedia(id, [...removed])));
                  }
                  // Done with this game: back to the list, which keeps to it if it was renamed.
                  page.close();
                  return problems[0] ?? 'Saved';
                })
      }
    ];
  };
  return build;
}
