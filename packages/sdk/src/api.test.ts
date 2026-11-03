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
    const forma = await sdk();
    expect((await forma.library.systems())[0]!.name).toBe('nes');
    const [zelda] = await forma.library.search('zelda', 5);
    expect(await forma.library.game(zelda!.id)).toMatchObject({ name: 'The Legend of Zelda' });
    const changed = new Promise<LibraryChanged>((resolve) => {
      forma.events.on('library:changed', resolve);
    });
    await forma.library.setFavorite(zelda!.id, true);
    expect(await changed).toEqual({ systems: ['nes'] });
    expect(forma.library.mediaUrl(zelda!.id, 'marquee')).toMatch(/^data:image\/svg/);
  });

  it('nav', async () => {
    const forma = await sdk();
    expect((await forma.nav.views()).map((v) => v.id)).toContain('home');
    const home = await forma.nav.browse('home');
    expect(home.listing.kind).toBe('branches');
    expect(
      (await forma.nav.browse('home', ['systems'], { offset: 0, limit: 2 })).listing
    ).toMatchObject({
      kind: 'groups',
      total: 5
    });
    await forma.nav.setDefaultView('home');
    expect(await forma.nav.defaultView()).toEqual({ id: 'home', notes: [] });
    await forma.nav.setDefaultView(null);
    expect((await forma.nav.defaultView()).id).toBe('systems');
  });

  it('launch and its events', async () => {
    const forma = await sdk();
    const [mario] = await forma.library.search('mario');
    const ended = new Promise((resolve) => {
      forma.events.on('game:end', resolve);
    });
    await forma.launch.game(mario!.id, { netplay: { mode: 'host' } });
    expect(await ended).toMatchObject({ type: 'game:end', gameId: mario!.id, exitCode: 0 });
  });

  it('settings, themes, controllers and system', async () => {
    const forma = await sdk();
    await forma.settings.set('grid', 4);
    expect(await forma.settings.get<number>('grid')).toBe(4);
    await expect(forma.settings.getSystem('x')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    await expect(forma.system.quit()).rejects.toMatchObject({ code: 'permissionDenied' });
    expect((await forma.themes.list())[0]).toMatchObject({ id: 'mock', current: true });
    await forma.themes.select('mock');
    expect(await forma.input.controllers()).toEqual([]);

    const admin = await sdk(['settings.system', 'system.power']);
    await admin.settings.setSystem('x', true);
    expect(await admin.settings.getSystem('x')).toBe(true);
    await expect(admin.system.restart()).rejects.toMatchObject({ code: 'unsupported' });
    await expect(admin.system.shutdown()).rejects.toMatchObject({ code: 'unsupported' });
  });

  it('turns keys into input events while someone listens', async () => {
    const target = new EventTarget();
    vi.stubGlobal('window', target);
    const forma = await connect({ backend: mockBackend() });
    const seen: InputEvent[] = [];
    const off = forma.input.on((e) => seen.push(e));

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
    const forma = await sdk();
    const user = await forma.ra.user();
    expect(user.stale).toBe(false);
    expect(user.data.username).toBe('fixture-user');
    expect(user.data.avatar).toMatch(/^data:image\/svg/);

    const [mario] = await forma.library.search('super mario');
    const progress = await forma.ra.game(mario!.id);
    expect(progress.data.achievements.map((a) => a.title)).toEqual([
      'Mushroom Kingdom',
      'Warp Zone',
      'Princess Toadstool'
    ]);
    const [zelda] = await forma.library.search('zelda');
    await expect(forma.ra.game(zelda!.id)).rejects.toMatchObject({ code: 'notFound' });

    const lobby = await forma.netplay.lobby();
    expect(lobby.data[0]).toMatchObject({ connectable: true, localGameId: mario!.id });

    const unset = await connect({ backend: mockBackend({ ra: false }), keyboard: false });
    await expect(unset.ra.user()).rejects.toMatchObject({ code: 'notConfigured' });
  });

  it('netplay: filters rooms, finds the local network, joins and hosts', async () => {
    const backend = mockBackend({ playMs: 0 });
    const launched: unknown[] = [];
    const invoke = backend.invoke.bind(backend);
    backend.invoke = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      if (cmd === 'launch_game') launched.push(args);
      return invoke<T>(cmd, args);
    };
    const forma = await connect({ backend, keyboard: false });
    const all = (await forma.netplay.lobby()).data;
    expect(all).toHaveLength(2);
    const mine = (await forma.netplay.lobby({ owned: true, joinable: true, open: true })).data;
    expect(mine).toHaveLength(1);
    expect(mine[0]!.core).toBe('fceumm');
    expect((await forma.netplay.lobby({ coreName: 'snes9x' })).data).toHaveLength(1);
    expect((await forma.netplay.lobby({ maxPingMs: 50 })).data[0]!.pingMs).toBe(30);
    expect((await forma.netplay.lobby({ maxPingMs: 10 })).data).toHaveLength(0);

    const [lan] = await forma.netplay.lan();
    expect(lan).toMatchObject({ lan: true, join: { host: '192.168.1.50' } });

    await forma.netplay.join(mine[0]!, { password: 'pw' });
    expect(launched.at(-1)).toEqual({
      id: mine[0]!.localGameId,
      request: {
        emulator: 'libretro',
        core: 'fceumm',
        netplay: { mode: 'client', host: '192.0.2.10', port: 55435, password: 'pw' }
      }
    });
    // The relayed room: not on this box, and its core is missing.
    const relayed = all.find((r) => r.join?.session)!;
    await expect(forma.netplay.join(relayed)).rejects.toMatchObject({ code: 'notFound' });
    await expect(forma.netplay.join({ ...relayed, localGameId: 1 })).rejects.toMatchObject({
      code: 'unsupported'
    });
    await expect(
      forma.netplay.join({ ...relayed, localGameId: 1, core: 'snes9x', join: null })
    ).rejects.toMatchObject({ code: 'unsupported' });
    await new Promise((r) => setTimeout(r, 5));
    await forma.netplay.join({ ...relayed, localGameId: 1, core: 'snes9x' }, { spectate: true });
    expect(launched.at(-1)).toMatchObject({
      request: { netplay: { mode: 'client', session: 'a1b2c3d4e5', spectate: true } }
    });
    await new Promise((r) => setTimeout(r, 5));
    await forma.netplay.host(1);
    expect(launched.at(-1)).toEqual({ id: 1, request: { netplay: { mode: 'host' } } });
  });
});
