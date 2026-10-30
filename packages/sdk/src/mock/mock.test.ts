import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Backend } from '../backend.js';
import type { Browse, Game, GameEnd } from '../types.js';
import { mockBackend } from './backend.js';

const browse = (b: Backend, view: string, path: string[] = [], page?: object) =>
  b.invoke<Browse>('nav_browse', { view, path, page });

afterEach(() => {
  vi.useRealTimers();
});

describe('mock backend', () => {
  it('serves the real engine output for the fixture library', async () => {
    const b = mockBackend();
    const root = await browse(b, 'systems');
    expect(root.listing.kind).toBe('groups');
    if (root.listing.kind !== 'groups') return;
    expect(root.listing.items.map((g) => g.key)).toEqual([
      'nes',
      'snes',
      'megadrive',
      'psx',
      'homebrew'
    ]);

    const nes = await browse(b, 'systems', ['nes']);
    expect(nes.trail).toEqual([{ key: 'nes', label: 'Nintendo Entertainment System' }]);
  });

  it('pages games and keeps the total', async () => {
    const b = mockBackend();
    const page = await browse(b, 'systems', ['nes'], { offset: 1, limit: 1 });
    if (page.listing.kind !== 'games') throw new Error('expected games');
    expect(page.listing.total).toBe(3);
    expect(page.listing.items.map((g) => g.name)).toEqual(['Super Mario Bros.']);
    expect(page.listing.items[0]!.media.image).toMatch(/^data:image\/svg\+xml/);
    expect(page.listing.items[0]!.media.video).toBeUndefined();
  });

  it('reports unknown views and paths with codes', async () => {
    const b = mockBackend();
    await expect(browse(b, 'nope')).rejects.toMatchObject({ code: 'notFound' });
    await expect(browse(b, 'systems', ['nes', 'x'])).rejects.toMatchObject({ code: 'invalid' });
  });

  it('searches by word prefix, ignoring case and accents, and skips hidden games', async () => {
    const b = mockBackend();
    const names = async (query: string) =>
      (await b.invoke<Game[]>('library_search', { query })).map((g) => g.name);
    expect(await names('super mar')).toEqual(['Super Mario Bros.']);
    expect(await names('CAFE')).toEqual(['Super Mario Bros.']);
    expect(await names('tom')).toEqual([]);
    expect(await names('   ')).toEqual([]);
  });

  it('keeps favourites and settings in memory, with the same permission rules', async () => {
    const b = mockBackend();
    const zelda = (await b.invoke<Game[]>('library_search', { query: 'zelda' }))[0]!;
    const changed = vi.fn();
    b.listen('library:changed', changed);
    await b.invoke('library_set_favorite', { id: zelda.id, favorite: true });
    expect((await b.invoke<Game>('library_game', { id: zelda.id })).favorite).toBe(true);
    expect(changed).toHaveBeenCalledWith({ systems: ['nes'] });

    await b.invoke('settings_set', { key: 'layout', value: { grid: 4 } });
    expect(await b.invoke('settings_get', { key: 'layout' })).toEqual({ grid: 4 });
    await expect(
      b.invoke('settings_set', { key: 'x', value: 1, scope: 'system' })
    ).rejects.toMatchObject({ code: 'permissionDenied' });
    const admin = mockBackend({ permissions: ['settings.system', 'system.power'] });
    await admin.invoke('settings_set', { key: 'x', value: 1, scope: 'system' });
    await expect(admin.invoke('system_quit')).rejects.toMatchObject({ code: 'unsupported' });

    await expect(b.invoke('nav_set_default_view', { id: 'nope' })).rejects.toMatchObject({
      code: 'notFound'
    });
    await b.invoke('nav_set_default_view', { id: 'genre' });
    expect(await b.invoke('nav_default_view')).toEqual({ id: 'genre', notes: [] });
  });

  it('plays a game: start, end after playMs, stats updated, busy meanwhile', async () => {
    vi.useFakeTimers();
    const b = mockBackend({ playMs: 3000 });
    const zelda = (await b.invoke<Game[]>('library_search', { query: 'zelda' }))[0]!;
    const events: string[] = [];
    b.listen('game:start', () => events.push('start'));
    b.listen<GameEnd>('game:end', (e) => events.push(`end:${e.seconds}`));

    await b.invoke('launch_game', { id: zelda.id });
    await expect(b.invoke('launch_game', { id: zelda.id })).rejects.toMatchObject({ code: 'busy' });
    expect(events).toEqual(['start']);
    vi.advanceTimersByTime(3000);
    expect(events).toEqual(['start', 'end:3']);
    const after = await b.invoke<Game>('library_game', { id: zelda.id });
    expect(after.playCount).toBe(1);
    expect(after.gameTime).toBe(3);
  });

  it('rejects unknown commands instead of answering null', async () => {
    await expect(mockBackend().invoke('rm_rf')).rejects.toMatchObject({ code: 'notFound' });
  });
});
