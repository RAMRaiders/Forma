// @vitest-environment jsdom
/**
 * Forma's menus, driven like a gamepad would, against the SDK's mock. The mock grants the
 * permissions a test asks for; inside Forma the shell's token lets it past them whatever the
 * theme declares, which `ipc_tests.rs` checks on the Rust side.
 */
import { Forma, type InputEvent, type ShellContext } from '@ramraiders/forma-sdk';
import { mockBackend } from '@ramraiders/forma-sdk/mock';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Shell, type ShellInfo } from './shell.js';

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
  const forma = new Forma(backend, '1.18.0', false);
  const open: boolean[] = [];
  const opened: string[] = [];
  const settings: [string, unknown][] = [];
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
    setThemeSetting: (key, value) => {
      settings.push([key, value]);
      const setting = theme.settings.find((s) => s.key === key);
      if (setting) setting.value = value;
      return Promise.resolve();
    }
  });
  document.body.append(shell.root);
  const root = shell.root;

  const press = async (action: InputEvent['action'], repeat = false) => {
    shell.input({ action, button: null, gamepad: 0, pressed: true, repeat });
    await vi.waitFor(() => Promise.resolve());
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
  return { shell, root, press, text, forma, open, opened, settings, openMenu, backend };
}

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
    await openMenu('Music');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Music']));
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Music']));
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
    await openMenu('Music');
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

  it('on Select over systems, offers their order, for every theme, and shuts on Select', async () => {
    const { press, text, shell, forma } = start(
      ['settings.system'],
      { view: 'systems', list: 'systems' },
      ['systems']
    );
    await press('select');
    await vi.waitFor(() => expect(text('.page li .name').length).toBeGreaterThan(0));
    expect(text('.page h2')).toEqual(['Systems']);
    expect(text('.page li .name')).toEqual(['Order: Name (A to Z)']);
    await press('right');
    await vi.waitFor(() => expect(text('.page li .name')).toEqual(['Order: Manufacturer']));
    expect(await forma.settings.getSystem('library.systemsOrder')).toBe('maker');
    const makers = (await forma.library.systems()).map((s) => s.manufacturer ?? '');
    // By maker; those without one last.
    const named = makers.filter(Boolean);
    expect(named).toEqual([...named].sort((a, b) => a.localeCompare(b)));
    expect(makers.slice(named.length).every((m) => m === '')).toBe(true);
    await press('left');
    await press('left');
    await vi.waitFor(() => expect(text('.page li .name')).toEqual(['Order: Year']));
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
      'Achievements',
      'Netplay',
      'Interface',
      'Music',
      'Games',
      'System',
      'Controllers',
      'Network',
      'Switch to ES',
      'Restart',
      'Shut down'
    ]);
    expect(text('.menu h3')).toEqual(['Library', 'Play', 'Settings', 'Devices', 'Power']);
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
    expect(tiles.length).toBe(14);
    const at = () => text('.menu li.selected .name')[0];
    expect(at()).toBe('Collections');
    await press('right');
    expect(at()).toBe('Views');
    await press('down');
    // the tile below, in the next row
    expect(at()).toBe('Netplay');
    await press('down');
    expect(at()).toBe('Music');
    await press('right');
    await press('right');
    expect(at()).toBe('System');
    await press('down');
    // the next row down, nearest across
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
    for (let i = 0; i < 12; i++) await press('down');
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
    for (let i = 0; i < 5; i++) await press('down');
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
        permissions: [],
        current: true,
        unavailable: null,
        logo: 'forma://localhost/theme/cyber/logo.svg'
      }
    ]);
    await press('start');
    for (let i = 0; i < 5; i++) await press('down');
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
    for (let i = 0; i < 10; i++) await press('down');
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
    await openMenu('Music');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('Chiptune'));
    expect(rows()[0]).toBe('Nothing playing');
    expect(text('.page li.heading')).toEqual(['Folders and playlists', 'Web radios']);

    // Down to Chiptune: Nothing playing, Volume, Shuffle, Show each new song, Music, Chiptune.
    for (let i = 0; i < 5; i++) await press('down');
    expect(text('.page li.selected .name')).toEqual(['Chiptune']);
    await press('confirm');
    await vi.waitFor(() => expect(rows()[0]).toBe('Track 1'));
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
    await vi.waitFor(() => expect(rows()).toContain('Game aspect ratio'));
    expect(text('.page h2')).toEqual(['Game settings']);
    expect(rows()).toContain('Game rendering & shaders…');
    while (selected() !== 'Game aspect ratio') await press('down');
    expect(text('.page li.selected .description')).toEqual(['Default']);
    await press('right');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['4/3']));
    // Its own page: B comes back to the row it was opened from.
    while (selected() !== 'Game rendering & shaders…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Game rendering & shaders']));
    expect(rows()).toEqual(['Shader set', 'Integer scaling (pixel perfect)']);
    await press('back');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Game settings']));
    expect(selected()).toBe('Game rendering & shaders…');
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
    await press('right');
    await press('right');
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
