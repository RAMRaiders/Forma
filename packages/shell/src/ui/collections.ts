import type { Forma } from '@ramraiders/forma-sdk';

import { type Page, type Row } from './page.js';

/** A second Y within this long deletes: a stray press must not lose a list. */
const CONFIRM_MS = 3000;

/**
 * The user's collections. With a game selected, each row puts it in or takes it out; without,
 * each says how many games it holds. Y deletes one (pressed twice); the last row makes a new one,
 * with the selected game already in it.
 */
export function collectionsScreen(
  forma: Forma,
  page: Page,
  notify: (message: string) => void,
  game: { id: number; name: string } | null
): () => Promise<Row[]> {
  let confirming: { name: string; until: number } | null = null;
  const remove = (name: string) => async () => {
    const now = Date.now();
    if (confirming?.name !== name || now > confirming.until) {
      confirming = { name, until: now + CONFIRM_MS };
      notify(`Press Y again to delete ${name}`);
      return;
    }
    confirming = null;
    await page.run('Deleting…', async () => {
      await forma.collections.delete(name);
      return `Deleted ${name}; its games stay in the library`;
    });
  };
  return async () => {
    const [all, mine] = await Promise.all([
      forma.collections.list(),
      game ? forma.collections.of(game.id) : Promise.resolve([] as string[])
    ]);
    const count = (n: number) => (n === 1 ? '1 game' : `${n} games`);
    const rows: Row[] = all.map((c) => {
      const del = { label: 'Delete', run: remove(c.name) };
      if (!game) return { label: c.name, detail: count(c.games), alt: del };
      const inIt = mine.includes(c.name);
      const flip = () =>
        page.run('Saving…', async () => {
          await (inIt
            ? forma.collections.remove(c.name, game.id)
            : forma.collections.add(c.name, game.id));
        });
      return {
        label: `${c.name}: ${inIt ? 'In' : 'Not in'}`,
        detail: `${count(c.games)}. Y: delete the collection`,
        run: flip,
        adjust: flip,
        alt: del
      };
    });
    rows.push({
      label: 'New collection…',
      detail: game ? `Starts with ${game.name}` : 'An empty list to fill from a game’s menu',
      run: async () => {
        const name = await page.ask({ title: 'Name of the collection', value: '' });
        if (!name?.trim()) return;
        await page.run('Saving…', async () => {
          const made = await forma.collections.create(name);
          if (game) await forma.collections.add(made, game.id);
          return game ? `Added ${game.name} to ${made}` : `Made ${made}`;
        });
      }
    });
    return rows;
  };
}
