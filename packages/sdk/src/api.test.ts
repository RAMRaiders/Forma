/** Every SDK method, once, through the mock: the names and arguments the host expects. */
import { afterEach, describe, expect, it, vi } from 'vitest';
import { connect } from './client.js';
import { mockBackend } from './mock/backend.js';
import type { InputEvent, LibraryChanged } from './types.js';

afterEach(() => {
  vi.unstubAllGlobals();
});

async function sdk(permissions: string[] = []) {
  return connect({ backend: mockBackend({ permissions, playMs: 0 }), keyboard: false });
}

describe('the SDK surface', () => {
  it('library', async () => {
    const romdex = await sdk();
    expect((await romdex.library.systems())[0]!.name).toBe('nes');
    const [zelda] = await romdex.library.search('zelda', 5);
    expect(await romdex.library.game(zelda!.id)).toMatchObject({ name: 'The Legend of Zelda' });
    const changed = new Promise<LibraryChanged>((resolve) => {
      romdex.events.on('library:changed', resolve);
    });
    await romdex.library.setFavorite(zelda!.id, true);
    expect(await changed).toEqual({ systems: ['nes'] });
    expect(romdex.library.mediaUrl(zelda!.id, 'marquee')).toMatch(/^data:image\/svg/);
  });

  it('nav', async () => {
    const romdex = await sdk();
    expect((await romdex.nav.views()).map((v) => v.id)).toContain('home');
    const home = await romdex.nav.browse('home');
    expect(home.listing.kind).toBe('branches');
    expect(
      (await romdex.nav.browse('home', ['systems'], { offset: 0, limit: 2 })).listing
    ).toMatchObject({
      kind: 'groups',
      total: 5
    });
    await romdex.nav.setDefaultView('home');
    expect(await romdex.nav.defaultView()).toEqual({ id: 'home', notes: [] });
    await romdex.nav.setDefaultView(null);
    expect((await romdex.nav.defaultView()).id).toBe('systems');
  });

  it('launch and its events', async () => {
    const romdex = await sdk();
    const [mario] = await romdex.library.search('mario');
    const ended = new Promise((resolve) => {
      romdex.events.on('game:end', resolve);
    });
    await romdex.launch.game(mario!.id, { netplay: { mode: 'host' } });
    expect(await ended).toMatchObject({ type: 'game:end', gameId: mario!.id, exitCode: 0 });
  });

  it('settings, themes, controllers and system', async () => {
    const romdex = await sdk();
    await romdex.settings.set('grid', 4);
    expect(await romdex.settings.get<number>('grid')).toBe(4);
    await expect(romdex.settings.getSystem('x')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    await expect(romdex.system.quit()).rejects.toMatchObject({ code: 'permissionDenied' });
    expect((await romdex.themes.list())[0]).toMatchObject({ id: 'mock', current: true });
    await romdex.themes.select('mock');
    expect(await romdex.input.controllers()).toEqual([]);

    const admin = await sdk(['settings.system', 'system.power']);
    await admin.settings.setSystem('x', true);
    expect(await admin.settings.getSystem('x')).toBe(true);
    await expect(admin.system.restart()).rejects.toMatchObject({ code: 'unsupported' });
    await expect(admin.system.shutdown()).rejects.toMatchObject({ code: 'unsupported' });
  });

  it('turns keys into input events while someone listens', async () => {
    const target = new EventTarget();
    vi.stubGlobal('window', target);
    const romdex = await connect({ backend: mockBackend() });
    const seen: InputEvent[] = [];
    const off = romdex.input.on((e) => seen.push(e));

    const press = (key: string, type = 'keydown') => {
      const event = Object.assign(new Event(type, { cancelable: true }), { key, repeat: false });
      target.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(press('Enter')).toBe(true);
    expect(press('Enter', 'keyup')).toBe(true);
    expect(press('q')).toBe(false);
    expect(seen.map((e) => [e.action, e.pressed])).toEqual([
      ['confirm', true],
      ['confirm', false]
    ]);

    off();
    press('Enter');
    expect(seen).toHaveLength(2);
  });

  it('RetroAchievements and the netplay lobby', async () => {
    const romdex = await sdk();
    const user = await romdex.ra.user();
    expect(user.stale).toBe(false);
    expect(user.data.username).toBe('fixture-user');
    expect(user.data.avatar).toMatch(/^data:image\/svg/);

    const [mario] = await romdex.library.search('super mario');
    const progress = await romdex.ra.game(mario!.id);
    expect(progress.data.achievements.map((a) => a.title)).toEqual([
      'Mushroom Kingdom',
      'Warp Zone',
      'Princess Toadstool'
    ]);
    const [zelda] = await romdex.library.search('zelda');
    await expect(romdex.ra.game(zelda!.id)).rejects.toMatchObject({ code: 'notFound' });

    const lobby = await romdex.netplay.lobby();
    expect(lobby.data[0]).toMatchObject({ connectable: true, localGameId: mario!.id });

    const unset = await connect({ backend: mockBackend({ ra: false }), keyboard: false });
    await expect(unset.ra.user()).rejects.toMatchObject({ code: 'notConfigured' });
  });
});
