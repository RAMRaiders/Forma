/**
 * The TypeScript types against the real engine's JSON. `mock/library.json` is generated from
 * the Rust types, so a field renamed or added on one side fails here.
 */
import { describe, expect, it } from 'vitest';
import data from './mock/library.json' with { type: 'json' };
import type {
  BranchItem,
  Browse,
  Crumb,
  Game,
  Group,
  SystemSummary,
  ViewInfo,
  EthernetStatus,
  Ipv4,
  NetplayRoom,
  NetworkStatus,
  RaAchievement,
  RaGame,
  RaRecent,
  RaUser,
  BatoceraSetting,
  OptionItem,
  OptionsPage,
  InfoItem,
  SystemSetting,
  MusicSource,
  Radio,
  WifiNetwork,
  WifiStatus
} from './types.js';

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

  it('RetroAchievements and the netplay lobby', () => {
    const user = keys<RaUser>()(
      'username',
      'points',
      'softcorePoints',
      'truePoints',
      'rank',
      'memberSince',
      'richPresence',
      'avatar',
      'recentlyPlayed'
    );
    expect(sorted(data.ra.user)).toEqual(user);
    const recent = keys<RaRecent>()('raGameId', 'title', 'console', 'icon', 'lastPlayed');
    for (const r of data.ra.user.recentlyPlayed) expect(sorted(r)).toEqual(recent);

    const game = keys<RaGame>()(
      'raGameId',
      'title',
      'console',
      'icon',
      'achievementCount',
      'earned',
      'earnedHardcore',
      'completion',
      'achievements'
    );
    const achievement = keys<RaAchievement>()(
      'id',
      'title',
      'description',
      'points',
      'badge',
      'earnedAt',
      'earnedHardcoreAt'
    );
    for (const g of Object.values(data.ra.games)) {
      expect(sorted(g)).toEqual(game);
      for (const a of g.achievements) expect(sorted(a)).toEqual(achievement);
    }

    const room = keys<NetplayRoom>()(
      'id',
      'username',
      'country',
      'gameName',
      'gameCrc',
      'coreName',
      'coreVersion',
      'retroarchVersion',
      'frontend',
      'host',
      'port',
      'relayHost',
      'relayPort',
      'hasPassword',
      'hasSpectatePassword',
      'connectable',
      'created',
      'localGameId'
    );
    for (const r of data.lobby) expect(sorted(r)).toEqual(room);
  });

  it('the network', () => {
    const { status, wifi } = data.network;
    expect(sorted(status)).toEqual(keys<NetworkStatus>()('ethernet', 'wifi'));
    expect(sorted(status.ethernet)).toEqual(
      keys<EthernetStatus>()('connected', 'ipv4', 'nameservers', 'config')
    );
    expect(sorted(status.ethernet.ipv4)).toEqual(keys<Ipv4>()('address', 'netmask', 'gateway'));
    expect(status.ethernet.config).toEqual({ method: 'dhcp' });
    expect(sorted(status.wifi)).toEqual(
      keys<WifiStatus>()('enabled', 'connected', 'ipv4', 'saved')
    );
    const network = keys<WifiNetwork>()('ssid', 'strength', 'security', 'connected', 'saved');
    for (const n of wifi) expect(sorted(n)).toEqual(network);
    expect(new Set(wifi.map((n) => n.security))).toEqual(new Set(['psk', 'open', 'enterprise']));
  });

  it("Batocera's settings, with secrets never read back", () => {
    const setting = keys<BatoceraSetting>()('key', 'value', 'kind', 'set', 'choices');
    for (const s of data.batocera) expect(sorted(s)).toEqual(setting);
    const password = data.batocera.find((s) => s.key === 'global.retroachievements.password')!;
    expect(password.value).toBeNull();
    expect(data.batocera.find((s) => s.key === 'global.netplay.relay')!.choices).toContain('nyc');
  });

  it('emulator options', () => {
    const pages = Object.values(data.options) as unknown as OptionsPage[];
    const page = keys<OptionsPage>()('options', 'flags');
    const item = keys<OptionItem>()(
      'key',
      'label',
      'group',
      'submenu',
      'description',
      'kind',
      'choices',
      'min',
      'max',
      'step',
      'default',
      'value',
      'inherited'
    );
    for (const p of pages) {
      expect(sorted(p)).toEqual(page);
      for (const o of p.options) expect(sorted(o)).toEqual(item);
    }
    const kinds = new Set(pages.flatMap((p) => p.options.map((o) => o.kind)));
    expect(kinds).toEqual(new Set(['choice', 'number', 'text']));
  });

  it('system settings and information', () => {
    const info = keys<InfoItem>()('label', 'value');
    for (const i of data.system.info) expect(sorted(i)).toEqual(info);
    const setting = keys<SystemSetting>()(
      'key',
      'label',
      'section',
      'description',
      'kind',
      'choices',
      'value',
      'applies'
    );
    for (const s of data.system.settings) expect(sorted(s)).toEqual(setting);
    expect(new Set(data.system.settings.map((s) => s.applies))).toEqual(
      new Set(['now', 'restart', 'reboot'])
    );
  });

  it('the music folder', () => {
    const source = keys<MusicSource>()('id', 'kind', 'name', 'tracks');
    for (const s of data.music.sources) expect(sorted(s)).toEqual(source);
    expect(data.music.sources.map((s) => [s.id, s.tracks])).toEqual([
      ['folder:', 1],
      ['folder:Chiptune', 2],
      ['radio:https://ice1.somafm.com/groovesalad-128-mp3', 0]
    ]);
    for (const r of data.music.radios) expect(sorted(r)).toEqual(keys<Radio>()('name', 'url'));
  });
});
