// @vitest-environment jsdom
/**
 * Forma's menus, driven like a gamepad would, against the SDK's mock. The mock grants the
 * permissions a test asks for; inside Forma the shell's token lets it past them whatever the
 * theme declares, which `ipc_tests.rs` checks on the Rust side.
 */
import {
  Forma,
  type Game,
  type InputEvent,
  type MediaKind,
  type ShellContext
} from '@ramraiders/forma-sdk';
import { mockBackend } from '@ramraiders/forma-sdk/mock';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { HOLD_MS, Shell, type ShellInfo } from './shell.js';
import { backupNote, themeUse } from './ui/scraper.js';

beforeEach(() => {
  // jsdom implements neither.
  Element.prototype.scrollIntoView = vi.fn();
  document.body.innerHTML = '';
});

function start(
  permissions: string[] = [],
  context: ShellContext = {},
  views: string[] | null = null,
  draws: string[] = []
) {
  const backend = mockBackend({ playMs: 50, permissions });
  const forma = new Forma(backend, '1.24.0', false);
  const open: boolean[] = [];
  const opened: string[] = [];
  const settings: [string, unknown][] = [];
  const saver: boolean[] = [];
  const theme: ShellInfo['theme'] = {
    id: 'neon',
    name: 'Neon',
    version: '2.0.0',
    settings: [
      {
        key: 'board',
        label: 'Board',
        group: 'Look',
        type: 'choice',
        choices: [
          { value: 'on', label: 'On' },
          { value: 'off', label: 'Off' }
        ],
        default: 'on',
        value: 'on'
      },
      { key: 'motion', label: 'Animations', type: 'toggle', default: true, value: true }
    ],
    views,
    draws
  };
  const shell = new Shell(forma, {
    info: () => Promise.resolve({ context, theme, formaVersion: '0.0.0' }),
    setOpen: (value) => {
      open.push(value);
      return Promise.resolve();
    },
    openView: (view) => {
      opened.push(view);
      return Promise.resolve();
    },
    openGame: (id) => {
      opened.push(`game ${id}`);
      return Promise.resolve();
    },
    setThemeSetting: (key, value) => {
      settings.push([key, value]);
      const setting = theme.settings.find((s) => s.key === key);
      if (setting) setting.value = value;
      return Promise.resolve();
    },
    screensaver: (active) => {
      saver.push(active);
      return Promise.resolve();
    }
  });
  document.body.append(shell.root);
  const root = shell.root;

  const press = async (action: InputEvent['action'], repeat = false) => {
    shell.input({ action, button: null, gamepad: 0, pressed: true, repeat });
    // Let go at once (Select's menu comes on release), unless a repeat: still held.
    if (!repeat) shell.input({ action, button: null, gamepad: 0, pressed: false, repeat: false });
    await vi.waitFor(() => Promise.resolve());
    await new Promise((r) => setTimeout(r, 0));
  };
  /** Holds a button `ms`, then lets go. */
  const hold = async (action: InputEvent['action'], ms: number) => {
    shell.input({ action, button: null, gamepad: 0, pressed: true, repeat: false });
    await new Promise((r) => setTimeout(r, ms));
    shell.input({ action, button: null, gamepad: 0, pressed: false, repeat: false });
    await new Promise((r) => setTimeout(r, 0));
  };
  const text = (selector: string) =>
    [...root.querySelectorAll(selector)].map((n) => n.textContent?.trim() ?? '');
  /** Start, down to the entry called `label`, A. */
  const openMenu = async (label: string) => {
    await press('start');
    await vi.waitFor(() => expect(text('.menu li .name').length).toBeGreaterThan(0));
    for (let i = 0; i < 20 && text('.menu li.selected .name')[0] !== label; i++) {
      await press('down');
    }
    expect(text('.menu li.selected .name')).toEqual([label]);
    await press('confirm');
  };
  return {
    hold,
    shell,
    root,
    press,
    text,
    forma,
    open,
    opened,
    settings,
    saver,
    openMenu,
    backend,
    theme
  };
}

describe('how the theme in use treats a kind of media', () => {
  const theme = {
    name: 'Cyber',
    view: null,
    media: [],
    fields: [],
    shows: ['wheel', 'marquee'] as MediaKind[],
    backups: { marquee: 'wheel' } as Partial<Record<MediaKind, MediaKind>>
  };
  const kinds = [{ kind: 'wheel' as MediaKind, label: 'Wheel' }];

  it('sets apart what it does not draw, and marks a backup', () => {
    expect(themeUse(theme, 'wheel')).toBe('shown');
    expect(themeUse(theme, 'marquee')).toBe('backup');
    expect(themeUse(theme, 'bezel')).toBe('unused');
    expect(backupNote(theme, 'marquee', kinds)).toBe('Cyber uses this only when there is no Wheel');
  });

  it('sets nothing apart for a theme that does not say', () => {
    expect(themeUse({ ...theme, shows: [], backups: {} }, 'bezel')).toBe('shown');
    expect(themeUse(null, 'bezel')).toBe('shown');
  });
});

describe('Forma’s shell', () => {
  it('shows each controller by kind and player, and the connection, top right', async () => {
    const { shell, root } = start();
    await vi.waitFor(() => expect(root.querySelectorAll('.status .pad').length).toBe(1));
    expect(root.querySelector('.status .pad-xbox .player')?.textContent).toBe('1');
    expect(root.querySelector('.status .net-ethernet')?.classList.contains('offline')).toBe(false);
    await shell.showStatus({
      controllers: [
        { player: 1, name: 'Xbox 360 Controller', kind: 'xbox' },
        { player: 2, name: 'DualSense', kind: 'playstation' },
        { player: 3, name: 'Pro Controller', kind: 'nintendo' }
      ],
      network: { kind: 'wifi', internet: false }
    });
    const pads = [...root.querySelectorAll('.status .pad')].map((p) =>
      p.getAttribute('aria-label')
    );
    expect(pads).toEqual([
      'Player 1: Xbox 360 Controller',
      'Player 2: DualSense',
      'Player 3: Pro Controller'
    ]);
    const net = root.querySelector('.status .net')!;
    expect(net.getAttribute('aria-label')).toBe('Wi-Fi, no internet');
    expect(net.classList.contains('offline')).toBe(true);
    expect(root.querySelector('.status')?.getAttribute('part')).toBe('status');
    shell.playing = true;
    expect(root.querySelector<HTMLElement>('.status')!.hidden).toBe(true);
  });

  it('leaves the corner to a theme that draws its own status', async () => {
    const { shell, root } = start([], {}, null, ['status']);
    await shell.showStatus({ controllers: [], network: { kind: 'none', internet: false } });
    expect(root.querySelector<HTMLElement>('.status')!.hidden).toBe(true);
  });

  it('goes back to its menu on B from a page, on the entry it came from', async () => {
    const { press, text, shell, openMenu } = start(['settings.system']);
    await openMenu('Audio');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Audio']));
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Audio']));
    expect(shell.open).toBe(true);
    // B in the menu itself closes it.
    await press('back');
    expect(shell.open).toBe(false);
  });

  it('goes back to its menu on B from the network screen and the views list', async () => {
    const { press, text, shell, openMenu } = start(['system.network']);
    await openMenu('Network');
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Network']));
    await press('back');
    await openMenu('Views');
    await vi.waitFor(() => expect(shell.open).toBe(true));
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Views']));
    // Start shuts everything, wherever it is.
    await press('start');
    expect(shell.open).toBe(false);
  });

  it('shows each new song for the time Forma’s setting says, then hides it', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      const { shell, root } = start();
      const song = root.querySelector<HTMLElement>('.song')!;
      expect(song.getAttribute('part')).toBe('now-playing');
      await shell.announce({ title: 'Angry Again', artist: 'Megadeth', seconds: 8 });
      expect(song.hidden).toBe(false);
      expect(song.querySelector('.title')?.textContent).toBe('Angry Again');
      expect(song.querySelector('.artist')?.textContent).toBe('Megadeth');
      vi.advanceTimersByTime(7900);
      expect(song.hidden).toBe(false);
      vi.advanceTimersByTime(200);
      expect(song.hidden).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('leaves the song to a theme that draws it, and shows none over a game', async () => {
    const drawn = start([], {}, null, ['nowPlaying']);
    await drawn.shell.announce({ title: 'Toxicity', artist: null, seconds: 8 });
    expect(drawn.root.querySelector<HTMLElement>('.song')!.hidden).toBe(true);

    const plain = start();
    plain.shell.playing = true;
    await plain.shell.announce({ title: 'Toxicity', artist: null, seconds: 8 });
    expect(plain.root.querySelector<HTMLElement>('.song')!.hidden).toBe(true);
  });

  it('sets how long a new song shows, from the music page', async () => {
    const { press, text, openMenu, forma } = start(['settings.system']);
    await openMenu('Audio');
    await vi.waitFor(() => expect(text('.page li .name')).toContain('Show each new song'));
    for (let i = 0; i < 30 && text('.page li.selected .name')[0] !== 'Show each new song'; i++) {
      await press('down');
    }
    expect(text('.page li.selected .description')).toEqual(['For 8 seconds']);
    await press('right');
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')).toEqual(['For 10 seconds'])
    );
    expect(await forma.settings.getSystem('music.songSeconds')).toBe(10);
    for (let i = 0; i < 6; i++) await press('left');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['Off']));
  });

  it('turns video sound off and on, from the audio page', async () => {
    const { press, text, openMenu, forma } = start(['settings.system']);
    await openMenu('Audio');
    await vi.waitFor(() => expect(text('.page li.selected .name')).toEqual(['Video sound']));
    expect(text('.page li.selected .description')[0]).toMatch(/^On/);
    expect((await forma.video.play(1)).sound).toBe(true);
    await press('confirm');
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')).toEqual(['Off: videos play muted'])
    );
    expect(await forma.settings.getSystem('video.sound')).toBe(false);
    expect((await forma.video.play(1)).sound).toBe(false);
    await press('right');
    await vi.waitFor(() => expect(text('.page li.selected .description')[0]).toMatch(/^On/));
  });

  it('keeps to the views a theme says it shows: one, and there is nothing to pick', async () => {
    const { press, text, shell, openMenu } = start([], {}, ['systems']);
    await press('start');
    expect(text('.menu li .name')).not.toContain('Views');
    await press('back');
    await press('select');
    expect(text('.toast')).toEqual(['Neon shows the library one way']);
    expect(shell.open).toBe(false);
    await openMenu('Interface');
    await vi.waitFor(() => expect(text('.page li .name').length).toBeGreaterThan(0));
    expect(text('.page li .name').some((n) => n.startsWith('Start on'))).toBe(false);
  });

  it('interface: a theme’s page says who made it and what it may do, and removes it', async () => {
    const { press, text, openMenu, forma } = start(['themes.manage']);
    await openMenu('Interface');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Neon 1.2.0'));
    while (selected() !== 'Neon 1.2.0') await press('down');
    await press('north');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Neon']));
    expect(text('.page > .panel li[role=option] .description')).toContain('RAM Raiders');
    expect(rows()).toContain('Quit Forma, restart or shut down the machine');
    while (selected() !== 'Remove') await press('down');
    await press('confirm');
    expect(rows()).toContain('Remove');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Interface']));
    await vi.waitFor(() => expect(rows()).not.toContain('Neon 1.2.0'));
    expect((await forma.themes.list()).map((t) => t.id)).toEqual(['mock']);
  });

  it('interface: installs from the catalogue and updates an installed theme', async () => {
    const { press, text, openMenu, forma } = start(['themes.manage']);
    await openMenu('Interface');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Get more themes…'));
    expect(details()[rows().indexOf('Neon 1.2.0')]).toMatch(/^Update to 1\.3\.0 · /);
    while (selected() !== 'Get more themes…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['Neon 1.3.0', 'Paper 0.4.0']));
    expect(details()).toEqual(['Installed 1.2.0 · update to 1.3.0', 'By A theme author']);
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Paper']));
    expect(rows()).toContain('Only show your library and play games');
    while (selected() !== 'Install') await press('down');
    await press('confirm');
    await vi.waitFor(async () =>
      expect((await forma.themes.list()).map((t) => t.id)).toContain('paper')
    );
    await press('back');
    await vi.waitFor(() => expect(details()[1]).toBe('Installed'));
    // Neon asks for a permission: Update takes a second press.
    await press('up');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Neon']));
    while (selected() !== 'Update to 1.3.0') await press('down');
    await press('confirm');
    expect((await forma.themes.list()).find((t) => t.id === 'neon')?.version).toBe('1.2.0');
    await press('confirm');
    await vi.waitFor(async () =>
      expect((await forma.themes.list()).find((t) => t.id === 'neon')?.version).toBe('1.3.0')
    );
  });

  it('magazines: a list by folder, a reader that turns pages and remembers where', async () => {
    const { press, text, openMenu, root } = start();
    await openMenu('Magazines');
    const rows = () => text('.page > .panel li[role=option] .name');
    await vi.waitFor(() => expect(rows()).toEqual(['Retro Gamer 1', 'Edge 9']));
    expect(text('.page .heading')).toEqual(['Magazines', 'Edge']);
    await press('confirm');
    const where = () => text('.reader .where')[0];
    await vi.waitFor(() => expect(where()).toBe('Retro Gamer 1 · 1 / 8 · A: zoom'));
    const page = () => root.querySelector<HTMLImageElement>('.reader .page')!.src;
    expect(page()).toContain('page%200');
    await press('right');
    await press('right');
    await press('left');
    expect(where()).toBe('Retro Gamer 1 · 2 / 8 · A: zoom');
    expect(page()).toContain('page%201');
    await press('confirm');
    expect(where()).toBe('Retro Gamer 1 · 2 / 8 · 1.5× · A: in · X: out');
    await press('back');
    expect(root.querySelector<HTMLElement>('.reader')!.hidden).toBe(true);
    await vi.waitFor(() =>
      expect(text('.page > .panel li[role=option] .description')[0]).toBe('PDF · read to page 2')
    );
    // Opening again starts where it was left.
    await press('confirm');
    await vi.waitFor(() => expect(where()).toBe('Retro Gamer 1 · 2 / 8 · A: zoom'));
    await press('start');
    expect(root.querySelector<HTMLElement>('.reader')!.hidden).toBe(true);
  });

  it('sibling games: one game under different names, per system', async () => {
    const { press, text, openMenu, backend } = start([]);
    const invoke = backend.invoke.bind(backend);
    let answer: unknown = null;
    vi.spyOn(backend, 'invoke').mockImplementation(async (command, args) =>
      command === 'library_duplicates' ? answer : invoke(command, args)
    );
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    await openMenu('Games');
    await vi.waitFor(() => expect(rows()).toContain('Sibling games'));
    for (let i = 0; i < 4; i++) await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(["Needs LaunchBox's data"]));

    answer = [
      {
        title: 'Mega Man',
        games: [
          { id: 1, name: 'Rockman', path: 'Rockman (Japan).nes' },
          { id: 2, name: 'Mega Man', path: 'Mega Man (USA).nes' }
        ]
      }
    ];
    await press('back');
    await press('confirm');
    await vi.waitFor(() => expect(details()[0]).toBe('1 game under several names'));
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['Mega Man']));
    expect(details()).toEqual(['Rockman · Mega Man']);
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['Rockman', 'Mega Man']));
    expect(details()).toEqual(['Rockman (Japan).nes', 'Mega Man (USA).nes']);
  });

  it('scraper: runs in the background, chooses what to fetch, sets up a source', async () => {
    const { press, text, openMenu } = start(['scraper.manage']);
    await openMenu('Games');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    const selected = () => text('.page li.selected .name')[0];
    const goTo = async (label: string) => {
      for (let i = 0; i < 40 && selected() !== label; i++) await press('down');
      expect(selected()).toBe(label);
    };
    await vi.waitFor(() =>
      expect(rows()).toEqual([
        'Scrapers',
        'Emulator options',
        'ROM check',
        'Game versions',
        'Sibling games',
        'Screen'
      ])
    );
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Start scraping'));
    expect(text('.page li.heading')).toEqual([
      'Scrape',
      'Scrapers, asked in this order',
      'Options'
    ]);
    expect(rows()).toContain('ScreenScraper');
    const at = (label: string) => details()[rows().indexOf(label)];
    expect(at('ScreenScraper')).toBe('On · needs setting up: A');
    expect(at('libretro thumbnails')).toBe('On · nothing to set up');

    // Everything, in the background: the page follows it to the end.
    await goTo('Start scraping');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Scrape everything'));
    await goTo('Scrape everything');
    await press('confirm');
    await vi.waitFor(() => expect(rows().some((r) => r.startsWith('Last: Everything'))).toBe(true));
    expect(at('Last: Everything')).toMatch(/^\d+ found · \d+ not found$/);

    // Manuals on.
    await goTo('What to fetch');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Manual'));
    expect(at('Manual')).toBe('Off');
    await goTo('Manual');
    await press('confirm');
    await vi.waitFor(() => expect(at('Manual')).toBe('On'));

    // What the theme asks for, on top: marked on its kinds, and can be turned off.
    expect(at('What Neon asks for')).toBe('On · Box spine, age ratings');
    expect(at('Box spine')).toBe('On · for Neon');
    await goTo('What Neon asks for');
    await press('confirm');
    await vi.waitFor(() => expect(at('Box spine')).toBe('Off'));
    await press('back');
    await vi.waitFor(() => expect(rows()).toContain('TheGamesDB'));

    // TheGamesDB's key, typed on the keyboard.
    await press('up');
    for (let i = 0; i < 40 && selected() !== 'TheGamesDB'; i++) await press('up');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['TheGamesDB']));
    await vi.waitFor(() => expect(rows()).toContain('API key'));
    await goTo('API key');
    expect(details()[rows().indexOf('API key')]).toBe('Needed: A to enter');
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['API key']);
    for (const key of 'abc123') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    await vi.waitFor(() =>
      expect(details()[rows().indexOf('API key')]).toBe('Entered · Y to clear')
    );
    await press('back');
    await vi.waitFor(() => expect(at('TheGamesDB')).toBe('On · set up'));
  });

  it('screen: the CRT shader, then picture and decorations, which give way to it', async () => {
    const { press, text, openMenu } = start(['settings.emulators']);
    await openMenu('Games');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    const at = (label: string) => details()[rows().indexOf(label)];
    const selected = () => text('.page li.selected .name')[0];
    const goTo = async (label: string) => {
      for (let i = 0; i < 60 && selected() !== label; i++) await press('down');
      expect(selected()).toBe(label);
    };
    await vi.waitFor(() => expect(rows()).toContain('Screen'));
    await goTo('Screen');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('CRT shader: Off'));
    // Off, global still shows what it would be; Batocera's shader set is there to pick.
    expect(at('Starts from')).toBe('Default · Monitor Balanced (koko-aio)');
    expect(text('.page li.heading')).toEqual(['CRT shader', 'Picture', 'Decorations']);
    expect(at('Shader set')).not.toContain('Off ·');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('CRT shader: On'));
    expect(at('Shader set')).toBe('Off · Forma’s CRT shader instead');
    // A switch flips; a number steps and stops at its ends.
    await goTo('Bloom/Halation');
    expect(at('Bloom/Halation')).toBe('Default · Off (koko-aio)');
    await press('right');
    await vi.waitFor(() => expect(at('Bloom/Halation')).toBe('On'));
    await goTo('Bloom Strength');
    await press('right');
    await vi.waitFor(() => expect(at('Bloom Strength')).toBe('0.46'));
    // koko-aio's bezel on: Batocera's decoration is off.
    expect(at('Decoration set')).not.toContain('koko-aio');
    await goTo('Bezel');
    await press('right');
    await vi.waitFor(() => expect(at('Decoration set')).toBe('Off · koko-aio draws the bezel'));
    await goTo('All settings…');
    expect(at('All settings…')).toBe('8 settings in 5 sections · 3 set here');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Bloom/Halation…'));
    expect(at('Bloom/Halation…')).toBe('On · 2 set here');
  });

  it('presets: one is switched on from its page, and the CRT shader follows', async () => {
    const { press, text, openMenu } = start(['settings.emulators']);
    await openMenu('Presets');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    const at = (label: string) => details()[rows().indexOf(label)];
    await vi.waitFor(() => expect(rows()).toContain('Raspberry Pi 5 · Monitor'));
    expect(at('No preset')).toBe('In use: your own settings alone');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Raspberry Pi 5 · Monitor: Off'));
    expect(text('.page li.heading')).toEqual([
      'What it does',
      'Emulator options it sets',
      'Your settings'
    ]);
    expect(rows()).toContain('nds.shaderset = none');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Raspberry Pi 5 · Monitor: On'));
    await press('back');
    await vi.waitFor(() =>
      expect(at('No preset')).toBe('Stop using Raspberry Pi 5 · Monitor: your own settings alone')
    );
  });

  it('a game on Select: its details by hand, and each scraper’s answers to choose from', async () => {
    const { press, text, backend, root } = start(
      ['scraper.manage'],
      { view: 'systems', list: 'games', game: { id: 1, name: 'Sonic' } },
      ['systems']
    );
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    const at = (label: string) => details()[rows().indexOf(label)];
    await press('select');
    await vi.waitFor(() =>
      expect(rows()).toEqual([
        'Scrape',
        'Edit details',
        'Emulator settings',
        'Screen',
        'Filter list',
        'Delete'
      ])
    );
    expect(text('.page h2')).toEqual(['SonicHomebrew Demo.nes']);
    // The game's file, small under its name.
    expect(text('.page h2 .file')).toEqual(['Homebrew Demo.nes']);
    expect(root.querySelector('.page li.danger .name')?.textContent).toBe('Delete');

    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Description'));
    expect(text('.page h2')).toEqual(['Scrape Sonic']);
    // It has been scraped before: nothing changes, gaps included, until chosen.
    // Only what would be written; its source is apart, in a badge.
    const badges = () => text('.page > .panel li[role=option] .badge');
    expect(at('Description')).toBe('');
    expect(badges()).toEqual(['Stays empty', 'Keep current']);
    expect(at('Save')).toBe('0 changes');

    // Another scraper's text.
    await press('confirm');
    await vi.waitFor(() =>
      expect(rows()).toEqual(['Leave empty', 'ScreenScraper · English', 'ArcadeDB'])
    );
    await press('down');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(at('Description')).toBe('From ArcadeDB.'));
    expect(badges()[0]).toBe('From ArcadeDB');

    // A picture to look at before choosing.
    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(rows()).toEqual([
        'Keep what it has',
        'ScreenScraper',
        'libretro thumbnails',
        'Remove and delete'
      ])
    );
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(badges()[1]).toBe('From ScreenScraper'));
    expect(at('Save')).toBe('2 changes');

    // Past the scraper that could not answer: Save is last.
    const apply = vi.spyOn(backend, 'invoke');
    await press('down');
    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(apply).toHaveBeenCalledWith('scraper_apply', {
        id: 1,
        text: { description: 'From ArcadeDB.' },
        media: [0]
      })
    );
    await vi.waitFor(() => expect(text('.toast')).toContain('Saved'));
    // The whole menu closes, not just the scrape page.
    expect(root.querySelector('.page')?.hasAttribute('hidden')).toBe(true);
  });

  it('the game list is filtered from Select: by first letter, and cleared again', async () => {
    const { press, text, backend } = start(['scraper.manage'], {
      view: 'systems',
      list: 'games',
      game: { id: 1, name: 'Sonic' }
    });
    const spy = vi.spyOn(backend, 'invoke');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Filter list'));
    while (text('.page li.selected .name')[0] !== 'Filter list') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Starting letter'));
    expect(rows()).not.toContain('Clear filter');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['✓ Any', 'M', 'S']));
    await press('down');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(details()[1]).toBe('S'));
    expect(spy).toHaveBeenCalledWith(
      'nav_set_filter',
      expect.objectContaining({ filter: expect.objectContaining({ letter: 'S' }) as unknown })
    );
    expect(rows()).toContain('Clear filter');

    while (text('.page li.selected .name')[0] !== 'Clear filter') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).not.toContain('Clear filter'));
    expect(spy).toHaveBeenCalledWith('nav_set_filter', { filter: null });
  });

  it('Select held on a game list shows only its favourites, and held again, all', async () => {
    const { press, hold, text, backend } = start([], {
      view: 'systems',
      list: 'games',
      game: { id: 1, name: 'Sonic' }
    });
    const spy = vi.spyOn(backend, 'invoke');
    await hold('select', HOLD_MS + 100);
    await vi.waitFor(() =>
      expect(spy).toHaveBeenCalledWith('nav_set_filter', { filter: { favorites: true } })
    );
    // No menu: the filter is on, said so.
    expect(text('.page > .panel li[role=option] .name')).toEqual([]);
    await vi.waitFor(() => expect(text('.toast')).toContain('Favourites only'));
    // Another part of the filter stays when it goes.
    await backend.invoke('nav_set_filter', { filter: { favorites: true, letter: 'S' } });
    spy.mockClear();
    await hold('select', HOLD_MS + 100);
    await vi.waitFor(() =>
      expect(spy).toHaveBeenCalledWith('nav_set_filter', {
        filter: { favorites: false, letter: 'S' }
      })
    );
    // A press is still the menu.
    await press('select');
    await vi.waitFor(() => expect(text('.page li .name')).toContain('Filter list'));
  });

  it('a game scraped for the first time starts from the first scraper’s answers', async () => {
    const { press, text, backend } = start(
      ['scraper.manage'],
      { view: 'systems', list: 'games', game: { id: 1, name: 'Sonic' } },
      ['systems']
    );
    const invoke = backend.invoke.bind(backend);
    vi.spyOn(backend, 'invoke').mockImplementation(async (command, args) => {
      const answer = await invoke(command, args);
      if (command !== 'scraper_offers') return answer;
      const offers = answer as { media: { current: boolean }[] };
      return { ...offers, media: offers.media.map((m) => ({ ...m, current: false })) };
    });
    const rows = () => text('.page > .panel li[role=option] .name');
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Scrape'));
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Description'));
    expect(text('.page > .panel li[role=option] .badge')).toEqual([
      'From ScreenScraper · English',
      'From ScreenScraper'
    ]);
  });

  it('scraping or editing a game shows its file name, which cannot be selected', async () => {
    const { press, text, backend } = start(
      ['scraper.manage'],
      { view: 'systems', list: 'games', game: { id: 1, name: 'Sonic' } },
      ['systems']
    );
    const invoke = backend.invoke.bind(backend);
    vi.spyOn(backend, 'invoke').mockImplementation(async (command, args) => {
      if (command === 'scraper_offers') {
        return {
          text: [
            {
              field: 'genre',
              label: 'Genre',
              current: null,
              offers: [{ source: 'ScreenScraper', value: 'Platform', note: null }]
            }
          ],
          media: [],
          problems: []
        };
      }
      const answer = await invoke(command, args);
      return command === 'library_game'
        ? { ...(answer as object), path: 'hacks/Sonic (USA) [h1].md' }
        : answer;
    });
    const rows = () => text('.page > .panel li[role=option] .name');
    const note = 'File: Sonic (USA) [h1].md';

    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Scrape'));
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Scrape Sonic']));
    expect(text('.page > .panel .note')).toEqual([note]);
    expect(rows()).not.toContain(note);

    await press('back');
    await vi.waitFor(() => expect(rows()).toContain('Edit details'));
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Edit Sonic']));
    expect(text('.page > .panel .note')).toEqual([note]);
    expect(rows()).not.toContain(note);
  });

  it('a field the game has can be cleared from the scrape page', async () => {
    const { press, text, backend } = start(
      ['scraper.manage'],
      { view: 'systems', list: 'games', game: { id: 1, name: 'Sonic' } },
      ['systems']
    );
    const invoke = backend.invoke.bind(backend);
    const spy = vi.spyOn(backend, 'invoke').mockImplementation((command, args) =>
      command === 'scraper_offers'
        ? Promise.resolve({
            text: [
              {
                field: 'genre',
                label: 'Genre',
                current: 'Wrong',
                offers: [{ source: 'ScreenScraper', value: 'Platform', note: null }]
              }
            ],
            media: [],
            problems: []
          } as never)
        : invoke(command, args)
    );
    const rows = () => text('.page > .panel li[role=option] .name');
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Scrape'));
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Scrape Sonic']));
    await press('confirm');
    await vi.waitFor(() =>
      expect(rows()).toEqual(['Keep what it has', 'ScreenScraper', 'Remove and delete'])
    );
    await press('down');
    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(text('.page > .panel li[role=option] .badge')).toEqual(['Will be cleared'])
    );

    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(spy).toHaveBeenCalledWith('scraper_edit', { id: 1, fields: { genre: '' } })
    );
  });

  it('a picture the game has can be removed and its file deleted, offers or not', async () => {
    const { press, text, backend } = start(
      ['scraper.manage'],
      { view: 'systems', list: 'games', game: { id: 1, name: 'Sonic' } },
      ['systems']
    );
    const invoke = backend.invoke.bind(backend);
    const spy = vi.spyOn(backend, 'invoke').mockImplementation((command, args) =>
      command === 'scraper_offers'
        ? Promise.resolve({
            text: [],
            media: [{ kind: 'image', label: 'Screenshot', current: true, offers: [] }],
            problems: []
          } as never)
        : invoke(command, args)
    );
    const rows = () => text('.page > .panel li[role=option] .name');
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Scrape'));
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Scrape Sonic']));
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['Keep what it has', 'Remove and delete']));
    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(text('.page > .panel li[role=option] .badge')).toEqual(['Will be deleted'])
    );
    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(spy).toHaveBeenCalledWith('scraper_remove_media', { id: 1, kinds: ['image'] })
    );
  });

  it('a name can be typed by hand instead of taken from a scraper', async () => {
    const { press, text, backend } = start(
      ['scraper.manage'],
      { view: 'systems', list: 'games', game: { id: 1, name: 'Sonic' } },
      ['systems']
    );
    const invoke = backend.invoke.bind(backend);
    const spy = vi.spyOn(backend, 'invoke').mockImplementation((command, args) =>
      command === 'scraper_offers'
        ? Promise.resolve({
            text: [
              {
                field: 'name',
                label: 'Name',
                current: 'Sonic',
                offers: [{ source: 'ScreenScraper', value: 'Sonic the Hedgehog', note: null }]
              }
            ],
            media: [],
            problems: []
          } as never)
        : invoke(command, args)
    );
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Scrape'));
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Scrape Sonic']));
    await press('confirm');
    await vi.waitFor(() =>
      expect(rows()).toEqual(['Keep what it has', 'ScreenScraper', 'Type it yourself'])
    );
    await press('down');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.keyboard h2')).toEqual(['Name']));
    const filled = text('.keyboard .field')[0]!.length;
    for (let i = 0; i < filled; i++) await press('back');
    for (const key of 'Mine') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    await vi.waitFor(() => expect(details()[0]).toBe('Mine'));
    expect(text('.page > .panel li[role=option] .badge')[0]).toBe('Typed by you');

    await press('down');
    await press('confirm');
    await vi.waitFor(() =>
      expect(spy).toHaveBeenCalledWith('scraper_apply', {
        id: 1,
        text: { name: 'Mine' },
        media: []
      })
    );
  });

  it('a game on Select can be deleted, asking first', async () => {
    const { press, text, backend, shell } = start([], {
      view: 'systems',
      list: 'games',
      game: { id: 1, name: 'Sonic' }
    });
    const rows = () => text('.page > .panel li[role=option] .name');
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Delete'));
    while (text('.page li.selected .name')[0] !== 'Delete') await press('down');

    const invoke = vi.spyOn(backend, 'invoke');
    await press('confirm'); // The first A only asks, on a page of its own, keeping it chosen.
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Delete Sonic?']));
    expect(rows()).toEqual(['Keep it', 'Delete for good']);
    expect(text('.page li.selected .name')).toEqual(['Keep it']);
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Delete'));
    expect(invoke).not.toHaveBeenCalledWith('library_delete_game', { id: 1 });

    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Delete Sonic?']));
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('library_delete_game', { id: 1 }));
    await vi.waitFor(() => expect(text('.toast')).toContain('Deleted Sonic'));
    expect(shell.open).toBe(false);
  });

  it('add-ons: installed ones, a page each with settings and switches, and the catalogue', async () => {
    const { press, text, openMenu } = start(['addons.manage', 'scraper.manage']);
    await openMenu('Add-ons');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    const at = (label: string) => details()[rows().indexOf(label)];
    const selected = () => text('.page li.selected .name')[0];
    const goTo = async (label: string) => {
      for (let i = 0; i < 40 && selected() !== label; i++) await press('down');
      expect(selected()).toBe(label);
    };
    await vi.waitFor(() =>
      expect(rows()).toEqual([
        'Web radios',
        'Example boxes 1.0.0',
        'Web radios 1.0.0',
        'Get more add-ons…'
      ])
    );
    expect(text('.page li.heading')).toEqual(['Open', 'Installed', 'More']);
    expect(at('Example boxes 1.0.0')).toBe('On · update to 1.1.0');

    // A menu add-on's pages, drawn by Forma: its rows, a deeper page, an action.
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['Chip FM', 'Arcade Lounge', 'About']));
    expect(text('.page li.heading')).toEqual(['Stations', 'More']);
    await press('confirm');
    await vi.waitFor(() =>
      expect(text('.toast')).toContain('Playing https://radio.example.com/chip')
    );
    await goTo('About');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toEqual(['Stations from radio.example.com']));
    await press('back');
    await press('back');

    // Its page: where it may go, its setting, off.
    await goTo('Example boxes 1.0.0');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Example boxes']));
    await vi.waitFor(() => expect(rows()).toContain('Token'));
    expect(rows()).toContain('boxes.example.com');
    expect(text('.page li.heading')).toEqual(['It may', 'Settings', 'Add-on']);
    await goTo('Use it');
    expect(at('Use it')).toBe('On · in Start → Scraper’s sources');
    await press('confirm');
    await vi.waitFor(() => expect(at('Use it')).toBe('Off'));
    await press('back');
    await vi.waitFor(() => expect(at('Example boxes 1.0.0')).toBe('Off · update to 1.1.0'));

    // A setting it reads: typed on the keyboard.
    await goTo('Web radios 1.0.0');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Your own station'));
    expect(rows()[0]).toBe('Open Web radios');
    expect(at('Your own station')).toBe('Optional: A to enter');
    await press('back');

    // The catalogue: install takes two presses.
    await goTo('Get more add-ons…');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Arcade flyers 0.3.0'));
    await goTo('Arcade flyers 0.3.0');
    await press('confirm');
    expect(text('.toast')[0]).toBe(
      'Arcade flyers may: reach the internet sites it lists, and only those. Press A again to install'
    );
    await press('confirm');
    await vi.waitFor(() => expect(at('Arcade flyers 0.3.0')).toBe('Installed'));

    // One with a system part says so before it installs.
    await goTo('FM tuner 0.1.0');
    await press('confirm');
    expect(text('.toast')[0]).toContain('It gets full control of this machine. Press A again');
    await press('confirm');
    await vi.waitFor(() => expect(at('FM tuner 0.1.0')).toBe('Installed'));
    await press('back');
    await vi.waitFor(() => expect(rows()).toContain('Arcade flyers 0.3.0'));
    expect(rows().slice(0, 2)).toEqual(['Web radios', 'FM tuner']);

    // Its page: what typed goes to its action, and the page shows what it did.
    await goTo('FM tuner 0.1.0');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Its programs'));
    expect(at('Its programs')).toBe('From Forma’s catalogue: installed with it, removed with it');
    expect(rows()[0]).toBe('Open FM tuner');
    await goTo('Open FM tuner');
    await press('confirm');
    await vi.waitFor(() => expect(at('Frequency')).toBe('Not tuned'));
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['Frequency']);
    for (const key of '99.1') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    await vi.waitFor(() => expect(text('.toast')).toContain('Tuned to 99.1'));
    await vi.waitFor(() => expect(at('Frequency')).toBe('99.1'));
  });

  it('a game’s manual from Select: pages turned, zoomed in and out, back to Select', async () => {
    const { press, text, root } = start([], {
      view: 'systems',
      list: 'games',
      game: { id: 2, name: 'Super Mario Bros.' }
    });
    const rows = () => text('.page > .panel li[role=option] .name');
    const where = () => text('.reader .where')[0];
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Manual'));
    expect(rows().indexOf('Media')).toBe(rows().indexOf('Manual') + 1);
    while (text('.page li.selected .name')[0] !== 'Manual') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(where()).toBe('Super Mario Bros. manual · 1 / 8 · A: zoom'));
    await press('right');
    await press('pageDown');
    expect(where()).toBe('Super Mario Bros. manual · 3 / 8 · A: zoom');
    await press('confirm');
    await press('r2');
    expect(where()).toBe('Super Mario Bros. manual · 3 / 8 · 2× · A: in · X: out');
    expect(root.querySelector('.reader')!.classList.contains('zoomed')).toBe(true);
    // Zoomed, Left and Right move over the page; L1 and R1 still turn.
    await press('left');
    expect(where()).toContain('3 / 8');
    await press('pageUp');
    expect(where()).toContain('2 / 8');
    await press('west');
    await press('l2');
    expect(where()).toBe('Super Mario Bros. manual · 2 / 8 · A: zoom');
    // Back at 1×: fitted to the screen by its style, nothing left of the zoom.
    expect(root.querySelector('.reader')!.classList.contains('zoomed')).toBe(false);
    expect(root.querySelector<HTMLElement>('.reader .page')!.style.width).toBe('');
    expect(root.querySelector<HTMLElement>('.reader .sheet')!.scrollTop).toBe(0);
    await press('back');
    expect(root.querySelector<HTMLElement>('.reader')!.hidden).toBe(true);
    expect(rows()).toContain('Manual');
  });

  it('a game’s media from Select: its pictures one by one, its video played by Forma', async () => {
    const { press, text, root, backend } = start([], {
      view: 'systems',
      list: 'games',
      game: { id: 2, name: 'Super Mario Bros.' }
    });
    // The mock draws no video; here Mario has one.
    const invoke = backend.invoke.bind(backend);
    const spy = vi.spyOn(backend, 'invoke').mockImplementation(async (command, args) => {
      const answer = await invoke(command, args);
      return command === 'library_game' && answer
        ? { ...(answer as Game), media: { ...(answer as Game).media, video: 'v.mp4' } }
        : answer;
    });
    const rows = () => text('.page > .panel li[role=option] .name');
    const where = () => text('.reader .where')[0];
    await press('select');
    await vi.waitFor(() => expect(rows()).toContain('Media'));
    const at = rows().indexOf('Media');
    expect(text('.page > .panel li[role=option] .description')[at]).toBe(
      'Video · Screenshot · Box spine · Whole box · Marquee · Banner · Flyer · Cabinet'
    );
    while (text('.page li.selected .name')[0] !== 'Media') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(where()).toBe('Super Mario Bros. · Video (1 / 8)'));
    expect(spy).toHaveBeenCalledWith('video_play', expect.objectContaining({ gameId: 2 }));
    expect(root.querySelector('.reader')!.classList.contains('video')).toBe(true);
    await press('right');
    expect(where()).toBe('Super Mario Bros. · Screenshot (2 / 8) · A: zoom');
    expect(spy.mock.calls.some(([command]) => command === 'video_stop')).toBe(true);
    expect(root.querySelector('.reader')!.classList.contains('video')).toBe(false);
    await press('confirm');
    expect(where()).toBe('Super Mario Bros. · Screenshot (2 / 8) · 1.5× · A: in · X: out');
    await press('back');
    expect(root.querySelector<HTMLElement>('.reader')!.hidden).toBe(true);
  });

  it('a theme opens a game’s manual in Forma’s reader', async () => {
    const { text, forma } = start();
    await forma.library.openManual(2);
    await vi.waitFor(() =>
      expect(text('.reader .where')[0]).toBe('Super Mario Bros. manual · 1 / 8 · A: zoom')
    );
  });

  it('magazines: a theme opens one in Forma’s reader', async () => {
    const { text, forma } = start();
    await forma.magazines.open('Edge/Edge 9.cbz');
    await vi.waitFor(() => expect(text('.reader .where')[0]).toBe('Edge 9 · 1 / 8 · A: zoom'));
  });

  it("on Select over a system, opens that system's emulator settings", async () => {
    const { press, text } = start(
      ['settings.system'],
      {
        view: 'systems',
        list: 'systems',
        system: { name: 'megadrive', fullname: 'Mega Drive' }
      },
      ['systems']
    );
    await press('select');
    await vi.waitFor(() => expect(text('.page li .name')[0]).toBe('Emulator settings'));
    expect(text('.page h2')).toEqual(['Mega Drive']);
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Mega Drive emulator']));
  });

  it('on Select over systems, offers their order, for every theme, and shuts on Select', async () => {
    const { press, text, shell, forma } = start(
      ['settings.system'],
      { view: 'systems', list: 'systems' },
      ['systems']
    );
    await press('select');
    await vi.waitFor(() => expect(text('.page li .name').length).toBeGreaterThan(0));
    expect(text('.page h2')).toEqual(['Systems']);
    // By maker, then year, unless chosen otherwise.
    expect(text('.page li .name')).toEqual(['Order: Manufacturer, then year']);
    const makers = (await forma.library.systems()).map((s) => s.manufacturer ?? '');
    // By maker; those without one last.
    const named = makers.filter(Boolean);
    expect(named).toEqual([...named].sort((a, b) => a.localeCompare(b)));
    expect(makers.slice(named.length).every((m) => m === '')).toBe(true);
    await press('right');
    await vi.waitFor(() => expect(text('.page li .name')).toEqual(['Order: Name (A to Z)']));
    expect(await forma.settings.getSystem('library.systemsOrder')).toBe('name');
    await press('left');
    await press('left');
    await vi.waitFor(() => expect(text('.page li .name')).toEqual(['Order: Year']));
    await press('select');
    expect(shell.open).toBe(false);
  });

  it('on Select over categories, offers the order of the systems in them too', async () => {
    const { press, text } = start(['settings.system'], { view: 'systems', list: 'categories' }, [
      'systems'
    ]);
    await press('select');
    await vi.waitFor(() => expect(text('.page li .name').length).toBeGreaterThan(0));
    expect(text('.page li .name')).toEqual(['Order: Manufacturer, then year']);
  });

  it('on Select over games, offers the theme’s own settings for them, as Theme', async () => {
    const { press, text, theme, settings, shell } = start([], { view: 'systems', list: 'games' }, [
      'systems'
    ]);
    theme.settings.push({
      key: 'gamesView',
      label: 'View',
      select: ['games'],
      type: 'choice',
      choices: [
        { value: 'immersive', label: 'Immersive' },
        { value: 'shelf', label: 'Shelf' }
      ],
      default: 'immersive',
      value: 'immersive'
    });
    await press('select');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Games']));
    expect(text('.page li .name')).toEqual(['Filter list', 'Theme']);
    expect(text('.page li .description')).toEqual([
      'By name, first letter, genre and more',
      'Change how Neon shows your games'
    ]);
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Theme']));
    expect(text('.page li .name')).toEqual(['View: Immersive']);
    await press('right');
    await vi.waitFor(() => expect(text('.page li .name')).toEqual(['View: Shelf']));
    expect(settings).toEqual([['gamesView', 'shelf']]);
    await press('back');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Games']));
    await press('select');
    expect(shell.open).toBe(false);
  });

  it('on Select elsewhere, still offers the views', async () => {
    const { press, text } = start([], { view: 'systems', list: 'games' }, ['systems']);
    await press('select');
    expect(text('.toast')).toEqual(['Neon shows the library one way']);
  });

  it('opens its menu on Start, over any theme, and tells Forma input is its own', async () => {
    const { press, text, open, shell } = start();
    await press('confirm');
    expect(shell.open).toBe(false);
    await press('start');
    expect(text('.menu h2')).toEqual(['Forma 0.0.0']);
    expect(text('.menu .note')).toEqual(['Neon 2.0.0']);
    expect(text('.menu li .name')).toEqual([
      'Collections',
      'Views',
      'Downloads',
      'Magazines',
      'Games',
      'Achievements',
      'Netplay',
      'Presets',
      'Interface',
      'Screensaver',
      'Audio',
      'System',
      'Add-ons',
      'Controllers',
      'Network',
      'Switch to ES',
      'Restart',
      'Shut down'
    ]);
    expect(text('.menu h3')).toEqual(['Library', 'Play', 'Setup', 'Settings', 'Devices', 'Power']);
    expect(text('.menu .about')).toEqual(['Your own lists of games']);
    expect(text('.hints .hint')).toEqual(['AChoose', 'BClose']);
    expect(open).toEqual([true]);
    await press('back');
    expect(shell.open).toBe(false);
    expect(open).toEqual([true, false]);
  });

  it('offers options for the game and system the theme says are selected', async () => {
    const { press, text } = start([], {
      view: 'systems',
      system: { name: 'snes', fullname: 'Super Nintendo' },
      game: { id: 3, name: 'Super Metroid' }
    });
    await press('start');
    expect(text('.menu li .name').slice(0, 2)).toEqual(['Game options', 'System options']);
    expect(text('.menu h3')[0]).toBe('Selected');
    expect(text('.menu .about')).toEqual(['Emulator, core and options for Super Metroid only']);
  });

  it('moves across the menu as it looks: rows of tiles, left and right too', async () => {
    const { press, text, root } = start();
    await press('start');
    // Lay the tiles out as on screen: four across in each row's own band.
    const tiles = [...root.querySelectorAll<HTMLElement>('.menu li')];
    const sections = [...root.querySelectorAll('.menu section')];
    let top = 0;
    for (const section of sections) {
      const own = [...section.querySelectorAll<HTMLElement>('li')];
      own.forEach((tile, n) => {
        const box = {
          left: (n % 4) * 100,
          top: top + Math.floor(n / 4) * 50,
          width: 90,
          height: 40
        };
        tile.getBoundingClientRect = () =>
          ({
            ...box,
            right: box.left + 90,
            bottom: box.top + 40,
            x: box.left,
            y: box.top
          }) as DOMRect;
      });
      top += Math.ceil(own.length / 4) * 50 + 20;
    }
    expect(tiles.length).toBe(18);
    const at = () => text('.menu li.selected .name')[0];
    expect(at()).toBe('Collections');
    await press('right');
    expect(at()).toBe('Views');
    await press('down');
    // the next row has one tile: the nearest across
    expect(at()).toBe('Games');
    await press('down');
    // the tile below, in the next row
    expect(at()).toBe('Achievements');
    await press('right');
    await press('down');
    // a row of one tile: the nearest across
    expect(at()).toBe('Presets');
    await press('down');
    expect(at()).toBe('Interface');
    await press('right');
    expect(at()).toBe('Screensaver');
    await press('right');
    expect(at()).toBe('Audio');
    await press('right');
    expect(at()).toBe('System');
    await press('down');
    // the row's second line has one tile: the nearest across
    expect(at()).toBe('Add-ons');
    await press('down');
    expect(at()).toBe('Controllers');
    await press('right');
    expect(at()).toBe('Network');
    await press('down');
    expect(at()).toBe('Restart');
    await press('down');
    // down from the bottom wraps to the top, in the same column
    expect(at()).toBe('Views');
    await press('left');
    await press('left');
    // left stops at the edge
    expect(at()).toBe('Collections');
  });

  it('power actions need a second press', async () => {
    const { press, text } = start();
    await press('start');
    for (let i = 0; i < 16; i++) await press('down');
    expect(text('.menu li.selected .name')).toEqual(['Restart']);
    await press('confirm');
    expect(text('.toast')).toEqual(['Press A again to restart']);
    await press('confirm');
    await vi.waitFor(() => expect(text('.toast')[0]).toContain('system.power'));
  });

  it('lists views on Select, opens one in the theme, and sets the home view', async () => {
    const { press, text, forma, opened, shell } = start([], { view: 'systems' });
    await press('select');
    await vi.waitFor(() => expect(text('.switcher li.selected .name')).toEqual(['Systems']));
    expect(text('.switcher li.selected .badge')).toEqual(['Home', 'Showing']);
    await press('down');
    await press('north');
    await vi.waitFor(async () =>
      expect((await forma.nav.defaultView()).id).toBe('manufacturer-system')
    );
    await press('confirm');
    expect(opened).toEqual(['manufacturer-system']);
    expect(shell.open).toBe(false);
  });

  it('switches theme, and sets the theme’s own settings, from Interface', async () => {
    const { press, text, settings, forma } = start(['settings.system']);
    await press('start');
    for (let i = 0; i < 8; i++) await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Interface']));
    expect(text('.page li.heading')).toEqual(['Theme', 'Screen', 'Look', 'This theme']);
    const names = text('.page li[role=option] .name');
    expect(names).toContain('Board: On');
    expect(names).toContain('Animations: On');

    // Down to the board, past the themes, Start on and the margins.
    const board = names.indexOf('Board: On');
    for (let i = 0; i < board; i++) await press('down');
    await press('right');
    await vi.waitFor(() => expect(settings).toEqual([['board', 'off']]));
    await vi.waitFor(() => expect(text('.page li.selected .name')).toEqual(['Board: Off']));

    // Margins are Forma's, shared by every theme.
    const margins = names.findIndex((n) => n.startsWith('Screen margins'));
    for (let i = board; i > margins; i--) await press('up');
    await press('right');
    await vi.waitFor(async () => expect(await forma.settings.getSystem('display.margins')).toBe(1));
  });

  it('shows a theme’s logo in place of its name in Interface', async () => {
    const { press, root, forma } = start(['settings.system']);
    vi.spyOn(forma.themes, 'list').mockResolvedValue([
      {
        id: 'cyber',
        name: 'Cyber',
        version: '0.7.12',
        origin: 'bundled',
        author: null,
        description: null,
        permissions: [],
        permissionDescriptions: [],
        removable: false,
        update: null,
        current: true,
        unavailable: null,
        logo: 'forma://localhost/theme/cyber/logo.svg'
      }
    ]);
    await press('start');
    for (let i = 0; i < 8; i++) await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(root.querySelector('.page .theme-logo')).not.toBeNull());
    const logo = root.querySelector<HTMLImageElement>('.page .theme-logo')!;
    expect(logo.getAttribute('src')).toBe('forma://localhost/theme/cyber/logo.svg');
    expect(logo.alt).toBe('Cyber');
    expect(logo.closest('.name')!.textContent).toBe('0.7.12');
  });

  it('Start closes whatever is open, except the keyboard, which it finishes', async () => {
    const { press, shell } = start(['system.network']);
    await press('start');
    await press('down');
    await press('confirm');
    expect(shell.open).toBe(true);
    await press('start');
    expect(shell.open).toBe(false);
  });

  it('stays shut while a game runs', async () => {
    const { press, shell } = start();
    shell.playing = true;
    await press('start');
    expect(shell.open).toBe(false);
  });

  it('joins a Wi-Fi network with a password typed on the on-screen keyboard', async () => {
    const { press, text } = start(['system.network']);
    await press('start');
    for (let i = 0; i < 14; i++) await press('down');
    expect(text('.menu li.selected .name')).toEqual(['Network']);
    await press('confirm');
    const rows = () => text('.network > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('▮▮▯▯  Office 5G'));
    expect(rows()[0]).toBe('Ethernet');
    expect(rows()[1]).toBe('Wi-Fi: on');

    for (let i = 0; i < 4; i++) await press('down');
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['Password for Office 5G']);
    await press('confirm');
    for (const key of 'uiet-place') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    }
    await press('north');
    expect(text('.keyboard .field')).toEqual(['quiet-place']);
    await press('start');
    await vi.waitFor(() =>
      expect(text('.toast')[0]).toBe(
        'Office 5G is saved. The cable stays in use; unplug it to use Wi-Fi'
      )
    );
  });
});

describe('Forma’s settings pages', () => {
  it('music: picks a playlist, shows it playing, and adds a web radio', async () => {
    const { press, text, openMenu } = start(['music.manage']);
    await openMenu('Audio');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('Chiptune'));
    expect(rows().slice(0, 2)).toEqual(['Video sound', 'Nothing playing']);
    expect(text('.page li.heading')).toEqual([
      'Folders and playlists',
      'Web radios',
      'From add-ons'
    ]);

    // Down to Chiptune: Video sound, Nothing playing, Volume, Shuffle, Show each new song, Music,
    // Chiptune.
    for (let i = 0; i < 6; i++) await press('down');
    expect(text('.page li.selected .name')).toEqual(['Chiptune']);
    await press('confirm');
    await vi.waitFor(() => expect(rows()[1]).toBe('Track 1'));
    // Rows were added above it (Pause, Next…): the selection stayed on Chiptune.
    expect(text('.page li.selected .name')).toEqual(['Chiptune']);
    expect(rows()).toContain('Pause');

    // Add a radio through the keyboard: name, then address.
    for (let i = 0; i < 20 && text('.page li.selected .name')[0] !== 'Add a web radio…'; i++) {
      await press('down');
    }
    expect(text('.page li.selected .name')).toEqual(['Add a web radio…']);
    await press('confirm');
    for (const key of 'Lofi') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    for (const key of 'lofi.example/live') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    }
    await press('start');
    await vi.waitFor(() => expect(text('.toast')[0]).toBe('Lofi added'));
    expect(rows()).toContain('Lofi');
  });

  it('RetroAchievements: signs in, and flips a switch', async () => {
    const { press, text, openMenu } = start(['settings.batocera', 'settings.system']);
    await openMenu('Achievements');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()[0]).toBe('Sign in…'));
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['RetroAchievements username']);
    // Clear what is there (the fixture's user; B on an empty field would cancel), then type.
    const filled = text('.keyboard .field')[0]!.length;
    for (let i = 0; i < filled; i++) await press('back');
    for (const key of 'mike') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    expect(text('.keyboard h2')).toEqual(['Password for mike']);
    for (const key of 'secret!') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    await vi.waitFor(() => expect(text('.toast')[0]).toBe('Signed in as mike'));
    expect(rows()[0]).toBe('Signed in as mike');

    await press('down');
    const before = text('.page li.selected .name')[0];
    await press('right');
    await vi.waitFor(() => expect(text('.page li.selected .name')[0]).not.toBe(before));
  });

  it('netplay: cycles the relay server and shows the custom one only when chosen', async () => {
    const { press, text, openMenu } = start(['settings.batocera']);
    await openMenu('Netplay');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('Relay server'));
    expect(rows()).not.toContain('Custom server');
    while (text('.page li.selected .name')[0] !== 'Relay server') await press('down');
    // Back from the first choice wraps to the last: Custom server.
    await press('left');
    await vi.waitFor(() => expect(rows()).toContain('Custom server'));
    expect(text('.page li.selected .description')).toEqual(['Custom server']);
  });

  it('netplay on: hosts the selected game, lists rooms for your games, joins one', async () => {
    const { shell, press, text, openMenu, forma, backend } = start(['settings.batocera'], {
      game: { id: 2, name: 'Super Mario Bros.' }
    });
    await forma.batocera.set('global.netplay', '1');
    const launches: unknown[] = [];
    const invoke = backend.invoke.bind(backend);
    backend.invoke = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      if (cmd === 'launch_game') launches.push(args);
      return invoke<T>(cmd, args);
    };
    await openMenu('Netplay');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()[0]).toBe('Host Super Mario Bros.'));
    expect(rows().slice(1, 3)).toEqual(['Rooms online…', 'On this network…']);
    expect(text('.page li.heading')).toEqual(['Play', 'Settings']);

    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Rooms online']));
    await vi.waitFor(() => expect(rows()).toContain('Super Mario Bros. (World)'));
    expect(rows()).toHaveLength(2);
    // Every room: the one this box cannot join says why.
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toHaveLength(3));
    expect(text('.page li[role=option] .description').at(-1)).toContain('Not on this box');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toHaveLength(2));
    await press('down');
    expect(selected()).toBe('Super Mario Bros. (World)');
    expect(text('.page li.selected .description')).toEqual(['Kirby_Fan · FCEUmm · 30 ms']);
    await press('confirm');
    await vi.waitFor(() => expect(launches).toHaveLength(1));
    expect(launches[0]).toMatchObject({
      id: 2,
      request: { core: 'fceumm', netplay: { mode: 'client', host: '192.0.2.10' } }
    });
    // The game starts: the page goes.
    await vi.waitFor(() => expect(shell.open).toBe(false));
  });

  it('netplay: with the lobby down, rooms online says so and tries again', async () => {
    const { press, text, openMenu, forma, backend } = start(['settings.batocera']);
    await forma.batocera.set('global.netplay', '1');
    const invoke = backend.invoke.bind(backend);
    let down = true;
    backend.invoke = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
      cmd === 'netplay_lobby' && down
        ? Promise.reject(new Error('lobby.libretro.com unreachable'))
        : invoke<T>(cmd, args);
    await openMenu('Netplay');
    const rows = () => text('.page > .panel li[role=option] .name');
    await vi.waitFor(() => expect(rows()[0]).toBe('Rooms online…'));
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('The lobby does not answer'));
    down = false;
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Super Mario Bros. (World)'));
  });

  it('collections: puts the selected game in one, makes one with it, deletes on a second Y', async () => {
    const { press, text, openMenu, forma } = start([], {
      game: { id: 2, name: 'Super Mario Bros.' }
    });
    await openMenu('Collections');
    const rows = () => text('.page > .panel li[role=option] .name');
    await vi.waitFor(() => expect(rows()[0]).toMatch(/^Couch co-op: /));
    const was = rows()[0]!;
    await press('confirm');
    await vi.waitFor(() => expect(rows()[0]).not.toBe(was));
    const inIt = rows()[0] === 'Couch co-op: In';
    expect((await forma.collections.of(2)).includes('Couch co-op')).toBe(inIt);

    // New collection…: typed on the keyboard, with the game in it.
    await press('down');
    expect(text('.page li.selected .name')).toEqual(['New collection…']);
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['Name of the collection']);
    for (const key of 'Zz') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    await vi.waitFor(() => expect(rows()).toContain('Zz: In'));
    expect(text('.toast')[0]).toBe('Added Super Mario Bros. to Zz');

    // Y once warns, Y again deletes.
    while (text('.page li.selected .name')[0] !== 'Zz: In') await press('up');
    await press('north');
    expect(text('.toast')[0]).toBe('Press Y again to delete Zz');
    expect(rows()).toContain('Zz: In');
    await press('north');
    await vi.waitFor(() => expect(rows()).not.toContain('Zz: In'));
    expect((await forma.collections.list()).map((c) => c.name)).toEqual(['Couch co-op']);
  });

  it('game settings: global options, a choice cycled, a page of its own and back', async () => {
    const { press, text, openMenu } = start(['settings.emulators']);
    await openMenu('Games');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Emulator options'));
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Game aspect ratio'));
    expect(text('.page h2')).toEqual(['Emulator options']);
    // How the game looks is on Screen, not here.
    expect(rows()).not.toContain('Game rendering & shaders…');
    expect(rows()).not.toContain('Decorations…');
    while (selected() !== 'Game aspect ratio') await press('down');
    expect(text('.page li.selected .description')).toEqual(['Default']);
    // A list to pick from, which goes back to the option with its new value.
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Game aspect ratio']));
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['4/3']));
    // Its own page: B comes back to the row it was opened from.
    while (selected() !== 'AI game translation…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['AI game translation']));
    expect(rows()).toHaveLength(1);
    await press('back');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Emulator options']));
    expect(selected()).toBe('AI game translation…');
  });

  it("a system's options follow its core, and a game inherits them", async () => {
    const context: ShellContext = {
      view: 'systems',
      system: { name: 'nes', fullname: 'Nintendo Entertainment System' }
    };
    const { press, text, openMenu } = start(['settings.emulators'], context);
    const rows = () => text('.page > .panel li[role=option] .name');
    await openMenu('System options');
    await vi.waitFor(() => expect(rows().slice(0, 2)).toEqual(['Emulator', 'Core']));
    expect(text('.page .heading')).toContain('Advanced options');
    expect(rows()).toContain('Palette');
    while (text('.page li.selected .name')[0] !== 'Core') await press('down');
    // Default (fceumm) → the first core in the list, then the next: nestopia, whose options
    // have no palette.
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Core']));
    await press('down');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['nestopia']));
    expect(rows()).not.toContain('Palette');
    // B back to the menu, B again to close it.
    await press('back');
    await press('back');

    // A game of the NES selected in the theme: its core is the system's.
    context.game = { id: 2, name: 'Super Mario Bros.' };
    await openMenu('Game options');
    await vi.waitFor(() => expect(rows()).toContain('Core'));
    const core = rows().indexOf('Core');
    expect(text('.page > .panel li[role=option] .description')[core]).toBe(
      'Default · nestopia (system)'
    );
  });

  it('system settings: information, a long list picked from, and when it applies', async () => {
    const { press, text, openMenu } = start(['system.settings']);
    await openMenu('System');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Time zone'));
    expect(text('.page .heading')).toEqual([
      'EmulationStation',
      'Display',
      'Audio',
      'Language & region',
      'System',
      'Storage'
    ]);
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['About this box']));
    expect(rows()[0]).toBe('Batocera');
    await press('back');
    // 36 languages: a list to pick from, starting on the one in use.
    while (selected() !== 'Language') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Language']));
    expect(selected()).toBe('✓ English (US)');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['System settings']));
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')).toEqual([
        'English (UK) · after Forma restarts'
      ])
    );
  });

  it('system settings: writing to gamelist.xml is off until turned on', async () => {
    const { press, text, openMenu, forma } = start(['system.settings', 'settings.system']);
    await openMenu('System');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(text('.page .heading')).toContain('EmulationStation'));
    while (!selected()?.startsWith('Save to gamelist.xml')) await press('down');
    expect(selected()).toBe('Save to gamelist.xml: Off');
    await press('confirm');
    await vi.waitFor(() => expect(selected()).toBe('Save to gamelist.xml: On'));
    expect(await forma.settings.getSystem('library.gamelistWriteBack')).toBe(true);
  });

  it('turns Batocera labels into sentence case, keeping acronyms', async () => {
    const { sentence } = await import('./ui/settings.js');
    expect(sentence('GAME ASPECT RATIO')).toBe('Game aspect ratio');
    expect(sentence('ENABLE AI TRANSLATION SERVICE')).toBe('Enable AI translation service');
    expect(sentence('STRETCH BEZELS (4K & ULTRAWIDE)')).toBe('Stretch bezels (4K & ultrawide)');
    expect(sentence('Already fine')).toBe('Already fine');
  });

  it('content downloader: a category, an install with its progress, and removing it', async () => {
    const { press, text, openMenu } = start(['store.manage']);
    await openMenu('Downloads');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Refresh the list'));
    // One package has a newer version; systems by their names in the library.
    expect(rows()[0]).toBe('Update all (1)');
    expect(rows()).toContain('Nintendo Entertainment System');
    expect(rows().indexOf('Decorations')).toBeLessThan(rows().indexOf('Other'));
    while (selected() !== 'Nintendo Entertainment System') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Nintendo Entertainment System']));
    const before = text('.page li.selected .description')[0]!;
    expect(before).not.toContain('Installed');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page .note')[0]).toMatch(/^Downloading .*%$/));
    expect(text('.page .busy .loader')).toHaveLength(1);
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')[0]).toContain('Installed')
    );
    expect(text('.toast')[0]).toMatch(/ installed$/);
    await press('north');
    await vi.waitFor(() => expect(text('.toast')[0]).toMatch(/ removed$/));
    expect(text('.page li.selected .description')[0]).toBe(before);
  });

  it('controllers: the pads in use, pairing a new one, connecting and forgetting', async () => {
    const { press, text, openMenu } = start(['system.controllers']);
    await openMenu('Controllers');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Pair a controller…'));
    expect(rows()[0]).toBe('Player 1');
    expect(rows()).toContain('8BitDo Pro 2');
    while (selected() !== 'Pair a controller…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.toast')[0]).toBe('Paired Wireless Controller'));
    await vi.waitFor(() => expect(rows()).toContain('Wireless Controller'));
    while (selected() !== 'PLAYSTATION(R)3 Controller') await press('down');
    expect(text('.page li.selected .description')[0]).toBe('Not connected');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page li.selected .description')[0]).toBe('Connected'));
    await press('north');
    await vi.waitFor(() => expect(rows()).not.toContain('PLAYSTATION(R)3 Controller'));
  });

  it('Batocera updates: the versions, then the update only after asking twice', async () => {
    const { press, text, openMenu } = start(['system.settings', 'system.update']);
    await openMenu('System');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Batocera updates…'));
    while (selected() !== 'Batocera updates…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()).toContain('Update to Batocera 44…'));
    expect(text('.page li[role=option] .description').slice(0, 2)).toEqual([
      'Batocera 43.1',
      'Batocera 44'
    ]);
    while (selected() !== 'Update to Batocera 44…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Update to Batocera 44?']));
    await press('confirm');
    await vi.waitFor(() =>
      expect(text('.toast')[0]).toBe('Batocera 44 installed: restart to use it')
    );
    await vi.waitFor(() => expect(rows()).toEqual(['Installed', 'Restart now']));
  });

  it('RetroAchievements: how many games are matched, per system', async () => {
    const { press, text, openMenu } = start(['settings.batocera', 'settings.system']);
    await openMenu('Achievements');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Games matched'));
    while (selected() !== 'Games matched') await press('down');
    expect(text('.page li.selected .description')[0]).toBe('4 of 7');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Games matched']));
    expect(text('.page li[role=option] .description')[0]).toBe('3 of 4 matched · 1 not found');
  });

  it('after a game, shows what it earned on RetroAchievements, and goes when another starts', async () => {
    const { text, shell } = start([]);
    const achievement = {
      id: 1,
      title: 'Warp Zone',
      description: 'Find a warp pipe',
      points: 10,
      badge: null,
      earnedAt: '2026-11-03 10:00:00',
      earnedHardcoreAt: null
    };
    await shell.showUnlocks({
      gameId: 2,
      game: {
        raGameId: 1446,
        title: 'Super Mario Bros.',
        console: 'NES',
        icon: null,
        achievementCount: 4,
        earned: 3,
        earnedHardcore: 0,
        completion: 0.75,
        achievements: []
      },
      unlocked: [achievement, { ...achievement, id: 2, title: 'Princess', points: 25 }],
      points: 35
    });
    expect(text('.unlocks .kicker')).toEqual(['2 achievements unlocked']);
    expect(text('.unlocks .title')).toEqual(['Warp Zone', 'Princess']);
    expect(text('.unlocks .total')).toEqual(['+35 points']);
    expect(text('.unlocks .progress')).toEqual(['3 of 4 · 75%']);
    shell.playing = true;
    await shell.showUnlocks({
      gameId: 2,
      game: {
        raGameId: 1,
        title: 'x',
        console: '',
        icon: null,
        achievementCount: 1,
        earned: 1,
        earnedHardcore: 0,
        completion: 1,
        achievements: []
      },
      unlocked: [achievement],
      points: 10
    });
    // Not over a game: the card still shows the first session.
    expect(text('.unlocks .kicker')).toEqual(['2 achievements unlocked']);
  });
});

describe('Forma’s screensaver', () => {
  /** Start → Screensaver, then down to the row whose label starts with `label`. */
  const toRow = async (h: ReturnType<typeof start>, label: string) => {
    for (let i = 0; i < 20 && !h.text('.page li.selected')[0]?.startsWith(label); i++) {
      await h.press('down');
    }
    expect(h.text('.page li.selected')[0]).toContain(label);
  };

  it('has ES’s settings under Start → Screensaver, saved as Forma’s', async () => {
    const h = start(['settings.system']);
    await h.openMenu('Screensaver');
    await vi.waitFor(() => expect(h.text('.page li')[0]).toContain('Start after: 5 minutes'));
    // Dim shows no slideshow rows.
    expect(h.text('.page li').some((r) => r.includes('Next game after'))).toBe(false);
    await h.press('right');
    await vi.waitFor(() => expect(h.text('.page li')[0]).toContain('Start after: 10 minutes'));
    await h.press('down');
    await h.press('right');
    await h.press('right');
    await h.press('right');
    await vi.waitFor(() => expect(h.text('.page li')[1]).toContain('Type: Videos'));
    const rows = h.text('.page li');
    for (const row of [
      'Next game after: 10 s',
      'Games: All games',
      'Game info: Always',
      'Sound: Off'
    ])
      expect(rows.some((r) => r.includes(row))).toBe(true);
    expect(await h.forma.settings.getSystem('screensaver')).toMatchObject({
      after: 10,
      type: 'video'
    });
  });

  it('starts after the minutes chosen, and the first press only wakes it', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { shell, root, saver, forma } = start(['settings.system']);
      await forma.settings.setSystem('screensaver', { after: 1, type: 'black' });
      await vi.advanceTimersByTimeAsync(10);
      shell.input({ action: 'down', button: null, gamepad: 0, pressed: true, repeat: false });
      await vi.advanceTimersByTimeAsync(59_000);
      expect(root.querySelector('.screensaver')!.hasAttribute('hidden')).toBe(true);
      await vi.advanceTimersByTimeAsync(2_000);
      expect(root.querySelector('.screensaver.type-black')!.hasAttribute('hidden')).toBe(false);
      expect(saver).toEqual([true]);
      shell.input({ action: 'start', button: null, gamepad: 0, pressed: true, repeat: false });
      await vi.advanceTimersByTimeAsync(10);
      expect(root.querySelector('.screensaver')!.hasAttribute('hidden')).toBe(true);
      expect(saver).toEqual([true, false]);
      // Start woke it and did not open the menu.
      expect(root.querySelector('.menu')!.hasAttribute('hidden')).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows the library’s pictures with the game’s name, and changes game on request', async () => {
    const h = start(['settings.system']);
    await h.forma.settings.setSystem('screensaver', { type: 'slideshow', games: 'favorites' });
    await h.openMenu('Screensaver');
    await vi.waitFor(() => expect(h.text('.page li')[1]).toContain('Type: Slideshow'));
    await toRow(h, 'Start the screensaver now');
    await h.press('confirm');
    const img = await vi.waitFor(() => {
      const found = h.root.querySelector<HTMLImageElement>('.screensaver .stage img');
      expect(found).not.toBeNull();
      return found!;
    });
    expect(decodeURIComponent(img.src)).toContain('Super Mario Bros.');
    // jsdom loads no pictures: as if it had.
    img.dispatchEvent(new Event('load'));
    await vi.waitFor(() =>
      expect(h.text('.screensaver .info .name')).toEqual(['Super Mario Bros.'])
    );
    expect(h.root.querySelector('.page')!.hasAttribute('hidden')).toBe(true);
    // Left and right are its controls; B wakes it.
    await h.press('right');
    expect(h.root.querySelector('.screensaver')!.hasAttribute('hidden')).toBe(false);
    await h.press('back');
    expect(h.root.querySelector('.screensaver')!.hasAttribute('hidden')).toBe(true);
    expect(h.saver).toEqual([true, false]);
  });

  it('says so when no game has what it would show', async () => {
    const h = start(['settings.system']);
    await h.forma.settings.setSystem('screensaver', { type: 'slideshow', picture: 'fanart' });
    await h.openMenu('Screensaver');
    await toRow(h, 'Start the screensaver now');
    await h.press('confirm');
    await vi.waitFor(() =>
      expect(h.text('.screensaver .info .name')).toEqual(['No game has that picture yet'])
    );
  });
});

describe('ROM check', () => {
  it('shows each system’s dump check under Start → Games, and turns it off', async () => {
    const { press, text, openMenu, forma } = start(['settings.system']);
    await openMenu('Games');
    const rows = () => text('.page > .panel li[role=option] .name');
    const details = () => text('.page > .panel li[role=option] .description');
    await vi.waitFor(() => expect(rows()).toContain('ROM check'));
    for (let i = 0; i < 2; i++) await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(rows()[0]).toBe('Check ROMs: On'));
    const nes = rows().findIndex(
      (r) => r.startsWith('Nintendo Entertainment System') || r === 'nes'
    );
    expect(nes).toBeGreaterThan(0);
    expect(details()[nes]).toMatch(/^1 of \d+ games verified · 1 not catalogued/);
    await press('confirm');
    await vi.waitFor(() => expect(rows()[0]).toBe('Check ROMs: Off'));
    expect(await forma.settings.getSystem('library.verifyDumps')).toBe(false);
  });
});
