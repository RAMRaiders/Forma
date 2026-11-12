import type {
  BatoceraSetting,
  Forma,
  Game,
  MusicSource,
  MusicStatus,
  SystemsOrder,
  ThemeInfo
} from '@ramraiders/forma-sdk';

import { filterScreen, filterSummary } from './filter.js';
import { mediaToShow } from './reader.js';
import { deleteGameRow, gameRows } from './gamedata.js';
import { optionsScreen } from './settings.js';
import { playRows } from './netplay.js';
import { type Page, type Row, onOff } from './page.js';
import { catalogueScreen, themeScreen } from './themes.js';

/** Music: what plays, the controls, and everything there is to play. */
/** Forma's setting for how long a new song is shown, in seconds (0: not at all). */
const SONG_SECONDS_SETTING = 'music.songSeconds';
/** Forma's setting: games' videos play with their sound, the music lowered meanwhile. */
const VIDEO_SOUND_SETTING = 'video.sound';
const SONG_SECONDS_DEFAULT = 8;
/** The lengths the menu steps through. */
const SONG_SECONDS = [0, 3, 5, 8, 10, 15, 20, 30];

export function musicScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  let shuffle: boolean | null = null;
  const act = (label: string, work: () => Promise<unknown>) =>
    page.run(label, async () => {
      await work();
    });

  return async () => {
    const [status, sources, shown, videoSound] = await Promise.all([
      forma.music.status(),
      forma.music.sources(),
      forma.settings.getSystem<number>(SONG_SECONDS_SETTING).catch(() => null),
      forma.settings.getSystem<boolean>(VIDEO_SOUND_SETTING).catch(() => null)
    ]);
    const heard = videoSound !== false;
    const toggleVideoSound = () =>
      page.run('…', () => forma.settings.setSystem(VIDEO_SOUND_SETTING, !heard));
    const seconds = typeof shown === 'number' ? shown : SONG_SECONDS_DEFAULT;
    const showFor = (step: -1 | 1) => {
      const at = SONG_SECONDS.indexOf(seconds);
      const next =
        at === -1
          ? SONG_SECONDS_DEFAULT
          : SONG_SECONDS[Math.max(0, Math.min(SONG_SECONDS.length - 1, at + step))]!;
      return page.run('…', () => forma.settings.setSystem(SONG_SECONDS_SETTING, next));
    };
    shuffle ??= status.shuffle;
    const rows: Row[] = [
      {
        label: 'Video sound',
        detail: heard
          ? 'On: the music goes quiet while a game’s video plays'
          : 'Off: videos play muted',
        adjust: toggleVideoSound,
        run: toggleVideoSound
      },
      { label: nowPlaying(status), detail: playingDetail(status) }
    ];
    if (status.state !== 'stopped') {
      rows.push(
        {
          label: status.state === 'playing' ? 'Pause' : 'Resume',
          run: () => act('…', () => forma.music.toggle())
        },
        ...(status.tracks !== null
          ? [
              { label: 'Next', run: () => act('…', () => forma.music.next()) },
              { label: 'Previous', run: () => act('…', () => forma.music.previous()) }
            ]
          : []),
        {
          label: 'Stop',
          detail: 'Nothing plays at the next start either',
          run: () => act('Stopping…', () => forma.music.stop())
        }
      );
    }
    rows.push(
      {
        label: 'Volume',
        detail: `${status.volume}%`,
        adjust: (step) =>
          act('…', () =>
            forma.music.setVolume(Math.max(0, Math.min(100, status.volume + step * 5)))
          )
      },
      {
        label: 'Shuffle',
        detail: shuffle ? 'On, when a playlist starts' : 'Off',
        adjust: () => {
          shuffle = !shuffle;
          void page.refresh();
        },
        run: () => {
          shuffle = !shuffle;
          void page.refresh();
        }
      },
      {
        label: 'Show each new song',
        detail: seconds === 0 ? 'Off' : `For ${seconds} seconds`,
        adjust: showFor,
        run: () => showFor(1)
      }
    );
    const play = (s: MusicSource) => () =>
      page.run(`Starting ${s.name}…`, async () => {
        await forma.music.play(s.id, { shuffle: s.kind !== 'radio' && Boolean(shuffle) });
        return `Playing ${s.name}`;
      });
    const lists = sources.filter((s) => s.kind !== 'radio');
    const radios = sources.filter((s) => s.kind === 'radio' && s.id.startsWith('radio:'));
    // A music add-on's stations: played like the user's, removed with the add-on.
    const fromAddons = sources.filter((s) => s.kind === 'radio' && !s.id.startsWith('radio:'));
    lists.forEach((s, i) =>
      rows.push({
        heading: i === 0 ? 'Folders and playlists' : undefined,
        label: s.name,
        detail: `${s.tracks} ${s.tracks === 1 ? 'track' : 'tracks'}${status.source?.id === s.id ? ' · playing' : ''}`,
        run: play(s)
      })
    );
    radios.forEach((s, i) =>
      rows.push({
        heading: i === 0 ? 'Web radios' : undefined,
        label: s.name,
        detail: status.source?.id === s.id ? 'Playing' : '',
        run: play(s),
        alt: {
          label: 'Remove',
          run: () =>
            page.run('Removing…', async () => {
              await forma.music.removeRadio(s.id.slice('radio:'.length));
              return `${s.name} removed`;
            })
        }
      })
    );
    rows.push({
      heading: radios.length === 0 ? 'Web radios' : undefined,
      label: 'Add a web radio…',
      detail: 'A stream address, like https://ice1.somafm.com/groovesalad-128-mp3',
      run: async () => {
        const name = await page.ask({ title: 'Radio name' });
        if (!name?.trim()) return;
        const url = await page.ask({ title: `Address of ${name.trim()}`, value: 'https://' });
        if (!url?.trim()) return;
        await page.run('Adding…', async () => {
          await forma.music.addRadio(name, url);
          return `${name.trim()} added`;
        });
      }
    });
    fromAddons.forEach((s, i) =>
      rows.push({
        heading: i === 0 ? 'From add-ons' : undefined,
        label: s.name,
        detail: status.source?.id === s.id ? 'Playing' : '',
        run: play(s)
      })
    );
    return rows;
  };
}

function nowPlaying(status: MusicStatus): string {
  if (status.state === 'stopped' || !status.source) return 'Nothing playing';
  return status.title ?? status.source.name;
}

function playingDetail(status: MusicStatus): string {
  if (status.state === 'stopped' || !status.source) return 'Pick something below';
  return [
    status.state === 'paused' ? 'Paused' : null,
    status.artist,
    status.source.name,
    status.track !== null && status.tracks !== null ? `${status.track}/${status.tracks}` : null
  ]
    .filter(Boolean)
    .join(' · ');
}

const RA = 'global.retroachievements';

/** RetroAchievements: the account, and what Batocera's RetroArch does with it. */
/** Per system: how many games RetroAchievements is known for. */
async function matchRows(forma: Forma): Promise<Row[]> {
  const [matches, systems] = await Promise.all([
    forma.ra.matches(),
    forma.library.systems().catch(() => [])
  ]);
  const name = new Map(systems.map((s) => [s.name, s.fullname]));
  return [...matches.systems]
    .sort((a, b) => b.games - a.games)
    .map((s) => ({
      label: name.get(s.system) ?? s.system,
      detail: `${s.matched} of ${s.games} matched${s.hashed > s.matched ? ` · ${s.hashed - s.matched} not found` : ''}`
    }));
}

export function retroAchievementsScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const all = await forma.batocera.settings(RA);
    const get = (key: string) => all.find((s) => s.key === key);
    const user = get(`${RA}.username`)?.value;
    const signedIn = Boolean(user && get(`${RA}.token`)?.set);
    const apiKey = await forma.settings.getSystem<string>('ra.apiKey').catch(() => null);

    const rows: Row[] = [
      signedIn
        ? {
            label: `Signed in as ${user}`,
            detail: 'A: sign out',
            run: () =>
              page.run('Signing out…', async () => {
                await forma.ra.logout();
                return 'Signed out of RetroAchievements';
              })
          }
        : {
            label: 'Sign in…',
            detail: 'Your retroachievements.org account',
            run: async () => {
              const name = await page.ask({
                title: 'RetroAchievements username',
                value: user ?? ''
              });
              if (!name?.trim()) return;
              const password = await page.ask({
                title: `Password for ${name.trim()}`,
                secret: true
              });
              if (!password) return;
              await page.run('Signing in…', async () => {
                const who = await forma.ra.login(name, password);
                return `Signed in as ${who}`;
              });
            }
          },
      switchRow(forma, page, get(RA), 'Achievements', 'Earn them while you play')
    ];
    for (const [key, label, detail] of [
      ['hardcore', 'Hardcore mode', 'No save states, rewind or cheats; counts double'],
      ['encore', 'Encore mode', 'Unlock achievements you already have again'],
      ['leaderboards', 'Leaderboards', 'Submit scores and times'],
      ['richpresence', 'Rich presence', 'Show what you are doing on your profile'],
      ['unofficial', 'Unofficial achievements', 'Include ones still being tested'],
      ['challenge_indicators', 'Challenge indicators', 'Show an icon while a challenge is active'],
      ['screenshot', 'Screenshot on unlock', ''],
      ['verbose', 'Verbose messages', 'More notifications from RetroArch']
    ] as const) {
      const setting = get(`${RA}.${key}`);
      if (setting) rows.push(switchRow(forma, page, setting, label, detail));
    }
    const sound = get(`${RA}.sound`);
    if (sound) rows.push(choiceRow(forma, page, sound, 'Unlock sound', (v) => v ?? 'none'));
    const matches = await forma.ra.matches().catch(() => null);
    if (matches) {
      const sum = (k: 'games' | 'matched') => matches.systems.reduce((n, s) => n + s[k], 0);
      rows.push({
        heading: 'Forma',
        label: 'Games matched',
        detail: `${sum('matched').toLocaleString()} of ${sum('games').toLocaleString()}${matches.running ? ' · still looking' : ''}`,
        run: () => page.push('Games matched', () => matchRows(forma))
      });
    }
    rows.push({
      heading: matches ? undefined : 'Forma',
      label: 'Web API key',
      detail: apiKey
        ? 'Set (for your profile and unlock dates)'
        : 'Optional. retroachievements.org → Settings → Keys; adds your profile and unlock dates',
      run: async () => {
        const key = await page.ask({ title: 'RetroAchievements web API key', secret: true });
        if (key === null) return;
        await page.run('Saving…', async () => {
          await forma.settings.setSystem('ra.apiKey', key.trim() || null);
          return key.trim() ? 'Web API key saved' : 'Web API key removed';
        });
      }
    });
    return rows;
  };
}

const RELAYS: Record<string, string> = {
  none: 'Off (direct connection)',
  nyc: 'New York',
  madrid: 'Madrid',
  montreal: 'Montreal',
  saopaulo: 'São Paulo',
  custom: 'Custom server'
};

const NP = 'global.netplay';

/** Netplay: hosting and joining, then on or off and how others see and reach this box. */
export function netplayScreen(
  forma: Forma,
  page: Page,
  game: { id: number; name: string } | null = null
): () => Promise<Row[]> {
  return async () => {
    const all = await forma.batocera.settings(NP);
    const get = (key: string) => all.find((s) => s.key === key);
    const text = (
      setting: BatoceraSetting | undefined,
      label: string,
      detail: string,
      layout?: 'address'
    ): Row[] =>
      setting
        ? [
            {
              label,
              detail: setting.value ?? detail,
              run: async () => {
                const value = await page.ask({ title: label, value: setting.value ?? '', layout });
                if (value === null) return;
                await page.run('Saving…', async () => {
                  await forma.batocera.set(setting.key, value);
                });
              }
            }
          ]
        : [];
    const relay = get(`${NP}.relay`);
    return [
      ...(get(NP)?.value === '1' ? playRows(forma, page, game) : []),
      {
        ...switchRow(forma, page, get(NP), 'Netplay', 'Play online with others, through RetroArch'),
        heading: 'Settings'
      },
      ...text(get(`${NP}.nickname`), 'Nickname', 'Not set: how others see you'),
      ...text(get(`${NP}.port`), 'Port', '55435', 'address'),
      ...(relay
        ? [choiceRow(forma, page, relay, 'Relay server', (v) => RELAYS[v ?? 'none'] ?? v ?? '')]
        : []),
      ...(relay?.value === 'custom'
        ? text(get(`${NP}.customserver`), 'Custom server', 'Not set')
        : [])
    ];
  };
}

function switchRow(
  forma: Forma,
  page: Page,
  setting: BatoceraSetting | undefined,
  label: string,
  detail: string
): Row {
  if (!setting) return { label, detail: 'Not available' };
  const flip = () =>
    page.run('Saving…', async () => {
      await forma.batocera.set(setting.key, setting.value === '1' ? '0' : '1');
    });
  return { label: `${label}: ${onOff(setting.value)}`, detail, run: flip, adjust: flip };
}

function choiceRow(
  forma: Forma,
  page: Page,
  setting: BatoceraSetting,
  label: string,
  show: (value: string | null) => string
): Row {
  const choices = setting.choices;
  const at = Math.max(0, choices.indexOf(setting.value ?? choices[0] ?? ''));
  const pick = (step: -1 | 1) => {
    const next = choices[(at + step + choices.length) % choices.length];
    if (next === undefined) return;
    return page.run('Saving…', async () => {
      await forma.batocera.set(setting.key, next);
    });
  };
  return { label, detail: show(setting.value), adjust: pick, run: () => pick(1) };
}

/** One of the theme's own settings, as Forma's menu shows it. */
export interface ThemeSettingValue {
  key: string;
  label: string;
  description?: string;
  group?: string;
  /** The lists on which Select offers it too, under the theme's logo. */
  select?: ('systems' | 'games')[];
  type: 'toggle' | 'choice';
  choices?: { value: string; label: string }[];
  default: unknown;
  value: unknown;
}

export interface InterfaceHooks {
  /** The theme's declared settings, with their values now, and the only views it shows. */
  theme: () => Promise<{ settings: ThemeSettingValue[]; views: string[] | null }>;
  setThemeSetting: (key: string, value: unknown) => Promise<void>;
  /** A short message at the foot of the screen (removing a theme asks for a second press). */
  notify: (message: string) => void;
}

/** Forma's setting for a TV's overscan, shared by every theme: percent per side, 0–10. */
const MARGINS_SETTING = 'display.margins';

/**
 * Interface: which theme, where Forma opens, the screen margins every theme follows, and the
 * settings the theme itself offers (its `theme.json`).
 */
export function interfaceScreen(
  forma: Forma,
  page: Page,
  hooks: InterfaceHooks
): () => Promise<Row[]> {
  const cycle = <T>(values: readonly T[], now: T, step: -1 | 1): T =>
    values[(values.indexOf(now) + step + values.length) % values.length]!;

  const about = (t: ThemeInfo) => () =>
    page.push(t.name ?? t.id, themeScreen(forma, page, hooks.notify, t.id));

  return async () => {
    const [themes, allViews, home, margins, theme] = await Promise.all([
      forma.themes.list(),
      forma.nav.views(),
      forma.nav.defaultView(),
      forma.settings.getSystem<number>(MARGINS_SETTING).catch(() => null),
      hooks.theme()
    ]);
    const { settings } = theme;
    const views = theme.views ? allViews.filter((v) => theme.views!.includes(v.id)) : allViews;
    const rows: Row[] = themes.map((t, i) => ({
      heading: i === 0 ? 'Theme' : undefined,
      label: `${t.name ?? t.id}${t.version ? ` ${t.version}` : ''}`,
      logo: t.logo ? { src: t.logo, name: t.name ?? t.id, text: t.version ?? '' } : undefined,
      detail:
        (t.update ? `Update to ${t.update} · ` : '') +
        (t.current
          ? 'In use · A for more'
          : t.unavailable
            ? `Cannot load: ${t.unavailable} · Y for more`
            : `${t.origin === 'bundled' ? 'Comes with Forma' : 'Installed'} · A to use it, Y for more`),
      run: t.current
        ? about(t)
        : t.unavailable
          ? undefined
          : () => page.run(`Switching to ${t.name ?? t.id}…`, () => forma.themes.select(t.id)),
      alt: { label: 'More', run: about(t) }
    }));
    rows.push({
      label: 'Get more themes…',
      detail: 'Install and update themes from the catalogue',
      run: () => page.push('Get more themes', catalogueScreen(forma, page, hooks.notify))
    });

    const viewIds = views.map((v) => v.id);
    const startOn = home.id ?? 'systems';
    const pickView = (step: -1 | 1) =>
      page.run('Saving…', () => forma.nav.setDefaultView(cycle(viewIds, startOn, step)));
    // A theme that shows the library one way has nothing to start on but that.
    if (views.length > 1) {
      rows.push({
        label: `Start on: ${views.find((v) => v.id === startOn)?.name ?? startOn}`,
        detail: 'The view Forma opens on',
        adjust: pickView,
        run: () => pickView(1)
      });
    }

    const now = typeof margins === 'number' ? Math.min(10, Math.max(0, Math.round(margins))) : 0;
    const nudge = (step: -1 | 1) =>
      page.run('…', () =>
        forma.settings.setSystem(MARGINS_SETTING, Math.min(10, Math.max(0, now + step)))
      );
    rows.push({
      heading: 'Screen',
      label: `Screen margins: ${now}%`,
      detail: 'If the TV cuts the edges off; or set the TV to “Just Scan” / “Screen Fit”',
      adjust: nudge,
      run: () => nudge(1)
    });

    // The theme's settings, under its own headings (or one for the theme), each once.
    let group: string | null = null;
    for (const s of settings) {
      const heading = s.group ?? 'This theme';
      const row = themeSettingRow(s, page, (key, value) => hooks.setThemeSetting(key, value));
      if (heading !== group) row.heading = heading;
      group = heading;
      rows.push(row);
    }
    return rows;
  };
}

/** One of the theme's own settings as a row: A or left/right changes it. */
function themeSettingRow(
  s: ThemeSettingValue,
  page: Page,
  setThemeSetting: (key: string, value: unknown) => Promise<void>
): Row {
  const set = (value: unknown) => page.run('…', () => setThemeSetting(s.key, value));
  if (s.type === 'toggle') {
    const flip = () => set(!s.value);
    return {
      label: `${s.label}: ${s.value ? 'On' : 'Off'}`,
      detail: s.description,
      run: flip,
      adjust: flip
    };
  }
  const values = (s.choices ?? []).map((c) => c.value);
  const pick = (step: -1 | 1) =>
    set(values[(values.indexOf(String(s.value)) + step + values.length) % values.length]);
  const name = s.choices?.find((c) => c.value === s.value)?.label ?? String(s.value);
  return {
    label: `${s.label}: ${name}`,
    detail: s.description,
    adjust: pick,
    run: () => pick(1)
  };
}

/** What Select's page needs to offer the theme's own settings for the list shown. */
export interface ThemeOnSelect {
  /** The theme's settings now, fresh after every change. */
  theme: () => Promise<{
    name: string;
    settings: ThemeSettingValue[];
  } | null>;
  setThemeSetting: (key: string, value: unknown) => Promise<void>;
}

/** The theme's settings that Select offers on `list`. */
export function onSelect(settings: ThemeSettingValue[], list: string): ThemeSettingValue[] {
  return settings.filter((s) => s.select?.includes(list as 'systems' | 'games'));
}

/**
 * The theme's own row on Select's page: its logo, opening a page of the settings it offers on
 * `list`. `null` when it offers none there.
 */
async function themeSelectRow(page: Page, list: string, hooks: ThemeOnSelect): Promise<Row | null> {
  const theme = await hooks.theme();
  if (!theme || onSelect(theme.settings, list).length === 0) return null;
  const build = async () => {
    const now = await hooks.theme();
    return onSelect(now?.settings ?? [], list).map((s) =>
      themeSettingRow(s, page, hooks.setThemeSetting)
    );
  };
  return {
    heading: 'Look',
    label: 'Theme',
    detail: `Change how ${theme.name} shows your ${list}`,
    run: () => page.push('Theme', build)
  };
}

/** Forma's setting for how every theme lists systems (`SystemsOrder` in the SDK). */
const SYSTEMS_ORDER_SETTING = 'library.systemsOrder';

const SYSTEMS_ORDERS: { value: SystemsOrder; label: string }[] = [
  { value: 'name', label: 'Name (A to Z)' },
  { value: 'maker', label: 'Manufacturer' },
  { value: 'year', label: 'Year' }
];

/**
 * Select's page while the theme shows systems: the order they are listed in, for every theme;
 * and the views list, when the theme shows more than one.
 */
export function systemsScreen(
  forma: Forma,
  page: Page,
  views?: () => void,
  theme?: ThemeOnSelect,
  system?: { name: string; fullname: string | null } | null
): () => Promise<Row[]> {
  return async () => {
    const saved = await forma.settings.getSystem<string>(SYSTEMS_ORDER_SETTING).catch(() => null);
    const at = Math.max(
      0,
      SYSTEMS_ORDERS.findIndex((o) => o.value === saved)
    );
    const pick = (step: -1 | 1) => {
      const next = SYSTEMS_ORDERS[(at + step + SYSTEMS_ORDERS.length) % SYSTEMS_ORDERS.length]!;
      return page.run('Sorting…', () =>
        forma.settings.setSystem(SYSTEMS_ORDER_SETTING, next.value)
      );
    };
    const title = system ? system.fullname || system.name : '';
    const rows: Row[] = [
      ...(system
        ? [
            {
              icon: 'emulator' as const,
              heading: title,
              label: 'Emulator settings',
              detail: `Graphics, gameplay and emulator, for every ${title} game`,
              run: () =>
                page.push(
                  `${title} emulator`,
                  optionsScreen(forma, page, { level: 'system', system: system.name })
                )
            }
          ]
        : []),
      {
        heading: 'Systems',
        label: `Order: ${SYSTEMS_ORDERS[at]!.label}`,
        detail: 'How systems are listed, in every theme · ◀ ▶ to change',
        adjust: pick,
        run: () => pick(1)
      }
    ];
    const own = theme ? await themeSelectRow(page, 'systems', theme) : null;
    if (own) rows.push(own);
    if (views) rows.push({ heading: 'Views', label: 'Change view', run: views });
    return rows;
  };
}

/** Opens what a game has to look at: its manual, its media (Forma's reader). */
export interface GameLook {
  manual(game: Game): void;
  media(game: Game): void;
}

/**
 * Select's page while the theme shows games: the selected game's details and scraping; the
 * theme's own row, for the settings it offers on a game list; and the views list, when the
 * theme shows more than one.
 */
export function gamesScreen(
  forma: Forma,
  page: Page,
  theme: ThemeOnSelect,
  game: { id: number; name: string } | null,
  views?: () => void,
  close: () => void = () => page.close(),
  look?: GameLook
): () => Promise<Row[]> {
  return async () => {
    const rows: Row[] = game ? gameRows(forma, page, game) : [];
    const [same, full] = game
      ? await Promise.all([sameGame(forma, game.id), forma.library.game(game.id).catch(() => null)])
      : [[], null];
    // Its manual, read in Forma's reader; its pictures and video, one by one.
    if (full && look && full.media.manual) {
      rows.push({
        icon: 'manual',
        label: 'Manual',
        detail: 'Read it: Left and Right turn pages, A and X zoom in and out',
        run: () => look.manual(full)
      });
    }
    const pictures = full ? mediaToShow(full) : [];
    if (full && look && pictures.length > 0) {
      rows.push({
        icon: 'media',
        label: 'Media',
        detail: pictures.map(([, label]) => label).join(' · '),
        run: () => look.media(full)
      });
    }
    if (same.length > 1) {
      rows.push({
        icon: 'filter',
        label: 'Sibling games',
        detail: `Only it and its ${same.length === 2 ? 'sibling' : `${same.length - 1} siblings`} (other versions, other names) in the list; Back shows every game again`,
        run: async () => {
          await forma.nav.setFilter({ games: same });
          close();
        }
      });
    }
    const filter = await forma.nav.filterOptions().catch(() => null);
    if (filter) {
      const active = filterSummary(filter);
      rows.push({
        icon: 'filter',
        label: 'Filter list',
        detail: active || 'By name, first letter, genre and more',
        badge: active ? { text: `${filter.total} of ${filter.all}` } : undefined,
        run: () => page.push('Filter list', filterScreen(forma, page))
      });
    }
    const own = await themeSelectRow(page, 'games', theme);
    if (own) rows.push({ ...own, icon: 'theme' });
    if (views) rows.push({ heading: 'Views', label: 'Change view', run: views });
    if (game) rows.push(deleteGameRow(forma, page, game));
    return rows;
  };
}

/**
 * A game and every other in the library that may be the same game: its versions (other files
 * of one game) and sibling games (one game under other names, by LaunchBox). Its id alone
 * if there is none.
 */
async function sameGame(forma: Forma, id: number): Promise<number[]> {
  const game = await forma.library.game(id).catch(() => null);
  if (!game) return [];
  const [versions, duplicates] = await Promise.all([
    forma.library.versions(game.system).catch(() => []),
    forma.library.duplicates(game.system).catch(() => null)
  ]);
  const ids = new Set([id]);
  for (const group of [...versions, ...(duplicates ?? [])]) {
    if (group.games.some((g) => g.id === id)) group.games.forEach((g) => ids.add(g.id));
  }
  return [...ids];
}
