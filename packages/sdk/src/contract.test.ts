/**
 * The TypeScript types against the real engine's JSON. `mock/library.json` is generated from
 * the Rust types, so a field renamed or added on one side fails here.
 */
import { describe, expect, it } from 'vitest';
import data from './mock/library.json' with { type: 'json' };
import type { BranchItem, Browse, Crumb, Game, Group, SystemSummary, ViewInfo } from './types.js';

/** The keys of `T`, listed by hand and checked for completeness by the compiler. */
function keys<T>() {
  return <K extends readonly (keyof T)[]>(
    ...k: K & ([keyof T] extends [K[number]] ? unknown : 'missing a key')
  ) => [...k].sort();
}

const pages = Object.values(data.browse) as unknown as Browse[];
const sorted = (o: object) => Object.keys(o).sort();

describe('contract with the engine', () => {
  it('Game', () => {
    const expected = keys<Game>()(
      'id',
      'system',
      'path',
      'name',
      'description',
      'developer',
      'publisher',
      'genre',
      'players',
      'releaseDate',
      'rating',
      'favorite',
      'hidden',
      'playCount',
      'lastPlayed',
      'gameTime',
      'raGameId',
      'media'
    );
    for (const game of data.games) expect(sorted(game)).toEqual(expected);
  });

  it('SystemSummary and ViewInfo', () => {
    const system = keys<SystemSummary>()(
      'name',
      'fullname',
      'manufacturer',
      'releaseYear',
      'hardware',
      'gameCount'
    );
    for (const s of data.systems) expect(sorted(s)).toEqual(system);
    const view = keys<ViewInfo>()('id', 'name', 'description', 'origin');
    for (const v of data.views) expect(sorted(v)).toEqual(view);
  });

  it('Browse, its listings and their items', () => {
    const kinds = new Set(pages.map((p) => p.listing.kind));
    expect(kinds).toEqual(new Set(['groups', 'branches', 'games']));
    const group = keys<Group>()('key', 'label', 'gameCount', 'imageGame');
    const branch = keys<BranchItem>()('id', 'name', 'gameCount');
    const crumb = keys<Crumb>()('key', 'label');

    for (const page of pages) {
      expect(sorted(page)).toEqual(keys<Browse>()('view', 'path', 'trail', 'listing'));
      for (const c of page.trail) expect(sorted(c)).toEqual(crumb);
      const listing = page.listing;
      if (listing.kind === 'groups') {
        expect(sorted(listing)).toEqual(['groupBy', 'items', 'kind', 'total']);
        for (const g of listing.items) expect(sorted(g)).toEqual(group);
      } else if (listing.kind === 'branches') {
        expect(sorted(listing)).toEqual(['items', 'kind']);
        for (const b of listing.items) expect(sorted(b)).toEqual(branch);
      } else {
        expect(sorted(listing)).toEqual(['items', 'kind', 'total']);
      }
    }
  });
});
