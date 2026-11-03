import type { BatoceraSetting, Forma, MusicSource, MusicStatus } from '@ramraiders/forma-sdk';

import { type Page, type Row, onOff } from './page.js';

/** Music: what plays, the controls, and everything there is to play. */
export function musicScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  let shuffle: boolean | null = null;
  const act = (label: string, work: () => Promise<unknown>) =>
    page.run(label, async () => {
      await work();
    });

  return async () => {
    const [status, sources] = await Promise.all([forma.music.status(), forma.music.sources()]);
    shuffle ??= status.shuffle;
    const rows: Row[] = [{ label: nowPlaying(status), detail: playingDetail(status) }];
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
      }
    );
    const play = (s: MusicSource) => () =>
      page.run(`Starting ${s.name}…`, async () => {
        await forma.music.play(s.id, { shuffle: s.kind !== 'radio' && Boolean(shuffle) });
        return `Playing ${s.name}`;
      });
    const lists = sources.filter((s) => s.kind !== 'radio');
    const radios = sources.filter((s) => s.kind === 'radio');
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
    rows.push({
      heading: 'Forma',
      label: 'Web API key',
      detail: apiKey
        ? 'Set (for your profile and progress in Forma)'
        : 'retroachievements.org → Settings → Keys; shows your profile and progress in Forma',
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

/** Netplay: on or off, how others see and reach this box. */
export function netplayScreen(forma: Forma, page: Page): () => Promise<Row[]> {
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
      switchRow(forma, page, get(NP), 'Netplay', 'Play online with others, through RetroArch'),
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
  type: 'toggle' | 'choice';
  choices?: { value: string; label: string }[];
  default: unknown;
  value: unknown;
}

export interface InterfaceHooks {
  /** The theme's declared settings, with their values now, and the only views it shows. */
  theme: () => Promise<{ settings: ThemeSettingValue[]; views: string[] | null }>;
  setThemeSetting: (key: string, value: unknown) => Promise<void>;
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
      detail: t.current
        ? 'In use'
        : t.unavailable
          ? `Cannot load: ${t.unavailable}`
          : t.origin === 'bundled'
            ? 'Comes with Forma · A to use it'
            : 'Installed · A to use it',
      run:
        t.current || t.unavailable
          ? undefined
          : () => page.run(`Switching to ${t.name ?? t.id}…`, () => forma.themes.select(t.id))
    }));

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
      const row = settingRow(s, page, hooks);
      if (heading !== group) row.heading = heading;
      group = heading;
      rows.push(row);
    }
    return rows;
  };

  function settingRow(s: ThemeSettingValue, page: Page, hooks: InterfaceHooks): Row {
    const set = (value: unknown) => page.run('…', () => hooks.setThemeSetting(s.key, value));
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
    const pick = (step: -1 | 1) => set(cycle(values, String(s.value), step));
    const name = s.choices?.find((c) => c.value === s.value)?.label ?? String(s.value);
    return {
      label: `${s.label}: ${name}`,
      detail: s.description,
      adjust: pick,
      run: () => pick(1)
    };
  }
}
