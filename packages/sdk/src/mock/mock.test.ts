import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Backend } from '../backend.js';
import type { Browse, Game, GameEnd, NetworkStatus, WifiNetwork } from '../types.js';
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

  it('network settings: permission, scan, join as Batocera does with a cable in', async () => {
    await expect(mockBackend().invoke('network_status')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ permissions: ['system.network'] });
    const status = await b.invoke<NetworkStatus>('network_status');
    expect(status.ethernet?.connected).toBe(true);
    expect(status.wifi?.saved).toBe('Home');

    const found = await b.invoke<WifiNetwork[]>('network_wifi_scan');
    expect(found.map((n) => n.ssid)).toEqual(['Home', 'Cafe Guest', 'Office 5G', 'Campus']);
    await expect(
      b.invoke('network_wifi_join', { ssid: 'Office 5G', passphrase: 'short' })
    ).rejects.toMatchObject({ code: 'invalid' });
    await expect(
      b.invoke('network_wifi_join', { ssid: 'Office 5G', passphrase: 'wrong-password' })
    ).rejects.toMatchObject({ code: 'network' });
    await expect(b.invoke('network_wifi_join', { ssid: 'Campus' })).rejects.toMatchObject({
      code: 'unsupported'
    });
    expect(await b.invoke('network_wifi_join', { ssid: 'Cafe Guest' })).toBe('saved');
    expect((await b.invoke<NetworkStatus>('network_status')).wifi?.saved).toBe('Cafe Guest');

    await b.invoke('network_wifi_set_enabled', { enabled: false });
    expect(await b.invoke('network_wifi_scan')).toEqual([]);

    const manual = {
      method: 'manual',
      address: '192.168.1.50',
      netmask: '255.255.255.0',
      gateway: '192.168.1.254',
      nameservers: ['1.1.1.1']
    };
    await b.invoke('network_ethernet_configure', { config: manual });
    const after = await b.invoke<NetworkStatus>('network_status');
    expect(after.ethernet?.config).toEqual(manual);
    expect(after.ethernet?.ipv4?.address).toBe('192.168.1.50');
    await expect(
      b.invoke('network_ethernet_configure', { config: { ...manual, address: 'nope' } })
    ).rejects.toMatchObject({ code: 'invalid' });
  });

  it("Batocera's settings and the RetroAchievements login", async () => {
    await expect(mockBackend().invoke('batocera_settings', { prefix: '' })).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ permissions: ['settings.batocera'] });
    const netplay = await b.invoke<{ key: string }[]>('batocera_settings', {
      prefix: 'global.netplay'
    });
    expect(netplay.every((s) => s.key.startsWith('global.netplay'))).toBe(true);
    await b.invoke('batocera_set', { key: 'global.netplay.relay', value: 'nyc' });
    await expect(
      b.invoke('batocera_set', { key: 'global.netplay.relay', value: 'tokyo' })
    ).rejects.toMatchObject({ code: 'invalid' });
    await expect(
      b.invoke('ra_login', { username: 'me', password: 'wrong-password' })
    ).rejects.toMatchObject({ code: 'invalid' });
    expect(await b.invoke('ra_login', { username: ' me ', password: 'right' })).toBe('me');
    const ra = await b.invoke<{ key: string; value: string | null; set: boolean }[]>(
      'batocera_settings',
      { prefix: 'global.retroachievements' }
    );
    const token = ra.find((s) => s.key === 'global.retroachievements.token')!;
    expect([token.value, token.set]).toEqual([null, true]);
  });

  it('rejects unknown commands instead of answering null', async () => {
    await expect(mockBackend().invoke('rm_rf')).rejects.toMatchObject({ code: 'notFound' });
  });
});
