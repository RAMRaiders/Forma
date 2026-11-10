import type { FilterChoice, FilterOptions, Forma, ListFilter } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

const REGIONS: Record<string, string> = {
  us: 'USA',
  eu: 'Europe',
  jp: 'Japan',
  wor: 'World',
  br: 'Brazil',
  kr: 'Korea',
  au: 'Australia'
};

const regionName = (value: string) => REGIONS[value.toLowerCase()] ?? value.toUpperCase();
const playersName = (value: string) => `${value} or more players`;
const letterName = (value: string) => (value === '#' ? 'Numbers and symbols' : value);

/** What the filter asks for, in words; empty with none. */
export function filterSummary({ filter: f }: FilterOptions): string {
  return [
    f.name && `“${f.name}”`,
    f.letter && `Starts with ${letterName(f.letter)}`,
    f.genre,
    f.developer,
    f.publisher,
    f.region && regionName(f.region),
    f.year,
    f.players && playersName(String(f.players)),
    f.favorites && 'Favourites'
  ]
    .filter(Boolean)
    .join(' · ');
}

const games = (n: number) => (n === 1 ? '1 game' : `${n} games`);

/** Select → Filter list: narrows the game list on screen, which follows behind the menu. */
export function filterScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  const apply = async (next: ListFilter): Promise<void> => {
    const left = await forma.nav.setFilter(next);
    if (left === 0) page.notify('No games match that');
  };

  return async () => {
    const options = await forma.nav.filterOptions();
    if (!options) return [{ label: 'No game list on screen' }];
    const { filter } = options;

    const choice = <K extends keyof ListFilter>(
      label: string,
      key: K,
      choices: FilterChoice[],
      value: (raw: string) => ListFilter[K],
      show: (raw: string) => string = (raw) => raw
    ): Row[] => {
      const current = filter[key] ?? null;
      const isCurrent = (raw: string) => current !== null && String(current) === raw;
      if (choices.length === 0 && current === null) return [];
      const choose = (next: ListFilter[K] | null) => async () => {
        await apply({ ...filter, [key]: next });
        await page.pop();
      };
      return [
        {
          label,
          detail: current === null ? 'Any' : show(String(current)),
          run: () =>
            page.push(
              label,
              () => [
                { label: current === null ? '✓ Any' : 'Any', run: choose(null) },
                ...choices.map((c) => ({
                  label: `${isCurrent(c.value) ? '✓ ' : ''}${show(c.value)}`,
                  detail: games(c.count),
                  run: choose(value(c.value))
                }))
              ],
              undefined,
              choices.findIndex((c) => isCurrent(c.value)) + 1
            )
        }
      ];
    };
    const text = (raw: string) => raw;

    const rows: Row[] = [
      {
        label: 'Name',
        detail: filter.name ?? 'Any',
        run: async () => {
          const typed = await page.ask({ title: 'Name contains', value: filter.name ?? '' });
          if (typed === null) return;
          await apply({ ...filter, name: typed.trim() || null });
          await page.refresh();
        }
      },
      ...choice('Starting letter', 'letter', options.letters, text, letterName),
      ...choice('Genre', 'genre', options.genres, text),
      ...choice('Developer', 'developer', options.developers, text),
      ...choice('Publisher', 'publisher', options.publishers, text),
      ...choice('Region', 'region', options.regions, text, regionName),
      ...choice('Year', 'year', options.years, Number),
      ...choice('Players', 'players', options.players, Number, playersName),
      {
        label: 'Favourites only',
        detail: filter.favorites ? 'On' : 'Off',
        run: async () => {
          await apply({ ...filter, favorites: !filter.favorites });
          await page.refresh();
        }
      }
    ];
    if (filterSummary(options)) {
      rows.push({
        heading: `Showing ${options.total} of ${games(options.all)}`,
        label: 'Clear filter',
        run: async () => {
          await forma.nav.setFilter(null);
          await page.refresh();
        }
      });
    }
    return rows;
  };
}
