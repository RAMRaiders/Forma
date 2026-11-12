import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Backend } from '../backend.js';
import type {
  BluetoothDevice,
  RaSession,
  Browse,
  Game,
  GameEnd,
  NetworkStatus,
  AddonActionResult,
  AddonInfo,
  AddonPage,
  CatalogueAddon,
  Fetched,
  ScraperSettings,
  ScraperStatus,
  WifiNetwork
} from '../types.js';
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

  it('after a game RetroAchievements knows, says what the session unlocked', async () => {
    vi.useFakeTimers();
    const b = mockBackend({ playMs: 1000 });
    const sessions: RaSession[] = [];
    b.listen<RaSession>('ra:unlocked', (e) => sessions.push(e));
    await b.invoke('launch_game', { id: 2 });
    vi.advanceTimersByTime(1000);
    expect(sessions).toHaveLength(1);
    expect(sessions[0]!.unlocked.map((a) => a.title)).toEqual(['Princess Toadstool']);
    expect(sessions[0]!.game.earned).toBe(3);
    expect(sessions[0]!.points).toBe(sessions[0]!.unlocked[0]!.points);
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

  it('content downloader: install with progress, one at a time, then remove', async () => {
    await expect(mockBackend().invoke('store_list')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ playMs: 20, permissions: ['store.manage'] });
    type Pkg = { name: string; installed: boolean };
    const name = 'atari2600-amoeba-jump';
    const seen: string[] = [];
    b.listen<{ stage: string; percent: number }>('store:progress', (p) =>
      seen.push(`${p.stage} ${p.percent}`)
    );
    const installing = b.invoke('store_install', { name });
    await expect(b.invoke('store_remove', { name })).rejects.toMatchObject({
      code: 'unsupported'
    });
    await installing;
    expect(seen).toEqual(['download 0', 'download 50', 'download 100', 'install 50']);
    const installed = (await b.invoke<Pkg[]>('store_list')).find((p) => p.name === name)!;
    expect(installed.installed).toBe(true);
    await b.invoke('store_remove', { name });
    expect((await b.invoke<Pkg[]>('store_list')).find((p) => p.name === name)!.installed).toBe(
      false
    );
    await expect(b.invoke('store_install', { name: '--all' })).rejects.toMatchObject({
      code: 'invalid'
    });
  });

  it('Batocera updates: checks, and installs only when asked', async () => {
    await expect(mockBackend().invoke('updates_check')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ permissions: ['system.update'] });
    type Check = { current: string; latest: string; available: boolean };
    expect(await b.invoke<Check>('updates_check')).toMatchObject({
      current: '43.1',
      available: true
    });
    const seen: string[] = [];
    b.listen<{ message: string; percent: number | null }>('updates:progress', (p) =>
      seen.push(`${p.message} ${p.percent ?? '-'}`)
    );
    await b.invoke('updates_install');
    expect(seen).toEqual([
      'Downloading 0',
      'Downloading 50',
      'Downloading 100',
      'Extracting files -'
    ]);
    expect(await b.invoke<Check>('updates_check')).toMatchObject({
      current: '44',
      available: false
    });
    await expect(b.invoke('updates_install')).rejects.toMatchObject({ code: 'unsupported' });
  });

  it('the volume is a number from 0 to 100', async () => {
    const b = mockBackend({ permissions: ['system.settings'] });
    type Setting = { key: string; kind: string; value: string | null };
    await b.invoke('system_settings_set', { key: 'audio.volume', value: '75' });
    const all = await b.invoke<Setting[]>('system_settings');
    expect(all.find((s) => s.key === 'audio.volume')).toMatchObject({
      kind: 'number',
      value: '75'
    });
    for (const value of ['101', 'loud']) {
      await expect(
        b.invoke('system_settings_set', { key: 'audio.volume', value })
      ).rejects.toMatchObject({ code: 'invalid' });
    }
  });

  it('system settings behind system.settings, checked against their choices', async () => {
    await expect(mockBackend().invoke('system_settings')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ permissions: ['system.settings'] });
    const info = await b.invoke<{ label: string }[]>('system_info');
    expect(info[0]!.label).toBe('Batocera');
    await b.invoke('system_settings_set', { key: 'system.timezone', value: 'Europe/Paris' });
    await b.invoke('system_settings_set', { key: 'system.hostname', value: 'retro-box' });
    const all = await b.invoke<{ key: string; value: string | null }[]>('system_settings');
    expect(all.find((s) => s.key === 'system.timezone')!.value).toBe('Europe/Paris');
    await expect(
      b.invoke('system_settings_set', { key: 'system.timezone', value: 'Mars/Olympus' })
    ).rejects.toMatchObject({ code: 'invalid' });
    await expect(
      b.invoke('system_settings_set', { key: 'system.hostname', value: 'my box' })
    ).rejects.toMatchObject({ code: 'invalid' });
  });

  it('Bluetooth: pairs, connects and forgets pads, with the permission', async () => {
    await expect(mockBackend().invoke('bluetooth_status')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ permissions: ['system.controllers'] });
    type Status = { enabled: boolean; devices: BluetoothDevice[] };
    const said: string[] = [];
    b.listen<{ message: string }>('bluetooth:pairing', (e) => said.push(e.message));
    expect((await b.invoke<Status>('bluetooth_status')).devices).toHaveLength(2);
    const paired = await b.invoke<BluetoothDevice[]>('bluetooth_pair');
    expect(paired.map((d) => d.name)).toEqual(['Wireless Controller']);
    expect(said[0]).toBe('Discovering…');
    await b.invoke('bluetooth_disconnect', { address: paired[0]!.address });
    await b.invoke('bluetooth_forget', { address: '00:06:F5:13:C3:50' });
    const { devices } = await b.invoke<Status>('bluetooth_status');
    expect(devices.map((d) => [d.name, d.connected])).toEqual([
      ['8BitDo Pro 2', true],
      ['Wireless Controller', false]
    ]);
    await b.invoke('bluetooth_set_enabled', { enabled: false });
    expect(await b.invoke('bluetooth_status')).toEqual({ enabled: false, devices: [] });
    await expect(b.invoke('bluetooth_pair')).rejects.toMatchObject({ code: 'unsupported' });
    await expect(b.invoke('bluetooth_connect', { address: 'nope' })).rejects.toMatchObject({
      code: 'invalid'
    });
  });

  it('the CRT shader on or off per system and game, over global', async () => {
    const b = mockBackend({ permissions: ['settings.emulators'] });
    type Page = {
      enabled: boolean;
      switch: { value: boolean | null; inherited: { from: string } };
    };
    const get = (scope: unknown) => b.invoke<Page>('shaders_get', { scope });
    const snes = { level: 'system', system: 'snes' };
    expect((await get(snes)).switch).toEqual({
      value: null,
      inherited: { value: false, from: 'global' }
    });
    await b.invoke('shaders_set_enabled', { enabled: true, scope: snes });
    expect((await get(snes)).enabled).toBe(true);
    expect((await get({ level: 'global' })).enabled).toBe(false);
    await b.invoke('shaders_set_enabled', { enabled: null, scope: snes });
    expect((await get(snes)).enabled).toBe(false);
    await expect(b.invoke('shaders_set_enabled', { enabled: null })).rejects.toMatchObject({
      code: 'invalid'
    });
  });

  it('emulator options: levels, inheritance, the emulator choosing the options', async () => {
    const global = { level: 'global' } as const;
    await expect(mockBackend().invoke('options_get', { scope: global })).rejects.toMatchObject({
      code: 'permissionDenied'
    });
    const b = mockBackend({ permissions: ['settings.emulators'] });
    type Page = { options: { key: string; value: string | null; inherited: unknown }[] };
    const get = (scope: unknown) => b.invoke<Page>('options_get', { scope });
    const set = (scope: unknown, key: string, value: string | null) =>
      b.invoke('options_set', { scope, key, value });
    const item = (page: Page, key: string) => page.options.find((o) => o.key === key)!;

    // A core alone: the default emulator's core, with its own options.
    const nes = { level: 'system', system: 'nes' } as const;
    expect((await get(nes)).options.map((o) => o.key)).toContain('fceumm_palette');
    await set(nes, 'core', 'nestopia');
    expect((await get(nes)).options.map((o) => o.key)).not.toContain('fceumm_palette');

    const psx = { level: 'system', system: 'psx' } as const;
    expect((await get(psx)).options.map((o) => o.key)).toContain('swanstation_pgxp');
    await set(psx, 'emulator', 'duckstation');
    expect((await get(psx)).options.map((o) => o.key)).toContain('duckstation_gfxbackend');
    await expect(set(psx, 'swanstation_pgxp', '1')).rejects.toMatchObject({ code: 'invalid' });

    await set(global, 'ratio', '4/3');
    const game = { level: 'game', game: 1 } as const;
    const ratio = item(await get(game), 'ratio');
    expect([ratio.value, ratio.inherited]).toEqual([null, { value: '4/3', from: 'global' }]);
    await set(game, 'ratio', '16/9');
    expect(item(await get(game), 'ratio').value).toBe('16/9');
    await set(game, 'ratio', null);
    expect(item(await get(game), 'ratio').value).toBeNull();
    await expect(set(global, 'ratio', '5/4')).rejects.toMatchObject({ code: 'invalid' });
    await expect(set(global, 'rumble_gain', '500')).rejects.toMatchObject({ code: 'invalid' });
  });

  it('music: sources, a playing status, events, and radios behind music.manage', async () => {
    const b = mockBackend({ permissions: ['music.manage'] });
    const heard: string[] = [];
    b.listen<{ state: string }>('music:changed', (s) => heard.push(s.state));
    const sources = await b.invoke<{ id: string }[]>('music_sources');
    expect(sources.map((s) => s.id)).toContain('folder:Chiptune');
    const playing = await b.invoke<{ state: string; track: number; tracks: number }>('music_play', {
      source: 'folder:Chiptune',
      options: { shuffle: true }
    });
    expect([playing.state, playing.track, playing.tracks]).toEqual(['playing', 1, 2]);
    await b.invoke('music_toggle');
    await b.invoke('music_set_volume', { volume: 250 });
    expect(heard).toEqual(['playing', 'paused', 'paused']);
    expect((await b.invoke<{ volume: number }>('music_status')).volume).toBe(100);
    await b.invoke('music_add_radio', { name: 'Lofi', url: 'https://lofi.example/live' });
    expect((await b.invoke<{ id: string }[]>('music_sources')).map((s) => s.id)).toContain(
      'radio:https://lofi.example/live'
    );
    await expect(
      mockBackend().invoke('music_add_radio', { name: 'x', url: 'https://x' })
    ).rejects.toMatchObject({ code: 'permissionDenied' });
  });

  it('prepares media ahead, which the mock has nothing to do for', async () => {
    const b = mockBackend();
    expect(
      await b.invoke('library_prepare_media', { items: [{ gameId: 1, kind: 'image', width: 320 }] })
    ).toBeNull();
    expect(
      await b.invoke('library_prepare_list', {
        view: 'systems',
        path: ['nes'],
        around: 0,
        pictures: []
      })
    ).toBeNull();
  });

  it('video: heard unless the user turned video sound off', async () => {
    const b = mockBackend({ permissions: ['settings.system'] });
    expect(await b.invoke('video_play', { gameId: 1 })).toEqual({ playing: false, sound: true });
    await b.invoke('settings_set', { key: 'video.sound', value: false, scope: 'system' });
    expect(await b.invoke('video_play', { gameId: 1 })).toEqual({ playing: false, sound: false });
    expect(await b.invoke('video_stop')).toBeNull();
  });

  it('rejects unknown commands instead of answering null', async () => {
    await expect(mockBackend().invoke('rm_rf')).rejects.toMatchObject({ code: 'notFound' });
  });

  it('scraper: sources in order, credentials kept secret, a run with progress', async () => {
    const b = mockBackend({ permissions: ['scraper.manage'] });
    const settings = await b.invoke<ScraperSettings>('scraper_settings');
    expect(settings.sources.map((s) => s.id)).toEqual([
      'screenscraper',
      'thegamesdb',
      'igdb',
      'mobygames',
      'steamgriddb',
      'arcadedb',
      'libretro',
      'addon:example-boxes'
    ]);
    expect(settings.sources.filter((s) => s.ready).map((s) => s.id)).toEqual([
      'arcadedb',
      'libretro',
      'addon:example-boxes'
    ]);
    const after = await b.invoke<ScraperSettings>('scraper_set_credential', {
      source: 'thegamesdb',
      key: 'apiKey',
      value: 'hush'
    });
    const tgdb = after.sources.find((s) => s.id === 'thegamesdb')!;
    expect(tgdb.ready).toBe(true);
    expect(tgdb.needs[0]).toMatchObject({ set: true, value: null });
    const moved = await b.invoke<ScraperSettings>('scraper_configure', {
      sources: [{ id: 'libretro', enabled: true }],
      media: ['video', 'thumbnail'],
      replace: true
    });
    expect(moved.sources[0]!.id).toBe('libretro');
    expect(moved.sources).toHaveLength(8);
    expect(moved.media).toEqual(['thumbnail', 'video']);
    expect(moved.theme).toEqual({
      name: 'Neon',
      view: null,
      media: ['boxside'],
      fields: ['ageRating'],
      shows: [],
      backups: {}
    });
    const off = await b.invoke<ScraperSettings>('scraper_configure', { themeNeeds: false });
    expect(off.themeNeeds).toBe(false);

    const seen: ScraperStatus[] = [];
    b.listen('scraper:progress', (s) => seen.push(s as ScraperStatus));
    const started = await b.invoke<ScraperStatus>('scraper_start', {
      scope: { kind: 'system', name: 'nes' }
    });
    expect(started).toMatchObject({ running: true, done: 0 });
    await expect(b.invoke('scraper_start', { scope: { kind: 'all' } })).rejects.toMatchObject({
      code: 'busy'
    });
    await vi.waitFor(() => expect(seen.at(-1)?.running).toBe(false));
    const done = await b.invoke<ScraperStatus>('scraper_status');
    expect(done.done).toBe(started.total);
    expect(done.found + done.notFound).toBe(started.total);

    await expect(mockBackend().invoke('scraper_settings')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
  });

  it('add-ons: list, catalogue, install, turn off (leaving the scraper), remove', async () => {
    const b = mockBackend({ permissions: ['addons.manage', 'scraper.manage'] });
    const listed = await b.invoke<AddonInfo[]>('addons_list');
    expect(listed.map((a) => [a.id, a.update])).toEqual([
      ['example-boxes', '1.1.0'],
      ['web-radios', null]
    ]);
    const cat = await b.invoke<Fetched<{ addons: CatalogueAddon[] }>>('addons_catalogue');
    expect(cat.data.addons.map((a) => [a.id, a.installed, a.update])).toEqual([
      ['example-boxes', '1.0.0', true],
      ['arcade-flyers', null, false],
      ['web-radios', '1.0.0', false],
      ['fm-tuner', null, false]
    ]);
    const added = await b.invoke<AddonInfo>('addons_install', { id: 'arcade-flyers' });
    expect(added.scraperSource).toBe('addon:arcade-flyers');
    const sources = async () =>
      (await b.invoke<ScraperSettings>('scraper_settings')).sources.map((s) => s.id);
    expect(await sources()).toContain('addon:arcade-flyers');
    await b.invoke('addons_set_enabled', { id: 'arcade-flyers', enabled: false });
    expect(await sources()).not.toContain('addon:arcade-flyers');
    await b.invoke('addons_remove', { id: 'arcade-flyers' });
    expect((await b.invoke<AddonInfo[]>('addons_list')).map((a) => a.id)).toEqual([
      'example-boxes',
      'web-radios'
    ]);
    await expect(mockBackend().invoke('addons_list')).rejects.toMatchObject({
      code: 'permissionDenied'
    });
  });

  it('add-ons: radios in music, settings, menu pages and actions, a system part', async () => {
    const b = mockBackend({ permissions: ['addons.manage', 'music.manage'] });
    const radios = async () =>
      (await b.invoke<{ id: string; name: string }[]>('music_sources'))
        .filter((s) => s.id.startsWith('addon:web-radios:'))
        .map((s) => s.name);
    expect(await radios()).toEqual(['Chip FM', 'Arcade Lounge']);
    const after = await b.invoke<AddonInfo[]>('addons_set_setting', {
      id: 'web-radios',
      key: 'station',
      value: 'https://mine.example/live'
    });
    expect(after[1]!.settings[0]).toMatchObject({ set: true, value: 'https://mine.example/live' });
    expect(await radios()).toEqual(['Chip FM', 'Arcade Lounge', 'Your station']);
    await expect(
      b.invoke('addons_set_setting', { id: 'web-radios', key: 'nope', value: 'x' })
    ).rejects.toMatchObject({ code: 'invalid' });

    const page = await b.invoke<AddonPage>('addons_page', { id: 'web-radios', path: [] });
    expect(page.rows[0]).toMatchObject({ heading: 'Stations', label: 'Chip FM' });
    expect(page.rows.at(-1)!.open).toEqual(['about']);
    const about = await b.invoke<AddonPage>('addons_page', { id: 'web-radios', path: ['about'] });
    expect(about.title).toBe('About');
    await expect(b.invoke('addons_page', { id: 'example-boxes', path: [] })).rejects.toBeTruthy();

    const tuner = await b.invoke<AddonInfo>('addons_install', { id: 'fm-tuner' });
    expect([tuner.system, tuner.verified, tuner.menu?.label]).toEqual([true, true, 'FM tuner']);
    const done = await b.invoke<AddonActionResult>('addons_action', {
      id: 'fm-tuner',
      action: 'tune',
      input: '99.1'
    });
    expect(done.notify).toBe('Tuned to 99.1');
    const tuned = await b.invoke<AddonPage>('addons_page', { id: 'fm-tuner', path: [] });
    expect(tuned.rows[0]!.detail).toBe('99.1');

    await b.invoke('addons_set_enabled', { id: 'web-radios', enabled: false });
    expect(await radios()).toEqual([]);
    await expect(b.invoke('addons_page', { id: 'web-radios', path: [] })).rejects.toMatchObject({
      code: 'invalid'
    });
  });
});
